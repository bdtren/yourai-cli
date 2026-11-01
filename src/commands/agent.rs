//! Tools for the `yourai chat` agent. File tools only see the configured workspace.

use std::path::{Path, PathBuf};

use console::style;
use serde_json::{json, Value};

use super::media::Gateway;
use crate::core::config::Config;
use crate::core::paths;
use crate::ui;

/// Folders the agent never lists or reads: secrets, caches, and dependencies.
const HIDDEN: [&str; 7] = [
    ".yourai",
    "yourai_modules",
    ".git",
    "node_modules",
    "target",
    ".venv",
    "__pycache__",
];
const MAX_LIST: usize = 300;

pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    pub fn new(project_root: &Path, workspace: &str) -> anyhow::Result<Sandbox> {
        let dir = paths::inside(project_root, workspace)?;
        std::fs::create_dir_all(&dir)?;
        Ok(Sandbox {
            root: dunce(std::fs::canonicalize(&dir)?),
        })
    }

    /// Resolves a path the model asked for. It must be relative, stay inside the
    /// workspace (also after following symlinks), and avoid hidden folders like .yourai.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let rel = rel.trim().trim_start_matches("./");
        let rel = if rel.is_empty() { "." } else { rel };
        let p = paths::inside(&self.root, rel).map_err(|e| e.to_string())?;
        if Path::new(rel)
            .components()
            .any(|c| HIDDEN.iter().any(|h| c.as_os_str() == *h))
        {
            return Err(format!("`{rel}` is in a folder the agent may not access"));
        }
        let mut probe = p.clone();
        while !probe.exists() {
            if !probe.pop() {
                break;
            }
        }
        let real = std::fs::canonicalize(&probe).map(dunce).map_err(|e| e.to_string())?;
        if !real.starts_with(&self.root) {
            return Err(format!("`{rel}` points outside the workspace"));
        }
        Ok(p)
    }

    fn display(&self, p: &Path) -> String {
        let rel = p
            .strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() {
            ".".into()
        } else {
            rel
        }
    }

    pub fn list(&self, rel: &str) -> Result<String, String> {
        let start = self.resolve(rel)?;
        if !start.is_dir() {
            return Err(format!("`{rel}` is not a folder"));
        }
        let mut out = Vec::new();
        let mut stack = vec![start];
        while let Some(dir) = stack.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().to_string();
                if HIDDEN.contains(&name.as_str()) {
                    continue;
                }
                let path = e.path();
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    out.push(format!("{}/", self.display(&path)));
                    stack.push(path);
                } else {
                    out.push(format!("{} ({})", self.display(&path), ui::gb(meta.len() as f64 / 1e9)));
                }
                if out.len() >= MAX_LIST {
                    out.push(format!("… stopped after {MAX_LIST} entries; list a subfolder"));
                    return Ok(out.join("\n"));
                }
            }
        }
        Ok(if out.is_empty() {
            "(empty folder)".into()
        } else {
            out.join("\n")
        })
    }

    pub fn read(&self, rel: &str, max_chars: usize) -> Result<String, String> {
        let p = self.resolve(rel)?;
        if !p.is_file() {
            return Err(format!("`{rel}` is not a file"));
        }
        let is_pdf = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        let text = if is_pdf {
            pdf_extract::extract_text(&p).map_err(|e| format!("could not read the PDF: {e}"))?
        } else {
            let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
            if bytes.iter().take(8000).any(|b| *b == 0) {
                return Err(format!("`{rel}` is a binary file, not text"));
            }
            String::from_utf8_lossy(&bytes).to_string()
        };
        let total = text.chars().count();
        if total > max_chars {
            let cut: String = text.chars().take(max_chars).collect();
            return Ok(format!(
                "{cut}\n… [truncated: showing {max_chars} of {total} characters]"
            ));
        }
        Ok(text)
    }

    pub fn write(&self, rel: &str, content: &str) -> Result<String, String> {
        let p = self.resolve(rel)?;
        if p.is_dir() {
            return Err(format!("`{rel}` is a folder"));
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, content).map_err(|e| e.to_string())?;
        Ok(format!("wrote {} ({} bytes)", self.display(&p), content.len()))
    }
}

/// Windows canonical paths start with `\\?\`; strip it so prefixes compare cleanly.
fn dunce(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest.to_string()),
        _ => p,
    }
}

fn function(name: &str, description: &str, params: Value) -> Value {
    json!({"type": "function", "function": {"name": name, "description": description, "parameters": params}})
}

fn string_params(fields: &[(&str, &str)], required: &[&str]) -> Value {
    let props: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(k, d)| (k.to_string(), json!({"type": "string", "description": d})))
        .collect();
    json!({"type": "object", "properties": props, "required": required})
}

/// OpenAI-style tool definitions for the tools turned on in yourai.toml.
pub fn definitions(cfg: &Config) -> Vec<Value> {
    cfg.agent_tools()
        .into_iter()
        .map(|t| match t {
            "list_files" => function(
                "list_files",
                "List files and folders in the user's workspace.",
                string_params(&[("path", "Folder relative to the workspace, \".\" for the top")], &[]),
            ),
            "read_file" => function(
                "read_file",
                "Read a text, code, Markdown, or PDF file from the workspace.",
                string_params(&[("path", "File path relative to the workspace")], &["path"]),
            ),
            "write_file" => function(
                "write_file",
                "Create or overwrite a text file in the workspace. The user is asked to confirm.",
                string_params(
                    &[
                        ("path", "File path relative to the workspace"),
                        ("content", "The full file content"),
                    ],
                    &["path", "content"],
                ),
            ),
            "generate_image" => function(
                "generate_image",
                "Create an image from a text description. Returns the saved file path.",
                string_params(
                    &[
                        ("prompt", "What the image shows, in detail"),
                        ("size", "Optional WIDTHxHEIGHT, like 512x512"),
                    ],
                    &["prompt"],
                ),
            ),
            "generate_video" => function(
                "generate_video",
                "Create a short video clip from a text description. Takes minutes. Returns the saved file path.",
                string_params(&[("prompt", "What happens in the video")], &["prompt"]),
            ),
            "speak" => function(
                "speak",
                "Read text aloud and save it as a WAV file. Returns the saved file path.",
                string_params(&[("text", "The text to speak")], &["text"]),
            ),
            "transcribe" => function(
                "transcribe",
                "Turn an audio file from the workspace into text.",
                string_params(&[("path", "Audio file path relative to the workspace")], &["path"]),
            ),
            other => unreachable!("unknown tool {other}"),
        })
        .collect()
}

pub struct Agent<'a> {
    pub cfg: &'a Config,
    pub sandbox: Sandbox,
    pub gateway: &'a Gateway,
    pub max_chars: usize,
}

fn arg<'v>(args: &'v Value, key: &str) -> &'v str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}

fn short(s: &str, n: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() > n {
        format!("{}…", one_line.chars().take(n).collect::<String>())
    } else {
        one_line
    }
}

impl Agent<'_> {
    /// Runs one tool call and returns the text sent back to the model.
    pub fn call(&self, name: &str, raw_args: &str) -> String {
        let args: Value = serde_json::from_str(raw_args).unwrap_or_else(|_| json!({}));
        let shown = match name {
            "write_file" => format!(
                "{}({}, {} chars)",
                name,
                arg(&args, "path"),
                arg(&args, "content").len()
            ),
            _ => format!("{}({})", name, short(&args.to_string(), 80)),
        };
        println!("{} {}", style("⚙").yellow(), style(shown).dim());
        if !self.cfg.agent_tools().contains(&name) {
            return format!("error: the tool `{name}` is not available");
        }
        let res: Result<String, String> = match name {
            "list_files" => {
                let p = arg(&args, "path");
                self.sandbox.list(if p.is_empty() { "." } else { p })
            }
            "read_file" => self.sandbox.read(arg(&args, "path"), self.max_chars),
            "write_file" => self.write(arg(&args, "path"), arg(&args, "content")),
            "generate_image" => {
                let mut body = json!({"prompt": arg(&args, "prompt")});
                if !arg(&args, "size").is_empty() {
                    body["size"] = json!(arg(&args, "size"));
                }
                self.media("Creating the image…", || {
                    self.gateway.image(body).map(|f| f.join(", "))
                })
            }
            "generate_video" => {
                let body = json!({"prompt": arg(&args, "prompt")});
                self.media("Creating the video (this takes minutes)…", || {
                    self.gateway.video(body).map(|f| f.join(", "))
                })
            }
            "speak" => self.media("Speaking…", || self.gateway.speak(arg(&args, "text"), None)),
            "transcribe" => match self.sandbox.resolve(arg(&args, "path")) {
                Ok(p) => self.media("Transcribing…", || self.gateway.transcribe(&p, None)),
                Err(e) => Err(e),
            },
            _ => Err(format!("unknown tool `{name}`")),
        };
        match res {
            Ok(s) => {
                if matches!(name, "generate_image" | "generate_video" | "speak") {
                    ui::ok(format!("saved {s}"));
                    format!("saved to {s}")
                } else {
                    s
                }
            }
            Err(e) => {
                ui::warn(&e);
                format!("error: {e}")
            }
        }
    }

    fn media(&self, msg: &str, f: impl FnOnce() -> anyhow::Result<String>) -> Result<String, String> {
        let pb = ui::spinner(msg.to_string());
        let r = f();
        pb.finish_and_clear();
        r.map_err(|e| e.to_string())
    }

    fn write(&self, rel: &str, content: &str) -> Result<String, String> {
        let p = self.sandbox.resolve(rel)?;
        let verb = if p.exists() { "Overwrite" } else { "Create" };
        println!(
            "{}",
            style(format!("--- {rel} ({} lines) ---", content.lines().count())).dim()
        );
        for l in content.lines().take(15) {
            println!("{}", style(format!("  {l}")).dim());
        }
        if content.lines().count() > 15 {
            println!("{}", style("  …").dim());
        }
        let ok = ui::confirm(&format!("{verb} {rel}?"), false, false).map_err(|e| e.to_string())?;
        if !ok {
            return Err("the user declined to write this file".into());
        }
        self.sandbox.write(rel, content)
    }
}

/// Builds an OpenAI content part for an image or audio file the user attached.
pub fn attachment(path: &Path, audio: bool) -> anyhow::Result<Value> {
    use base64::Engine as _;
    let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    if bytes.len() > 20 * 1024 * 1024 {
        anyhow::bail!("{} is larger than 20 MB", path.display());
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    if audio {
        let format = match ext.as_str() {
            "wav" | "mp3" => ext.as_str(),
            _ => anyhow::bail!("audio must be a .wav or .mp3 file"),
        };
        return Ok(json!({"type": "input_audio", "input_audio": {"data": b64, "format": format}}));
    }
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => anyhow::bail!("images must be .png, .jpg, .webp, .gif, or .bmp"),
    };
    Ok(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{b64}")}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> (tempfile::TempDir, Sandbox) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join(".yourai")).unwrap();
        std::fs::write(root.join("docs/a.md"), "hello").unwrap();
        std::fs::write(root.join(".yourai/secrets.toml"), "api_key = \"x\"").unwrap();
        std::fs::write(tmp.path().join("outside.txt"), "secret").unwrap();
        let s = Sandbox::new(&root, ".").unwrap();
        (tmp, s)
    }

    #[test]
    fn sandbox_blocks_escapes_and_secrets() {
        let (tmp, s) = sandbox();
        assert_eq!(s.read("docs/a.md", 100).unwrap(), "hello");
        assert!(s.read("../outside.txt", 100).is_err());
        assert!(s
            .read(&tmp.path().join("outside.txt").display().to_string(), 100)
            .is_err());
        assert!(s.read("docs/../../outside.txt", 100).is_err());
        assert!(s.read(".yourai/secrets.toml", 100).is_err());
        assert!(!s.list(".").unwrap().contains(".yourai"));
        assert!(s.write("../evil.txt", "x").is_err());
        assert!(s.write("notes/new.md", "hi").is_ok());
        assert_eq!(s.read("notes/new.md", 100).unwrap(), "hi");
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_blocks_symlinks_out() {
        let (tmp, s) = sandbox();
        let root = tmp.path().join("proj");
        std::os::unix::fs::symlink(tmp.path().join("outside.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), root.join("up")).unwrap();
        assert!(s.read("link.txt", 100).is_err());
        assert!(s.read("up/outside.txt", 100).is_err());
        assert!(s.write("up/x.txt", "x").is_err());
    }

    #[test]
    fn reads_are_truncated_and_binary_rejected() {
        let (tmp, s) = sandbox();
        let root = tmp.path().join("proj");
        std::fs::write(root.join("big.txt"), "x".repeat(500)).unwrap();
        assert!(s.read("big.txt", 100).unwrap().contains("truncated"));
        std::fs::write(root.join("bin.dat"), [0u8, 1, 2]).unwrap();
        assert!(s.read("bin.dat", 100).is_err());
    }

    #[test]
    fn tools_follow_config() {
        let mut cfg = crate::core::config::sample();
        assert!(definitions(&cfg).is_empty());
        cfg.agent.enabled = true;
        let names: Vec<String> = definitions(&cfg)
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["list_files", "read_file"]);
    }
}

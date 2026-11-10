use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::lifecycle;
use crate::core::project::Project;
use crate::core::secrets;
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

/// Talks to a running project's gateway.
pub struct Gateway {
    base: String,
    key: Option<String>,
    http: reqwest::blocking::Client,
    root: PathBuf,
}

impl Gateway {
    pub fn connect(p: &Project) -> anyhow::Result<Gateway> {
        let cfg = p.config()?;
        if lifecycle::live_state(p).state != "running" {
            return fail_hint(Exit::Runtime, "the AI is not running", "run `yourai start` first");
        }
        Ok(Gateway {
            base: cfg.server.local_url(),
            key: secrets::api_key(&p.root),
            http: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(None)
                .build()?,
            root: p.root.clone(),
        })
    }

    fn post(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let rb = self.http.post(format!("{}{path}", self.base));
        match &self.key {
            Some(k) => rb.bearer_auth(k),
            None => rb,
        }
    }

    fn check(r: reqwest::blocking::Response) -> anyhow::Result<reqwest::blocking::Response> {
        if r.status().is_success() {
            return Ok(r);
        }
        let status = r.status();
        let text = r.text().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
            .unwrap_or(text);
        let code = if status.as_u16() == 404 {
            Exit::InvalidConfig
        } else {
            Exit::Runtime
        };
        Err(CliError::new(code, msg)
            .hint("see `yourai status` and the logs in .yourai/")
            .into())
    }

    fn paths(v: &Value) -> Vec<String> {
        v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["path"].as_str().map(str::to_string))
            .collect()
    }

    /// Encode a local image file as a data URL for the gateway.
    pub fn encode_file(&self, path: &Path) -> anyhow::Result<String> {
        let bytes = std::fs::read(path)
            .map_err(|e| CliError::new(Exit::InvalidConfig, format!("cannot read {}: {e}", path.display())))?;
        if bytes.is_empty() {
            return fail_hint(Exit::InvalidConfig, "image file is empty", "pick a PNG, JPEG, or WebP file");
        }
        if bytes.len() > 20 * 1024 * 1024 {
            return fail_hint(Exit::InvalidConfig, "image is larger than 20 MB", "use a smaller file");
        }
        let mime = match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "image/png",
        };
        use base64::Engine as _;
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    /// Returns the saved files, relative to the project.
    pub fn image(&self, body: Value) -> anyhow::Result<Vec<String>> {
        let mut body = body;
        body["response_format"] = json!("url");
        let r = Self::check(self.post("/v1/images/generations").json(&body).send()?)?;
        Ok(Self::paths(&r.json()?))
    }

    /// Image edit (img2img) via `/v1/images/edits`.
    pub fn image_edit(&self, body: Value) -> anyhow::Result<Vec<String>> {
        let mut body = body;
        body["response_format"] = json!("url");
        let r = Self::check(self.post("/v1/images/edits").json(&body).send()?)?;
        Ok(Self::paths(&r.json()?))
    }

    pub fn video(&self, body: Value) -> anyhow::Result<Vec<String>> {
        let r = Self::check(self.post("/v1/videos/generations").json(&body).send()?)?;
        Ok(Self::paths(&r.json()?))
    }

    pub fn speak(&self, text: &str, language: Option<&str>) -> anyhow::Result<String> {
        let mut body = json!({"input": text});
        if let Some(l) = language {
            body["language"] = json!(l);
        }
        let r = Self::check(self.post("/v1/audio/speech").json(&body).send()?)?;
        let path = r
            .headers()
            .get("x-yourai-file")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        match path {
            Some(p) => Ok(p),
            None => {
                let bytes = r.bytes()?;
                let rel = format!("outputs/audio/speech-{}.wav", std::process::id());
                std::fs::create_dir_all(self.root.join("outputs/audio"))?;
                std::fs::write(self.root.join(&rel), bytes)?;
                Ok(rel)
            }
        }
    }

    pub fn transcribe(&self, file: &Path, language: Option<&str>) -> anyhow::Result<String> {
        let bytes = std::fs::read(file)
            .map_err(|e| CliError::new(Exit::InvalidConfig, format!("cannot read {}: {e}", file.display())))?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio.wav".into());
        let mut form = reqwest::blocking::multipart::Form::new()
            .part("file", reqwest::blocking::multipart::Part::bytes(bytes).file_name(name))
            .text("response_format", "json");
        if let Some(l) = language {
            form = form.text("language", l.to_string());
        }
        let r = Self::check(self.post("/v1/audio/transcriptions").multipart(form).send()?)?;
        let v: Value = r.json()?;
        Ok(v["text"].as_str().unwrap_or("").trim().to_string())
    }
}

fn spinner_while<T>(msg: &str, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<(T, f64)> {
    let pb = ui::spinner(msg.to_string());
    let t0 = Instant::now();
    let r = f();
    pb.finish_and_clear();
    Ok((r?, t0.elapsed().as_secs_f64()))
}

pub fn image(
    p: &Project,
    prompt: &str,
    size: Option<String>,
    n: u32,
    steps: Option<u32>,
    seed: Option<i64>,
    negative: Option<String>,
    image: Option<PathBuf>,
    strength: Option<f64>,
) -> anyhow::Result<()> {
    let gw = Gateway::connect(p)?;
    let mut body = json!({"prompt": prompt, "n": n.clamp(1, 4)});
    if let Some(s) = size {
        body["size"] = json!(s);
    }
    if let Some(s) = steps {
        body["steps"] = json!(s);
    }
    if let Some(s) = seed {
        body["seed"] = json!(s);
    }
    if let Some(s) = negative {
        body["negative_prompt"] = json!(s);
    }
    let edit = image.is_some();
    if let Some(path) = image {
        body["image"] = json!(gw.encode_file(&path)?);
        if let Some(s) = strength {
            body["strength"] = json!(s);
        }
    } else if strength.is_some() {
        ui::warn("--strength is only used with --image");
    }
    let msg = if edit {
        "Editing the image (the first one also loads the model)…"
    } else {
        "Creating the image (the first one also loads the model)…"
    };
    let (files, secs) = spinner_while(msg, || {
        if edit {
            gw.image_edit(body)
        } else {
            gw.image(body)
        }
    })?;
    for f in &files {
        ui::ok(format!("saved {f}"));
    }
    ui::info(format!("took {secs:.0}s"));
    Ok(())
}

pub fn video(
    p: &Project,
    prompt: &str,
    size: Option<String>,
    frames: Option<u32>,
    fps: Option<u32>,
    steps: Option<u32>,
    seed: Option<i64>,
    image: Option<PathBuf>,
    strength: Option<f64>,
) -> anyhow::Result<()> {
    let gw = Gateway::connect(p)?;
    let mut body = json!({"prompt": prompt});
    for (k, v) in [("frames", frames), ("fps", fps), ("steps", steps)] {
        if let Some(v) = v {
            body[k] = json!(v);
        }
    }
    if let Some(s) = size {
        body["size"] = json!(s);
    }
    if let Some(s) = seed {
        body["seed"] = json!(s);
    }
    if let Some(path) = image {
        body["image"] = json!(gw.encode_file(&path)?);
        if let Some(s) = strength {
            body["strength"] = json!(s);
        }
    } else if strength.is_some() {
        ui::warn("--strength is only used with --image");
    }
    let (files, secs) = spinner_while("Creating the video (this takes several minutes)…", || gw.video(body))?;
    for f in &files {
        ui::ok(format!("saved {f}"));
    }
    ui::info(format!("took {:.0}m {:.0}s", (secs / 60.0).floor(), secs % 60.0));
    Ok(())
}

pub fn speak(p: &Project, text: &str, language: Option<String>, out: Option<PathBuf>) -> anyhow::Result<()> {
    let gw = Gateway::connect(p)?;
    let (rel, secs) = spinner_while("Speaking…", || gw.speak(text, language.as_deref()))?;
    ui::ok(format!("saved {rel} ({secs:.1}s)"));
    if let Some(o) = out {
        std::fs::copy(p.root.join(&rel), &o)?;
        ui::ok(format!("copied to {}", o.display()));
    }
    Ok(())
}

pub fn transcribe(p: &Project, file: &Path, language: Option<String>, out: Option<PathBuf>) -> anyhow::Result<()> {
    let gw = Gateway::connect(p)?;
    let (text, secs) = spinner_while("Transcribing…", || gw.transcribe(file, language.as_deref()))?;
    match out {
        Some(o) => {
            std::fs::write(&o, format!("{text}\n"))?;
            ui::ok(format!("wrote {} ({secs:.1}s)", o.display()));
        }
        None => println!("{text}"),
    }
    Ok(())
}

//! Project-scoped Ollama: serve YourAI-downloaded GGUF under `yourai_modules/ollama`.
//! Never uses `ollama pull` or the user's `~/.ollama` store for project models.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::{CliError, Exit};
use crate::ui;

pub const MODEL_NAME: &str = "yourai-chat";

/// `OLLAMA_MODELS` under the project module (or shared home when project_modules = false).
pub fn ollama_models_dir(module_or_home: &Path) -> PathBuf {
    module_or_home.join("ollama")
}

pub fn find_binary() -> anyhow::Result<PathBuf> {
    super::paths::which("ollama").ok_or_else(|| {
        CliError::new(
            Exit::Unsupported,
            "runtime.engine is \"ollama\" but the `ollama` binary was not found on PATH",
        )
        .hint("install Ollama from https://ollama.com, or set runtime.engine = \"llamacpp\"")
        .into()
    })
}

pub fn modelfile_contents(gguf: &Path, context_length: u32) -> String {
    format!(
        "FROM {}\nPARAMETER num_ctx {}\n",
        gguf.display(),
        context_length.max(256)
    )
}

pub fn write_modelfile(dir: &Path, gguf: &Path, context_length: u32) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let abs = if gguf.is_absolute() {
        gguf.to_path_buf()
    } else {
        std::fs::canonicalize(gguf).unwrap_or_else(|_| gguf.to_path_buf())
    };
    let path = dir.join("Modelfile");
    std::fs::write(&path, modelfile_contents(&abs, context_length))?;
    Ok(path)
}

/// Prepare the project Ollama store (Modelfile). Import happens when the project serve starts.
pub fn prepare_store(ollama_models: &Path, gguf: &Path, context_length: u32) -> anyhow::Result<()> {
    if !gguf.is_file() {
        return Err(CliError::new(
            Exit::InvalidConfig,
            format!("model file missing: {}", gguf.display()),
        )
        .hint("run `yourai build` to download the GGUF first")
        .into());
    }
    write_modelfile(ollama_models, gguf, context_length)?;
    ui::ok(format!(
        "Ollama Modelfile ready under {} (import on start; not ~/.ollama)",
        ollama_models.display()
    ));
    Ok(())
}

/// Import the project GGUF into the already-running project-scoped `ollama serve`.
pub fn create_against_host(
    ollama_bin: &Path,
    ollama_models: &Path,
    host: &str,
    port: u16,
) -> anyhow::Result<()> {
    let modelfile = ollama_models.join("Modelfile");
    if !modelfile.is_file() {
        return Err(CliError::new(
            Exit::InvalidConfig,
            format!("missing {}", modelfile.display()),
        )
        .hint("run `yourai rebuild`")
        .into());
    }
    ui::info(format!("importing project GGUF into Ollama as `{MODEL_NAME}`"));
    let out = Command::new(ollama_bin)
        .args(["create", MODEL_NAME, "-f"])
        .arg(&modelfile)
        .env("OLLAMA_MODELS", ollama_models)
        .env("OLLAMA_HOST", format!("{host}:{port}"))
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(CliError::new(
            Exit::Runtime,
            format!("`ollama create {MODEL_NAME}` failed"),
        )
        .hint(format!("{stdout}{stderr}").trim().to_string())
        .into());
    }
    Ok(())
}

pub fn serve_env(host: &str, port: u16, ollama_models: &Path) -> Vec<(String, String)> {
    vec![
        ("OLLAMA_HOST".into(), format!("{host}:{port}")),
        ("OLLAMA_MODELS".into(), ollama_models.display().to_string()),
    ]
}

pub fn serve_args() -> Vec<String> {
    vec!["serve".into()]
}

pub fn chat_model_id() -> &'static str {
    MODEL_NAME
}

/// True if `/api/tags` JSON lists our project model.
pub fn tags_list_has_model(body: &str) -> bool {
    body.contains(MODEL_NAME)
}

/// Poll until the serve answers, then ensure `yourai-chat` exists (create if needed).
pub fn ensure_model_imported(
    ollama_bin: &Path,
    ollama_models: &Path,
    host: &str,
    port: u16,
) -> anyhow::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let url = format!("http://{host}:{port}/api/tags");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(r) = client.get(&url).send() {
            if r.status().is_success() {
                let body = r.text().unwrap_or_default();
                if tags_list_has_model(&body) {
                    return Ok(());
                }
                create_against_host(ollama_bin, ollama_models, host, port)?;
                // re-check
                if let Ok(r2) = client.get(&url).send() {
                    let body = r2.text().unwrap_or_default();
                    if tags_list_has_model(&body) {
                        ui::ok(format!("Ollama model `{MODEL_NAME}` ready"));
                        return Ok(());
                    }
                }
            }
        }
        if Instant::now() > deadline {
            return Err(CliError::new(
                Exit::Runtime,
                "project Ollama serve did not become ready",
            )
            .hint(format!("check the server log; expected model `{MODEL_NAME}` under {}", ollama_models.display()))
            .into());
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modelfile_points_at_absolute_gguf() {
        let p = Path::new("/proj/yourai_modules/models/custom/a__b/m.gguf");
        let t = modelfile_contents(p, 4096);
        assert!(t.contains("FROM /proj/yourai_modules/models/custom/a__b/m.gguf"));
        assert!(t.contains("PARAMETER num_ctx 4096"));
    }

    #[test]
    fn ollama_models_under_module() {
        let d = ollama_models_dir(Path::new("/p/yourai_modules"));
        assert_eq!(d, PathBuf::from("/p/yourai_modules/ollama"));
    }

    #[test]
    fn tags_detects_model_name() {
        assert!(tags_list_has_model(r#"{"models":[{"name":"yourai-chat:latest"}]}"#));
        assert!(!tags_list_has_model(r#"{"models":[]}"#));
    }
}

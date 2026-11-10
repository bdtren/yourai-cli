use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config::{self, Config};
use super::lock::{self, Lock};
use super::store::{self, MODULE_DIR};
use crate::error::{CliError, Exit};

pub const CONFIG_FILE: &str = "yourai.toml";
pub const LOCK_FILE: &str = "yourai.lock";

pub struct Project {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// "running" | "paused" | "stopped"
    pub state: String,
    pub pid: Option<u32>,
    pub pid_start_time: Option<u64>,
    pub port: Option<u16>,
    pub host: Option<String>,
    pub engine_version: Option<String>,
    pub started_at: Option<u64>,
}

impl Project {
    /// Uses `explicit` if given, otherwise walks up from the current folder.
    pub fn discover(explicit: Option<&Path>) -> anyhow::Result<Project> {
        if let Some(p) = explicit {
            let root = p.to_path_buf();
            if !root.join(CONFIG_FILE).is_file() {
                return Err(
                    CliError::new(Exit::InvalidConfig, format!("no {CONFIG_FILE} in {}", root.display()))
                        .hint("create a project with `yourai init`")
                        .into(),
                );
            }
            return Ok(Project {
                root: std::fs::canonicalize(root)?,
            });
        }
        let mut dir = std::env::current_dir()?;
        loop {
            if dir.join(CONFIG_FILE).is_file() {
                return Ok(Project { root: dir });
            }
            if !dir.pop() {
                return Err(
                    CliError::new(Exit::InvalidConfig, "this folder is not a YourAI project")
                        .hint("cd into your project folder, pass --project PATH, or create one with `yourai init`")
                        .into(),
                );
            }
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn config(&self) -> anyhow::Result<Config> {
        Config::load(&self.config_path())
    }

    /// Rewrites an older yourai.toml in the current schema, keeping comments.
    pub fn upgrade_config(&self) -> anyhow::Result<bool> {
        let text = std::fs::read_to_string(self.config_path())?;
        match config::migrate(&text)? {
            Some(new) => {
                std::fs::write(self.config_path(), new)?;
                crate::ui::info(format!(
                    "upgraded {CONFIG_FILE} to schema {} (new capabilities are off until you enable them)",
                    config::SCHEMA_VERSION
                ));
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn outputs_dir(&self) -> PathBuf {
        self.root.join("outputs")
    }

    pub fn engines_path(&self) -> PathBuf {
        self.state_dir().join("engines.json")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".yourai")
    }

    pub fn module_dir(&self) -> PathBuf {
        self.root.join(MODULE_DIR)
    }

    pub fn log_path(&self) -> PathBuf {
        self.state_dir().join("server.log")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    /// Older builds wrote `.yourai/lock.json`.
    pub fn legacy_lock_path(&self) -> PathBuf {
        self.state_dir().join("lock.json")
    }

    /// Resolve a path stored in yourai.lock (relative to the project, or legacy absolute).
    pub fn resolve_path(&self, stored: &str) -> anyhow::Result<PathBuf> {
        store::resolve_lock_path(&self.root, stored)
    }

    /// Root `yourai.lock` only. Start/runtime ignore legacy `.yourai/lock.json`
    /// and locks that don't match `[store] project_modules` (module paths vs shared home).
    pub fn lock(&self) -> Option<Lock> {
        let l = lock::load(&self.lock_path())?;
        let project_modules = self.config().map(|c| c.store.project_modules).unwrap_or(true);
        if project_modules {
            if !self.module_dir().is_dir() || !store::lock_uses_module(&l) {
                return None;
            }
        } else if !store::lock_uses_shared(&l) {
            return None;
        }
        Some(l)
    }

    /// Any lock on disk (root or legacy), for `yourai build` migration.
    pub fn any_lock(&self) -> Option<Lock> {
        lock::load(&self.lock_path()).or_else(|| lock::load(&self.legacy_lock_path()))
    }

    pub fn system_prompt(&self, cfg: &Config) -> String {
        super::paths::inside(&self.root, &cfg.system_prompt)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
    }

    pub fn state(&self) -> State {
        std::fs::read_to_string(self.state_dir().join("state.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| State {
                state: "stopped".into(),
                ..Default::default()
            })
    }

    pub fn save_state(&self, s: &State) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.state_dir())?;
        std::fs::write(self.state_dir().join("state.json"), serde_json::to_string_pretty(s)?)?;
        Ok(())
    }
}

const SYSTEM_CHAT: &str = include_str!("../templates/system_chat.md");
const SYSTEM_CODE: &str = include_str!("../templates/system_code.md");
const README: &str = include_str!("../templates/README.md");
const GITIGNORE: &str = include_str!("../templates/gitignore");
const START_SH: &str = include_str!("../templates/start.sh");
const STOP_SH: &str = include_str!("../templates/stop.sh");
const START_PS1: &str = include_str!("../templates/start.ps1");
const STOP_PS1: &str = include_str!("../templates/stop.ps1");
const WEB_INDEX: &str = include_str!("../templates/web/index.html");
const WEB_APP_JS: &str = include_str!("../templates/web/app.js");
const WEB_CSS: &str = include_str!("../templates/web/styles.css");
const API_OPENAI_README: &str = include_str!("../templates/api/openai/README.md");
const API_OPENAI_HTTP: &str = include_str!("../templates/api/openai/chat.http");
const API_REALTIME_README: &str = include_str!("../templates/api/realtime/README.md");
const API_REALTIME_JS: &str = include_str!("../templates/api/realtime/chat-ws.js");
const API_CLAUDE_README: &str = include_str!("../templates/api/claude/README.md");
const API_CLAUDE_HTTP: &str = include_str!("../templates/api/claude/messages.http");
const API_GEMINI_README: &str = include_str!("../templates/api/gemini/README.md");
const API_GEMINI_HTTP: &str = include_str!("../templates/api/gemini/generate.http");
const API_HANDLER_ECHO: &str = include_str!("../templates/api/handlers/example_echo.js");
const API_HANDLER_STREAM: &str = include_str!("../templates/api/handlers/example_stream.js");

fn write_api_scaffolds(dir: &Path, cfg: &Config) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("api/handlers"))?;
    std::fs::write(
        dir.join("api/routes.toml"),
        super::api::routes::scaffold_toml(&cfg.server.api_styles),
    )?;
    std::fs::write(dir.join("api/handlers/example_echo.js"), API_HANDLER_ECHO)?;
    std::fs::write(dir.join("api/handlers/example_stream.js"), API_HANDLER_STREAM)?;
    for style in &cfg.server.api_styles {
        match style.as_str() {
            "openai" => {
                std::fs::create_dir_all(dir.join("api/openai"))?;
                std::fs::create_dir_all(dir.join("api/realtime"))?;
                std::fs::write(dir.join("api/openai/README.md"), API_OPENAI_README)?;
                std::fs::write(dir.join("api/openai/chat.http"), API_OPENAI_HTTP)?;
                std::fs::write(dir.join("api/realtime/README.md"), API_REALTIME_README)?;
                std::fs::write(dir.join("api/realtime/chat-ws.js"), API_REALTIME_JS)?;
            }
            "claude" => {
                std::fs::create_dir_all(dir.join("api/claude"))?;
                std::fs::write(dir.join("api/claude/README.md"), API_CLAUDE_README)?;
                std::fs::write(dir.join("api/claude/messages.http"), API_CLAUDE_HTTP)?;
            }
            "gemini" => {
                std::fs::create_dir_all(dir.join("api/gemini"))?;
                std::fs::write(dir.join("api/gemini/README.md"), API_GEMINI_README)?;
                std::fs::write(dir.join("api/gemini/generate.http"), API_GEMINI_HTTP)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Creates a new project folder. Refuses to write into a non-empty folder.
pub fn scaffold(dir: &Path, cfg: &Config, api_key: Option<&str>) -> anyhow::Result<()> {
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        return Err(CliError::new(
            Exit::General,
            format!("{} already exists and is not empty", dir.display()),
        )
        .hint("choose another name or folder; existing files are never overwritten")
        .into());
    }
    for sub in ["prompts", "data/docs", "data/voices", "outputs", "scripts", ".yourai"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join(CONFIG_FILE), config::render(cfg))?;
    let prompt = if cfg.goal == "code" { SYSTEM_CODE } else { SYSTEM_CHAT };
    std::fs::write(dir.join("prompts/system.md"), prompt)?;
    std::fs::write(dir.join("README.md"), README.replace("{{name}}", &cfg.name))?;
    std::fs::write(dir.join(".gitignore"), GITIGNORE)?;
    std::fs::write(dir.join("data/docs/.gitkeep"), "")?;
    std::fs::write(dir.join("data/voices/.gitkeep"), "")?;
    // Keep web/ even when customize_web_ui is false so flipping the flag needs no re-scaffold.
    // Skip only when browser chat is off entirely (API-only projects).
    if cfg.server.web_ui {
        std::fs::create_dir_all(dir.join("web"))?;
        std::fs::write(dir.join("web/index.html"), WEB_INDEX.replace("{{name}}", &cfg.name))?;
        std::fs::write(dir.join("web/app.js"), WEB_APP_JS)?;
        std::fs::write(dir.join("web/styles.css"), WEB_CSS)?;
    }
    write_api_scaffolds(dir, cfg)?;
    for (name, body) in [
        ("start.sh", START_SH),
        ("stop.sh", STOP_SH),
        ("start.ps1", START_PS1),
        ("stop.ps1", STOP_PS1),
    ] {
        let p = dir.join("scripts").join(name);
        std::fs::write(&p, body)?;
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    if let Some(key) = api_key {
        super::secrets::write_key(dir, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_writes_valid_project_and_refuses_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let cfg = config::sample();
        scaffold(&dir, &cfg, None).unwrap();
        let p = Project { root: dir.clone() };
        assert_eq!(p.config().unwrap(), cfg);
        assert!(!p.system_prompt(&cfg).is_empty());
        assert!(scaffold(&dir, &cfg, None).is_err());
    }

    #[test]
    fn lock_ignores_legacy_absolute_paths_and_missing_module_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let cfg = config::sample();
        scaffold(&dir, &cfg, None).unwrap();
        let p = Project { root: dir.clone() };

        // Absolute paths under ~/.yourai (legacy) → not a runtime lock in module mode.
        let legacy = Lock {
            engine_exe: "/Users/me/.yourai/engines/llama/bin/llama-server".into(),
            model_path: "/Users/me/.yourai/models/m.gguf".into(),
            ..Default::default()
        };
        lock::save(&p.lock_path(), &legacy).unwrap();
        assert!(p.lock().is_none());
        assert!(p.any_lock().is_some());

        // Module-relative paths but no yourai_modules/ folder → refuse.
        let good = Lock {
            engine_exe: "yourai_modules/engines/llama/bin/llama-server".into(),
            model_path: "yourai_modules/models/m.gguf".into(),
            ..Default::default()
        };
        lock::save(&p.lock_path(), &good).unwrap();
        assert!(p.lock().is_none());

        std::fs::create_dir_all(p.module_dir()).unwrap();
        assert!(p.lock().is_some());
    }

    #[test]
    fn lock_accepts_shared_home_paths_when_project_modules_off() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        // Re-set before each check: other tests also touch YOURAI_HOME.
        std::env::set_var("YOURAI_HOME", &home);
        let dir = tmp.path().join("p");
        let mut cfg = config::sample();
        cfg.store.project_modules = false;
        scaffold(&dir, &cfg, None).unwrap();
        let p = Project { root: dir };

        let shared = Lock {
            engine_exe: home.join("engines/llama/bin/llama-server").to_string_lossy().into(),
            model_path: home.join("models/m.gguf").to_string_lossy().into(),
            ..Default::default()
        };
        lock::save(&p.lock_path(), &shared).unwrap();
        std::env::set_var("YOURAI_HOME", &home);
        assert!(p.lock().is_some());

        // Module-relative lock is refused in shared mode.
        let module = Lock {
            engine_exe: "yourai_modules/engines/llama/bin/llama-server".into(),
            model_path: "yourai_modules/models/m.gguf".into(),
            ..Default::default()
        };
        lock::save(&p.lock_path(), &module).unwrap();
        std::env::set_var("YOURAI_HOME", &home);
        assert!(p.lock().is_none());
        std::env::remove_var("YOURAI_HOME");
    }

    #[test]
    fn scaffold_includes_web_ui() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        scaffold(&dir, &config::sample(), None).unwrap();
        assert!(dir.join("web/index.html").is_file());
        assert!(dir.join("web/app.js").is_file());
        assert!(!dir.join("web/api").exists());
        assert!(dir.join("api/routes.toml").is_file());
        assert!(dir.join("api/handlers/example_echo.js").is_file());
        assert!(dir.join("api/handlers/example_stream.js").is_file());
        assert!(dir.join("api/openai/chat.http").is_file());
        assert!(dir.join("api/realtime/README.md").is_file());
        let routes = std::fs::read_to_string(dir.join("api/routes.toml")).unwrap();
        assert!(routes.contains("upstream = \"images_generations\""));
        assert!(routes.contains("stream = true"));
    }

    #[test]
    fn scaffold_skips_web_when_web_ui_off() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let mut cfg = config::sample();
        cfg.server.web_ui = false;
        scaffold(&dir, &cfg, None).unwrap();
        assert!(!dir.join("web").exists());
        assert!(dir.join("api/routes.toml").is_file());
    }

    #[test]
    fn scaffold_api_styles_select_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let mut cfg = config::sample();
        cfg.server.api_styles = vec!["claude".into(), "gemini".into()];
        scaffold(&dir, &cfg, None).unwrap();
        assert!(dir.join("api/claude/messages.http").is_file());
        assert!(dir.join("api/gemini/generate.http").is_file());
        assert!(!dir.join("api/openai").exists());
        assert!(!dir.join("api/realtime").exists());
        let routes = std::fs::read_to_string(dir.join("api/routes.toml")).unwrap();
        assert!(routes.contains("upstream = \"claude_messages\""));
        assert!(routes.contains("upstream = \"gemini_generate\""));
        assert!(!routes.contains("upstream = \"openai_chat_ws\""));
        assert!(!routes.contains("path = \"/v1/chat/ws\""));
    }
}

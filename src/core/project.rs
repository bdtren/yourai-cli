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
    /// and locks that still point outside `yourai_modules/`.
    pub fn lock(&self) -> Option<Lock> {
        let l = lock::load(&self.lock_path())?;
        if !self.module_dir().is_dir() || !store::lock_uses_module(&l) {
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
    for sub in ["prompts", "data/docs", "outputs", "scripts", ".yourai"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join(CONFIG_FILE), config::render(cfg))?;
    let prompt = if cfg.goal == "code" { SYSTEM_CODE } else { SYSTEM_CHAT };
    std::fs::write(dir.join("prompts/system.md"), prompt)?;
    std::fs::write(dir.join("README.md"), README.replace("{{name}}", &cfg.name))?;
    std::fs::write(dir.join(".gitignore"), GITIGNORE)?;
    std::fs::write(dir.join("data/docs/.gitkeep"), "")?;
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

        // Absolute paths under ~/.yourai (legacy) → not a runtime lock.
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
}

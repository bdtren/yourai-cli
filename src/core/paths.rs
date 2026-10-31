use std::path::{Path, PathBuf};

/// Root of the shared cache. Override with YOURAI_HOME (used by tests).
pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("YOURAI_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".yourai")
}

pub fn models() -> PathBuf {
    home().join("models")
}

pub fn engines() -> PathBuf {
    home().join("engines")
}

pub fn downloads() -> PathBuf {
    home().join("downloads")
}

pub fn bin() -> PathBuf {
    home().join("bin")
}

pub fn catalog_cache() -> PathBuf {
    home().join("catalog.json")
}

/// Written by the installers to the real home folder, so YOURAI_HOME does not move it.
pub fn receipt() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".yourai")
        .join("install-receipt.json")
}

pub fn update_check() -> PathBuf {
    home().join("update-check.json")
}

pub fn licenses() -> PathBuf {
    home().join("accepted-licenses.json")
}

pub fn base_url() -> String {
    std::env::var("YOURAI_BASE_URL").unwrap_or_else(|_| "https://yourai.bdtren.com/cli".into())
}

pub fn ensure_home() -> anyhow::Result<()> {
    for d in [home(), models(), engines(), downloads()] {
        std::fs::create_dir_all(&d)?;
    }
    Ok(())
}

/// Finds an executable on PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Rejects relative paths that escape `root` (for config values that point at project files).
pub fn inside(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    use std::path::Component;
    let p = Path::new(rel);
    // On Windows `/etc/x` is not "absolute" (no drive) but join() still replaces the root.
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        anyhow::bail!("path `{rel}` must be relative and stay inside the project folder");
    }
    Ok(root.join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_rejects_escape() {
        let root = Path::new("/tmp/p");
        assert!(inside(root, "prompts/system.md").is_ok());
        assert!(inside(root, "../x").is_err());
        assert!(inside(root, "/etc/passwd").is_err());
        assert!(inside(root, "a/../../x").is_err());
        if cfg!(windows) {
            assert!(inside(root, r"\Windows\x").is_err());
            assert!(inside(root, r"C:\x").is_err());
            assert!(inside(root, "C:x").is_err());
        }
    }
}

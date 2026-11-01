//! Project-local `yourai_modules/` plus optional shared `~/.yourai` cache.
//!
//! Runtime always uses files under the project module. The shared cache is only
//! read for a fast copy, and only written when `--save` is set.

use std::path::{Path, PathBuf};

use super::paths;
use crate::ui;

pub const MODULE_DIR: &str = "yourai_modules";

#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    /// Also write downloaded files into the shared ~/.yourai cache.
    pub save: bool,
    /// Skip the shared cache and download from the network.
    pub force: bool,
}

/// Where this project keeps models and engines.
pub struct Store {
    pub root: PathBuf,
    pub opts: Opts,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, opts: Opts) -> Self {
        Self {
            root: root.into(),
            opts,
        }
    }

    pub fn module_dir(&self) -> PathBuf {
        self.root.join(MODULE_DIR)
    }

    pub fn models_dir(&self) -> PathBuf {
        self.module_dir().join("models")
    }

    pub fn engines_dir(&self) -> PathBuf {
        self.module_dir().join("engines")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.module_dir().join("downloads")
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        for d in [self.models_dir(), self.engines_dir(), self.downloads_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// Shared cache roots (YOURAI_HOME / ~/.yourai).
    pub fn cache_models() -> PathBuf {
        paths::models()
    }

    pub fn cache_engines() -> PathBuf {
        paths::engines()
    }

    /// Path under the module → matching path under the shared models cache.
    pub fn cache_for_model(&self, module_path: &Path) -> Option<PathBuf> {
        let rel = module_path.strip_prefix(self.models_dir()).ok()?;
        Some(Self::cache_models().join(rel))
    }

    /// Project-relative path for the lock file (`yourai_modules/...`).
    pub fn rel(&self, abs: &Path) -> anyhow::Result<String> {
        let rel = abs
            .strip_prefix(&self.root)
            .map_err(|_| anyhow::anyhow!("path {} is outside the project {}", abs.display(), self.root.display()))?;
        let s = rel.to_string_lossy().replace('\\', "/");
        if s.is_empty() || s.contains("..") {
            anyhow::bail!("refusing to store unsafe lock path `{s}`");
        }
        Ok(s)
    }
}

/// Resolve a path stored in yourai.lock against the project root.
pub fn resolve_lock_path(root: &Path, stored: &str) -> anyhow::Result<PathBuf> {
    let p = Path::new(stored);
    if p.is_absolute() {
        // Legacy locks pointed at ~/.yourai with absolute paths.
        return Ok(p.to_path_buf());
    }
    use std::path::Component;
    if p.components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        anyhow::bail!("lock path `{stored}` must stay inside the project");
    }
    Ok(root.join(p))
}

/// True when `stored` is a project-relative path under `yourai_modules/`.
pub fn path_in_module(stored: &str) -> bool {
    let s = stored.replace('\\', "/");
    !Path::new(stored).is_absolute()
        && (s == MODULE_DIR || s.starts_with(&format!("{MODULE_DIR}/")))
        && !s.contains("..")
}

/// Every filesystem path in the lock must live under `yourai_modules/`.
pub fn lock_uses_module(lk: &super::lock::Lock) -> bool {
    let mut paths = vec![lk.engine_exe.as_str(), lk.model_path.as_str()];
    if !lk.mmproj_path.is_empty() {
        paths.push(lk.mmproj_path.as_str());
    }
    for e in lk.engines.values() {
        paths.push(e.exe.as_str());
    }
    for m in lk.models.values() {
        for f in m.files.values() {
            paths.push(f.as_str());
        }
    }
    paths.into_iter().all(path_in_module)
}

/// Rename a leftover 0.0.4 `yourai_module/` folder to `yourai_modules/`.
pub fn rename_singular_module_dir(root: &Path) -> anyhow::Result<()> {
    let old = root.join("yourai_module");
    let new = root.join(MODULE_DIR);
    if old.is_dir() && !new.exists() {
        ui::info("renaming yourai_module/ to yourai_modules/");
        std::fs::rename(&old, &new)?;
    }
    Ok(())
}

/// Hardlink when possible, otherwise copy. Overwrites `dest` if it exists.
pub fn link_or_copy(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        if dest.is_dir() {
            std::fs::remove_dir_all(dest)?;
        } else {
            std::fs::remove_file(dest)?;
        }
    }
    match std::fs::hard_link(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dest)?;
            Ok(())
        }
    }
}

/// Recursively hardlink-or-copy a directory tree.
pub fn link_or_copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if src.is_file() {
        return link_or_copy(src, dest);
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        let from = entry.path();
        if from.is_dir() {
            link_or_copy_tree(&from, &to)?;
        } else {
            link_or_copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Ensure `dest` (under yourai_modules) has the file. Uses the shared cache unless `force`.
/// When `save` is set after a download (or force), also updates the cache copy.
pub fn ensure_file(
    store: &Store,
    dest: &Path,
    cache: Option<&Path>,
    download: impl FnOnce(&Path) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let force = store.opts.force;
    let save = store.opts.save;

    // Prefer an existing project file unless --force.
    if !force && dest.is_file() {
        return download(dest);
    }

    // Fast path: copy from the shared cache into the project.
    if !force {
        if let Some(c) = cache {
            if c.is_file() {
                ui::info(format!("copying from shared cache {}", c.display()));
                link_or_copy(c, dest)?;
                let marker = format!("{}.sha256", c.display());
                if Path::new(&marker).is_file() {
                    let _ = link_or_copy(Path::new(&marker), Path::new(&format!("{}.sha256", dest.display())));
                }
                // Verify / record via the same download helper (no network if hash matches).
                return download(dest);
            }
        }
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if force && dest.exists() {
        let _ = std::fs::remove_file(dest);
        let _ = std::fs::remove_file(format!("{}.sha256", dest.display()));
    }
    let sha = download(dest)?;

    if save {
        if let Some(c) = cache {
            ui::info(format!("saving to shared cache {}", c.display()));
            link_or_copy(dest, c)?;
            let dest_marker = format!("{}.sha256", dest.display());
            if Path::new(&dest_marker).is_file() {
                let _ = link_or_copy(Path::new(&dest_marker), Path::new(&format!("{}.sha256", c.display())));
            }
        }
    }
    Ok(sha)
}

/// Copy a ready engine tree from the shared cache into the module, if allowed.
/// Returns true when the module dir was filled from the cache.
pub fn try_copy_engine_from_cache(store: &Store, dest_dir: &Path, cache_dir: &Path) -> anyhow::Result<bool> {
    if store.opts.force {
        return Ok(false);
    }
    if !cache_dir.join(".installed").is_file() {
        return Ok(false);
    }
    ui::info(format!("copying engine from shared cache {}", cache_dir.display()));
    if dest_dir.exists() {
        let _ = std::fs::remove_dir_all(dest_dir);
    }
    link_or_copy_tree(cache_dir, dest_dir)?;
    Ok(true)
}

/// After a fresh install into the module, optionally mirror it into the shared cache.
pub fn maybe_save_engine_dir(store: &Store, dest_dir: &Path, cache_dir: &Path) -> anyhow::Result<()> {
    if !store.opts.save {
        return Ok(());
    }
    ui::info(format!("saving engine to shared cache {}", cache_dir.display()));
    if cache_dir.exists() {
        let _ = std::fs::remove_dir_all(cache_dir);
    }
    link_or_copy_tree(dest_dir, cache_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_and_resolve_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path(), Opts::default());
        let abs = store.models_dir().join("m").join("f.gguf");
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, b"x").unwrap();
        let rel = store.rel(&abs).unwrap();
        assert!(rel.starts_with("yourai_modules/models/"));
        let back = resolve_lock_path(tmp.path(), &rel).unwrap();
        assert_eq!(back, abs);
    }

    #[test]
    fn resolve_rejects_escape() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_lock_path(tmp.path(), "../etc/passwd").is_err());
    }

    #[test]
    fn path_in_module_accepts_only_relative_module_paths() {
        assert!(path_in_module("yourai_modules/models/m.gguf"));
        assert!(path_in_module("yourai_modules"));
        assert!(!path_in_module("yourai_module/models/m.gguf"));
        assert!(!path_in_module("/home/me/.yourai/models/m.gguf"));
        assert!(!path_in_module("models/m.gguf"));
        assert!(!path_in_module("yourai_modules/../etc/passwd"));
    }

    #[test]
    fn lock_uses_module_rejects_absolute_legacy_paths() {
        let mut lk = super::super::lock::Lock {
            engine_exe: "yourai_modules/engines/llama/bin/llama-server".into(),
            model_path: "yourai_modules/models/m.gguf".into(),
            ..Default::default()
        };
        assert!(lock_uses_module(&lk));
        lk.model_path = "/Users/me/.yourai/models/m.gguf".into();
        assert!(!lock_uses_module(&lk));
    }

    #[test]
    fn rename_singular_module_dir_moves_legacy_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("yourai_module");
        std::fs::create_dir_all(old.join("models")).unwrap();
        std::fs::write(old.join("models/x.gguf"), b"x").unwrap();
        rename_singular_module_dir(tmp.path()).unwrap();
        assert!(!old.exists());
        assert!(tmp.path().join("yourai_modules/models/x.gguf").is_file());
        // Idempotent when plural already exists.
        rename_singular_module_dir(tmp.path()).unwrap();
    }
}

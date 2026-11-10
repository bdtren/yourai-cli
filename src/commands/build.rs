use std::path::{Path, PathBuf};

use crate::core::catalog::Catalog;
use crate::core::config::{Capability, Config};
use crate::core::engine::{self, EngineSpec};
use crate::core::hardware::{self, Hardware};
use crate::core::lock::{self, EngineLock, Lock, ModelLock};
use crate::core::models::{self, Resolved};
use crate::core::ollama;
use crate::core::project::Project;
use crate::core::store::{self, Opts, Store};
use crate::core::{paths, secrets};
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

fn engine_lock(
    store: &Store,
    catalog: &Catalog,
    spec: &EngineSpec,
    key: String,
    exe: &Path,
) -> anyhow::Result<EngineLock> {
    Ok(EngineLock {
        version: catalog.engine(spec.name).map(|e| e.version.clone()).unwrap_or_default(),
        key,
        exe: store.rel(exe)?,
    })
}

fn install_engine(
    store: &Store,
    hw: &Hardware,
    catalog: &Catalog,
    spec: &EngineSpec,
    lock: &mut Lock,
) -> anyhow::Result<()> {
    if lock.engines.contains_key(spec.name) {
        return Ok(());
    }
    let (key, exe) = engine::ensure_spec(store, hw, catalog, spec)?;
    lock.engines
        .insert(spec.name.to_string(), engine_lock(store, catalog, spec, key, &exe)?);
    Ok(())
}

fn rel_files(store: &Store, r: &Resolved) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(r.main_role(), store.rel(&r.path)?);
    for f in &r.extras {
        m.insert(f.role.clone(), store.rel(&f.path)?);
    }
    Ok(m)
}

/// Copy a legacy absolute path into the project module and return the new absolute path.
fn migrate_file(store: &Store, abs: &Path) -> anyhow::Result<PathBuf> {
    if !abs.is_file() {
        return Ok(abs.to_path_buf());
    }
    // Already under this project's module.
    if abs.starts_with(store.module_dir()) {
        return Ok(abs.to_path_buf());
    }
    let name = abs.file_name().unwrap_or_default();
    let parent_name = abs
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "migrated".into());
    let dest = if abs.starts_with(paths::models()) {
        let rel = abs.strip_prefix(paths::models()).unwrap_or(Path::new(name));
        store.models_dir().join(rel)
    } else if abs.starts_with(paths::engines()) {
        let rel = abs.strip_prefix(paths::engines()).unwrap_or(Path::new(name));
        store.engines_dir().join(rel)
    } else {
        store.models_dir().join("migrated").join(&parent_name).join(name)
    };
    if !dest.is_file() {
        store::link_or_copy(abs, &dest)?;
        let marker = format!("{}.sha256", abs.display());
        if Path::new(&marker).is_file() {
            let _ = store::link_or_copy(Path::new(&marker), Path::new(&format!("{}.sha256", dest.display())));
        }
    }
    Ok(dest)
}

fn migrate_engine_tree(store: &Store, exe: &Path) -> anyhow::Result<PathBuf> {
    if !exe.is_file() {
        return Ok(exe.to_path_buf());
    }
    if exe.starts_with(store.module_dir()) {
        return Ok(exe.to_path_buf());
    }
    // Walk up until we find `.installed` or the engines root.
    let mut dir = exe.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d.join(".installed").is_file() || d.parent() == Some(&paths::engines()) {
            let rel = d
                .strip_prefix(paths::engines())
                .unwrap_or(d.file_name().map(Path::new).unwrap_or(Path::new("engine")));
            let dest_dir = store.engines_dir().join(rel);
            if !dest_dir.join(".installed").is_file() {
                store::link_or_copy_tree(&d, &dest_dir)?;
            }
            let rel_exe = exe.strip_prefix(&d).unwrap_or(Path::new(exe.file_name().unwrap()));
            return Ok(dest_dir.join(rel_exe));
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    Ok(exe.to_path_buf())
}

/// If only a legacy `.yourai/lock.json` exists, copy artifacts into `yourai_modules/` and
/// rewrite paths into a new root `yourai.lock`.
fn migrate_legacy_lock(p: &Project, store: &Store) -> anyhow::Result<Option<Lock>> {
    // Already a valid module-based root lock — nothing to migrate.
    if let Some(lk) = lock::load(&p.lock_path()) {
        if store::lock_uses_module(&lk) {
            return Ok(None);
        }
        // Root lock still points at ~/.yourai — rewrite into yourai_modules/.
        ui::info("rewriting yourai.lock paths into yourai_modules/");
        return Ok(Some(rewrite_lock_into_module(p, store, lk)?));
    }
    let Some(lk) = p.any_lock() else {
        return Ok(None);
    };
    ui::info("migrating .yourai/lock.json into yourai.lock and yourai_modules/");
    Ok(Some(rewrite_lock_into_module(p, store, lk)?))
}

fn rewrite_lock_into_module(p: &Project, store: &Store, mut lk: Lock) -> anyhow::Result<Lock> {
    store.ensure_dirs()?;
    let exe = migrate_engine_tree(store, Path::new(&lk.engine_exe))?;
    lk.engine_exe = store.rel(&exe)?;
    let model = migrate_file(store, Path::new(&lk.model_path))?;
    lk.model_path = store.rel(&model)?;
    if !lk.mmproj_path.is_empty() {
        let mm = migrate_file(store, Path::new(&lk.mmproj_path))?;
        lk.mmproj_path = store.rel(&mm)?;
    }
    for el in lk.engines.values_mut() {
        let e = migrate_engine_tree(store, Path::new(&el.exe))?;
        el.exe = store.rel(&e)?;
    }
    for ml in lk.models.values_mut() {
        for path in ml.files.values_mut() {
            let f = migrate_file(store, Path::new(path))?;
            *path = store.rel(&f)?;
        }
    }
    lock::save(&p.lock_path(), &lk)?;
    ui::ok("migrated to yourai.lock");
    Ok(lk)
}

/// Makes sure the engines and models of every enabled capability exist under
/// `yourai_modules/`, then writes `yourai.lock`. Safe to run repeatedly.
pub fn prepare(p: &Project, cfg: &Config, accept_license: bool, opts: Opts) -> anyhow::Result<Lock> {
    if cfg.server.require_api_key && secrets::api_key(&p.root).is_none() {
        return fail_hint(
            Exit::Security,
            "server.require_api_key is true but no API key is set",
            "set YOURAI_API_KEY or create .yourai/secrets.toml with api_key = \"...\"",
        );
    }
    // Shared cache dirs (licenses, catalog) still live under YOURAI_HOME.
    paths::ensure_home()?;
    let project_modules = cfg.store.project_modules;
    if project_modules {
        store::rename_singular_module_dir(&p.root)?;
    }
    let store = Store::new(&p.root, opts, project_modules);
    store.ensure_dirs()?;
    if project_modules {
        let _ = migrate_legacy_lock(p, &store)?;
    }

    let hw = hardware::detect();
    let catalog = Catalog::load();
    let chat = models::resolve(cfg, &catalog, &store.models_dir())?;
    let mut caps: Vec<(Capability, Resolved)> = Vec::new();
    for c in cfg.enabled_caps() {
        caps.push((c, models::resolve_cap(cfg, c, &catalog, &store.models_dir())?));
    }
    for (c, r) in &caps {
        if let Some(m) = catalog.model(&r.id) {
            if let Err(why) = engine::model_fits(&hw, m) {
                ui::warn(format!("{}: {} {why}", c.label(), m.name));
            }
        }
    }

    let total: f64 = chat.download_gb() + caps.iter().map(|(_, r)| r.download_gb()).sum::<f64>();
    if !caps.is_empty() {
        let where_ = if project_modules {
            "yourai_modules/ or the shared cache"
        } else {
            "~/.yourai"
        };
        ui::info(format!(
            "this project uses {} of models in total (files already in {where_} are reused)",
            ui::gb(total)
        ));
    }
    models::accept_license(&chat, accept_license)?;
    for (_, r) in &caps {
        models::accept_license(r, accept_license)?;
    }

    let use_ollama = cfg.runtime.engine == "ollama";
    let need_llama = !use_ollama
        || caps.iter().any(|(c, _)| matches!(c, Capability::TextToSpeech));

    ui::heading("Engines");
    let mut lock = Lock {
        sections: lock::section_hashes(cfg, &p.system_prompt(cfg), &p.root),
        model_id: chat.id.clone(),
        model_quant: cfg.model.quant.clone(),
        ..Default::default()
    };

    let (engine_key, llama_exe) = if need_llama {
        let (key, exe) = engine::ensure(&store, &hw, &catalog)?;
        lock.engine_version = catalog.llama().version.clone();
        lock.engine_key = key.clone();
        if !use_ollama {
            lock.engine_exe = store.rel(&exe)?;
        }
        (key, Some(exe))
    } else {
        (String::new(), None)
    };

    if use_ollama {
        let ollama_bin = ollama::find_binary()?;
        lock.engine_version = "ollama".into();
        lock.engine_key = "ollama".into();
        lock.engine_exe = ollama_bin.display().to_string();
    }

    for (c, _) in &caps {
        match c {
            Capability::Image | Capability::Video => install_engine(&store, &hw, &catalog, &engine::SD, &mut lock)?,
            Capability::SpeechToText => install_engine(&store, &hw, &catalog, &engine::WHISPER, &mut lock)?,
            Capability::TextToSpeech => {
                let exe = llama_exe.as_ref().ok_or_else(|| {
                    CliError::new(Exit::Unsupported, "text_to_speech needs the llama.cpp engine")
                        .hint("run `yourai rebuild`, or turn off text_to_speech in yourai.toml")
                })?;
                let tts = engine::sibling(exe, "llama-tts").ok_or_else(|| {
                    CliError::new(Exit::Unsupported, "this llama.cpp build has no llama-tts")
                        .hint("run `yourai update`, or turn off text_to_speech in yourai.toml")
                })?;
                lock.engines.insert(
                    "llama-tts".into(),
                    EngineLock {
                        version: catalog.llama().version.clone(),
                        key: engine_key.clone(),
                        exe: store.rel(&tts)?,
                    },
                );
            }
        }
    }

    ui::heading("Models");
    lock.model_sha256 = models::ensure(&store, &chat)?;
    lock.model_path = store.rel(&chat.path)?;
    if let Some(f) = chat.extra("mmproj") {
        lock.mmproj_path = store.rel(&f.path)?;
    }
    if use_ollama {
        let _ = ollama::find_binary()?;
        let ollama_models = if store.project_modules {
            ollama::ollama_models_dir(&store.module_dir())
        } else {
            ollama::ollama_models_dir(&paths::home())
        };
        ollama::prepare_store(&ollama_models, &chat.path, cfg.runtime.context_length)?;
    }
    for (c, r) in &caps {
        let sha = models::ensure(&store, r)?;
        let quant = cfg.cap(*c).quant;
        let quant = if quant.is_empty() {
            catalog
                .model(&r.id)
                .map(|m| m.default_quant().to_string())
                .unwrap_or_default()
        } else {
            quant.to_string()
        };
        lock.models.insert(
            c.key().to_string(),
            ModelLock {
                id: r.id.clone(),
                quant,
                sha256: sha,
                files: rel_files(&store, r)?,
                main: r.main_role(),
                defaults: r.defaults.clone(),
            },
        );
    }
    lock::save(&p.lock_path(), &lock)?;
    if cfg.rag.enabled {
        ui::heading("Document index");
        crate::core::rag::ensure_index(p, &cfg.rag)?;
    }
    Ok(lock)
}

pub fn run(p: &Project, accept_license: bool, opts: Opts) -> anyhow::Result<()> {
    p.upgrade_config()?;
    let cfg = p.config()?;
    prepare(p, &cfg, accept_license, opts)?;
    ui::ok(format!("{} is built", cfg.name));
    ui::next("yourai start");
    Ok(())
}

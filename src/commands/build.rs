use crate::core::catalog::Catalog;
use crate::core::config::{Capability, Config};
use crate::core::engine::{self, EngineSpec};
use crate::core::hardware::{self, Hardware};
use crate::core::lock::{self, EngineLock, Lock, ModelLock};
use crate::core::models::{self, Resolved};
use crate::core::project::Project;
use crate::core::{paths, secrets};
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

fn engine_lock(catalog: &Catalog, spec: &EngineSpec, key: String, exe: &std::path::Path) -> EngineLock {
    EngineLock {
        version: catalog.engine(spec.name).map(|e| e.version.clone()).unwrap_or_default(),
        key,
        exe: exe.display().to_string(),
    }
}

fn install_engine(hw: &Hardware, catalog: &Catalog, spec: &EngineSpec, lock: &mut Lock) -> anyhow::Result<()> {
    if lock.engines.contains_key(spec.name) {
        return Ok(());
    }
    let (key, exe) = engine::ensure_spec(hw, catalog, spec)?;
    lock.engines
        .insert(spec.name.to_string(), engine_lock(catalog, spec, key, &exe));
    Ok(())
}

/// Makes sure the engines and models of every enabled capability exist, then writes
/// lock.json. Safe to run repeatedly: files already present are reused.
pub fn prepare(p: &Project, cfg: &Config, accept_license: bool) -> anyhow::Result<Lock> {
    if cfg.server.require_api_key && secrets::api_key(&p.root).is_none() {
        return fail_hint(
            Exit::Security,
            "server.require_api_key is true but no API key is set",
            "set YOURAI_API_KEY or create .yourai/secrets.toml with api_key = \"...\"",
        );
    }
    paths::ensure_home()?;
    let hw = hardware::detect();
    let catalog = Catalog::load();
    let chat = models::resolve(cfg, &catalog)?;
    let mut caps: Vec<(Capability, Resolved)> = Vec::new();
    for c in cfg.enabled_caps() {
        caps.push((c, models::resolve_cap(cfg, c, &catalog)?));
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
        ui::info(format!(
            "this project uses {} of models in total (files already downloaded are reused)",
            ui::gb(total)
        ));
    }
    models::accept_license(&chat, accept_license)?;
    for (_, r) in &caps {
        models::accept_license(r, accept_license)?;
    }

    ui::heading("Engines");
    let (engine_key, exe) = engine::ensure(&hw, &catalog)?;
    let mut lock = Lock {
        sections: lock::section_hashes(cfg, &p.system_prompt(cfg)),
        engine_version: catalog.llama().version.clone(),
        engine_key: engine_key.clone(),
        engine_exe: exe.display().to_string(),
        model_id: chat.id.clone(),
        model_quant: cfg.model.quant.clone(),
        ..Default::default()
    };
    for (c, _) in &caps {
        match c {
            Capability::Image | Capability::Video => install_engine(&hw, &catalog, &engine::SD, &mut lock)?,
            Capability::SpeechToText => install_engine(&hw, &catalog, &engine::WHISPER, &mut lock)?,
            Capability::TextToSpeech => {
                let tts = engine::sibling(&exe, "llama-tts").ok_or_else(|| {
                    CliError::new(Exit::Unsupported, "this llama.cpp build has no llama-tts")
                        .hint("run `yourai update`, or turn off text_to_speech in yourai.toml")
                })?;
                lock.engines.insert(
                    "llama-tts".into(),
                    EngineLock {
                        version: lock.engine_version.clone(),
                        key: engine_key.clone(),
                        exe: tts.display().to_string(),
                    },
                );
            }
        }
    }

    ui::heading("Models");
    lock.model_sha256 = models::ensure(&chat)?;
    lock.model_path = chat.path.display().to_string();
    if let Some(f) = chat.extra("mmproj") {
        lock.mmproj_path = f.path.display().to_string();
    }
    for (c, r) in &caps {
        let sha = models::ensure(r)?;
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
                files: r.files(),
                main: r.main_role(),
                defaults: r.defaults.clone(),
            },
        );
    }
    lock::save(&p.lock_path(), &lock)?;
    Ok(lock)
}

pub fn run(p: &Project, accept_license: bool) -> anyhow::Result<()> {
    p.upgrade_config()?;
    let cfg = p.config()?;
    prepare(p, &cfg, accept_license)?;
    ui::ok(format!("{} is built", cfg.name));
    ui::next("yourai start");
    Ok(())
}

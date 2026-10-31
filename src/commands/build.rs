use crate::core::catalog::Catalog;
use crate::core::config::Config;
use crate::core::lock::{self, Lock};
use crate::core::project::Project;
use crate::core::{engine, hardware, models, paths, secrets};
use crate::error::{fail_hint, Exit};
use crate::ui;

/// Makes sure the engine and model exist, then writes lock.json. Safe to run repeatedly.
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
    let model = models::resolve(cfg, &catalog)?;
    models::accept_license(&model, accept_license)?;

    ui::heading("Engine");
    let (engine_key, exe) = engine::ensure(&hw, &catalog)?;
    ui::heading("Model");
    let sha = models::ensure(&model)?;

    let lock = Lock {
        sections: lock::section_hashes(cfg, &p.system_prompt(cfg)),
        engine_version: catalog.llama().version.clone(),
        engine_key,
        engine_exe: exe.display().to_string(),
        model_id: model.id.clone(),
        model_quant: cfg.model.quant.clone(),
        model_path: model.path.display().to_string(),
        model_sha256: sha,
    };
    lock::save(&p.lock_path(), &lock)?;
    Ok(lock)
}

pub fn run(p: &Project, accept_license: bool) -> anyhow::Result<()> {
    let cfg = p.config()?;
    prepare(p, &cfg, accept_license)?;
    ui::ok(format!("{} is built", cfg.name));
    ui::next("yourai start");
    Ok(())
}

use std::path::Path;

use console::style;

use super::lifecycle;
use crate::core::catalog::Catalog;
use crate::core::config::Capability;
use crate::core::lock::EngineLock;
use crate::core::project::{Project, State};
use crate::core::store::{Opts, Store};
use crate::core::{engine, hardware, lock, models, paths};
use crate::error::{fail_hint, Exit};
use crate::ui;

fn row(name: String, installed: String, latest: String, update: bool, what: &str) -> Vec<String> {
    vec![
        name,
        installed,
        latest,
        if update { what.into() } else { "up to date".into() },
    ]
}

pub fn run(explicit: Option<&Path>, want_engine: bool, want_model: bool, opts: Opts) -> anyhow::Result<()> {
    let pb = ui::spinner("Refreshing the model catalog…");
    let (catalog, res) = Catalog::refresh();
    pb.finish_and_clear();
    match res {
        Ok(()) => ui::ok(format!(
            "catalog updated ({} models, {})",
            catalog.models.len(),
            catalog.updated
        )),
        Err(e) => ui::warn(format!("could not refresh the catalog ({e}); using the copy on disk")),
    }

    let Ok(p) = Project::discover(explicit) else {
        if want_engine || want_model {
            return fail_hint(
                Exit::InvalidConfig,
                "--engine and --model need a project",
                "cd into your project folder",
            );
        }
        return Ok(());
    };
    paths::ensure_home()?;
    let store = Store::new(&p.root, opts);
    store.ensure_dirs()?;
    let cfg = p.config()?;
    let Some(mut lk) = p.lock() else {
        ui::info("this project has not been built yet; `yourai build` will use the latest versions");
        return Ok(());
    };

    ui::heading("Updates for this project");
    let mut t = ui::table();
    t.set_header(vec!["Component", "Installed", "Latest", ""]);

    let new_engine = catalog.llama().version.clone();
    let mut engine_update = new_engine != lk.engine_version;
    t.add_row(row(
        "Engine (llama.cpp)".into(),
        lk.engine_version.clone(),
        new_engine.clone(),
        engine_update,
        "update available",
    ));
    let mut engine_updates = Vec::new();
    for (name, el) in &lk.engines {
        let Some(spec) = engine::spec(name) else { continue };
        let latest = catalog.engine(name).map(|e| e.version.clone()).unwrap_or_default();
        let update = !latest.is_empty() && latest != el.version;
        t.add_row(row(
            format!("Engine ({})", spec.label),
            el.version.clone(),
            latest,
            update,
            "update available",
        ));
        if update {
            engine_updates.push(spec);
        }
    }
    engine_update |= !engine_updates.is_empty();

    let resolved = models::resolve(&cfg, &catalog, &store.models_dir())?;
    let chat_update = resolved
        .sha256
        .as_deref()
        .is_some_and(|s| !s.eq_ignore_ascii_case(&lk.model_sha256));
    t.add_row(row(
        format!("Model {} {}", cfg.model.id, cfg.model.quant),
        short(&lk.model_sha256),
        resolved.sha256.as_deref().map(short).unwrap_or_else(|| "n/a".into()),
        chat_update,
        "new file available",
    ));
    let mut cap_updates = Vec::new();
    for c in cfg.enabled_caps() {
        let Some(ml) = lk.models.get(c.key()) else { continue };
        let Ok(r) = models::resolve_cap(&cfg, c, &catalog, &store.models_dir()) else {
            continue;
        };
        let update = r.id == ml.id && r.sha256.as_deref().is_some_and(|s| !s.eq_ignore_ascii_case(&ml.sha256));
        t.add_row(row(
            format!("{} model {}", c.label(), r.id),
            short(&ml.sha256),
            r.sha256.as_deref().map(short).unwrap_or_else(|| "n/a".into()),
            update,
            "new file available",
        ));
        if update {
            cap_updates.push((c, r));
        }
    }
    let model_update = chat_update || !cap_updates.is_empty();
    println!("{t}");

    if !want_engine && !want_model {
        if engine_update || model_update {
            println!(
                "{}",
                style("Apply with `yourai update --engine` and/or `yourai update --model`. Nothing else changes.")
                    .dim()
            );
        }
        return Ok(());
    }

    let mut changed = false;
    if want_engine && engine_update {
        let hw = hardware::detect();
        if new_engine != lk.engine_version {
            let (key, exe) = engine::ensure(&store, &hw, &catalog)?;
            lk.engine_version = new_engine;
            lk.engine_key = key.clone();
            lk.engine_exe = store.rel(&exe)?;
            if lk.engines.contains_key("llama-tts") {
                if let Some(tts) = engine::sibling(&exe, "llama-tts") {
                    lk.engines.insert(
                        "llama-tts".into(),
                        EngineLock {
                            version: lk.engine_version.clone(),
                            key,
                            exe: store.rel(&tts)?,
                        },
                    );
                }
            }
        }
        for spec in engine_updates {
            let (key, exe) = engine::ensure_spec(&store, &hw, &catalog, spec)?;
            lk.engines.insert(
                spec.name.to_string(),
                EngineLock {
                    version: catalog.engine(spec.name).map(|e| e.version.clone()).unwrap_or_default(),
                    key,
                    exe: store.rel(&exe)?,
                },
            );
        }
        changed = true;
    }
    if want_model && model_update {
        if chat_update {
            models::accept_license(&resolved, false)?;
            lk.model_sha256 = models::ensure(&store, &resolved)?;
            lk.model_path = store.rel(&resolved.path)?;
            if let Some(f) = resolved.extra("mmproj") {
                lk.mmproj_path = store.rel(&f.path)?;
            }
        }
        for (c, r) in cap_updates {
            models::accept_license(&r, false)?;
            let sha = models::ensure(&store, &r)?;
            update_cap_lock(&store, &mut lk, c, &r, sha)?;
        }
        ui::info(format!(
            "the previous model files were kept under {}; delete unused ones there if you need space",
            p.module_dir().join("models").display()
        ));
        changed = true;
    }
    if !changed {
        ui::ok("already up to date");
        return Ok(());
    }
    lock::save(&p.lock_path(), &lk)?;

    let s = lifecycle::live_state(&p);
    if s.state == "running" {
        ui::info("restarting with the update");
        lifecycle::stop_process(&p, &s);
        p.save_state(&State {
            state: "stopped".into(),
            ..Default::default()
        })?;
        lifecycle::launch(&p, &cfg)?;
    } else {
        ui::ok("updated");
    }
    Ok(())
}

fn update_cap_lock(
    store: &Store,
    lk: &mut lock::Lock,
    c: Capability,
    r: &models::Resolved,
    sha: String,
) -> anyhow::Result<()> {
    if let Some(ml) = lk.models.get_mut(c.key()) {
        ml.sha256 = sha;
        let mut files = std::collections::BTreeMap::new();
        files.insert(r.main_role(), store.rel(&r.path)?);
        for f in &r.extras {
            files.insert(f.role.clone(), store.rel(&f.path)?);
        }
        ml.files = files;
        ml.main = r.main_role();
        ml.defaults = r.defaults.clone();
    }
    Ok(())
}

fn short(s: &str) -> String {
    if s.len() > 12 {
        format!("{}…", &s[..12])
    } else {
        s.to_string()
    }
}

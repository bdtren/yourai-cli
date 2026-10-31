use std::path::Path;

use console::style;

use super::lifecycle;
use crate::core::catalog::Catalog;
use crate::core::project::{Project, State};
use crate::core::{engine, hardware, lock, models};
use crate::error::{fail_hint, Exit};
use crate::ui;

pub fn run(explicit: Option<&Path>, want_engine: bool, want_model: bool) -> anyhow::Result<()> {
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
    let cfg = p.config()?;
    let Some(mut lk) = p.lock() else {
        ui::info("this project has not been built yet; `yourai build` will use the latest versions");
        return Ok(());
    };

    let new_engine = catalog.llama().version.clone();
    let engine_update = new_engine != lk.engine_version;
    let resolved = models::resolve(&cfg, &catalog)?;
    let model_update = resolved
        .sha256
        .as_deref()
        .is_some_and(|s| !s.eq_ignore_ascii_case(&lk.model_sha256));

    ui::heading("Updates for this project");
    let mut t = ui::table();
    t.set_header(vec!["Component", "Installed", "Latest", ""]);
    t.add_row(vec![
        "Engine (llama.cpp)".to_string(),
        lk.engine_version.clone(),
        new_engine.clone(),
        if engine_update {
            "update available".into()
        } else {
            "up to date".into()
        },
    ]);
    t.add_row(vec![
        format!("Model {} {}", cfg.model.id, cfg.model.quant),
        short(&lk.model_sha256),
        resolved.sha256.as_deref().map(short).unwrap_or_else(|| "n/a".into()),
        if model_update {
            "new file available".into()
        } else {
            "up to date".into()
        },
    ]);
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
        let (key, exe) = engine::ensure(&hardware::detect(), &catalog)?;
        lk.engine_version = new_engine;
        lk.engine_key = key;
        lk.engine_exe = exe.display().to_string();
        changed = true;
    }
    if want_model && model_update {
        models::accept_license(&resolved, false)?;
        lk.model_sha256 = models::ensure(&resolved)?;
        lk.model_path = resolved.path.display().to_string();
        ui::info("the previous model file was kept; delete it from ~/.yourai/models if you no longer need it");
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
        if let Some(pid) = s.pid {
            crate::core::process::stop(pid);
        }
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

fn short(s: &str) -> String {
    if s.len() > 12 {
        format!("{}…", &s[..12])
    } else {
        s.to_string()
    }
}

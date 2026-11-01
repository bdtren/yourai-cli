use super::{build, lifecycle};
use crate::core::lock;
use crate::core::project::{Project, State};
use crate::core::store::Opts;
use crate::ui;

pub fn run(p: &Project, accept_license: bool, opts: Opts) -> anyhow::Result<()> {
    p.upgrade_config()?;
    let cfg = p.config()?;
    let current = lock::section_hashes(&cfg, &p.system_prompt(&cfg));
    let state = lifecycle::live_state(p);
    let was_running = state.state == "running";

    let changed = match p.lock() {
        None => vec!["model".to_string(), "runtime".to_string()],
        Some(l) => lock::changed(&l, &current),
    };
    if changed.is_empty() {
        ui::ok("nothing changed since the last build");
        return Ok(());
    }
    ui::info(format!("changed: {}", changed.join(", ")));

    if changed.iter().any(|s| lock::BUILD_SECTIONS.contains(&s.as_str())) {
        ui::info("models or engines changed: checking downloads (files already present are reused)");
        build::prepare(p, &cfg, accept_license, opts)?;
    } else if let Some(mut l) = p.lock() {
        l.sections = current;
        lock::save(&p.lock_path(), &l)?;
    }

    if was_running {
        ui::info("restarting to apply changes");
        if let Some(pid) = state.pid {
            crate::core::process::stop(pid);
        }
        p.save_state(&State {
            state: "stopped".into(),
            ..Default::default()
        })?;
        lifecycle::launch(p, &cfg)?;
    } else {
        ui::ok("changes applied");
        ui::next(if state.state == "paused" {
            "yourai resume"
        } else {
            "yourai start"
        });
    }
    Ok(())
}

pub mod config;
mod dns;
mod local_expose;
mod net;
mod remote_run;
mod ssh;
mod wizard;

use crate::core::project::Project;
use crate::error::{fail_hint, Exit};
use crate::ui;

use config::{DeployConfig, DeployMode};

/// Entry used by `yourai init` after choosing access=remote.
pub fn run_wizard(p: &Project, dry_run: bool) -> anyhow::Result<()> {
    let cfg = wizard::run(p, dry_run)?;
    execute(p, &cfg, dry_run)
}

/// `yourai deploy` / `yourai deploy -e` / `--dry-run`.
pub fn run(p: &Project, edit: bool, dry_run: bool) -> anyhow::Result<()> {
    let existing = DeployConfig::load(p)?;

    if edit {
        if existing.is_some()
            && !ui::confirm(
                "Overwrite existing .yourai/deploy.toml with a new wizard run?",
                false,
                false,
            )?
        {
            ui::info("Left deploy.toml unchanged. Edit it manually or re-run with -e.");
            return Ok(());
        }
        let cfg = wizard::run(p, dry_run)?;
        return execute(p, &cfg, dry_run);
    }

    match existing {
        Some(cfg) if cfg.is_runnable() => {
            if dry_run {
                ui::info(format!(
                    "[dry-run] would run mode {}",
                    cfg.mode.map(|m| m.as_str()).unwrap_or("?")
                ));
            }
            execute(p, &cfg, dry_run)
        }
        Some(cfg) if cfg.incomplete => {
            ui::warn(".yourai/deploy.toml is a stub from non-interactive init.");
            if ui::interactive() {
                let cfg = wizard::run(p, dry_run)?;
                execute(p, &cfg, dry_run)
            } else {
                fail_hint(
                    Exit::InvalidConfig,
                    "deploy.toml is incomplete",
                    "run `yourai deploy -e` in a terminal",
                )
            }
        }
        Some(_) => {
            ui::warn("deploy.toml has no mode.");
            if ui::interactive() {
                let cfg = wizard::run(p, dry_run)?;
                execute(p, &cfg, dry_run)
            } else {
                fail_hint(Exit::InvalidConfig, "deploy.toml has no mode", "run `yourai deploy -e`")
            }
        }
        None => {
            if !ui::interactive() {
                return fail_hint(
                    Exit::InvalidConfig,
                    "no .yourai/deploy.toml yet",
                    "run `yourai deploy -e` in a terminal, or pass answers after creating the file",
                );
            }
            let cfg = wizard::run(p, dry_run)?;
            execute(p, &cfg, dry_run)
        }
    }
}

fn execute(p: &Project, cfg: &DeployConfig, dry_run: bool) -> anyhow::Result<()> {
    match cfg.mode {
        Some(DeployMode::RemoteRun) => remote_run::execute(p, cfg, dry_run),
        Some(DeployMode::LocalExpose) => local_expose::execute(p, cfg, dry_run),
        None => fail_hint(Exit::InvalidConfig, "deploy.toml has no mode", "run `yourai deploy -e`"),
    }
}

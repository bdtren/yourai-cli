use crate::core::project::Project;
use crate::error::{fail_hint, Exit};
use crate::ui;

use super::config::{DeployConfig, DeployMode, ExposeVia, Provider, TunnelKind};
use super::ssh;

/// Interactive deploy wizard. Writes `.yourai/deploy.toml` and optionally runs steps.
pub fn run(p: &Project, dry_run: bool) -> anyhow::Result<DeployConfig> {
    ui::heading("Where should yourai run?");
    let mode_choice = inquire::Select::new(
        "Choose one:",
        vec![DeployMode::RemoteRun.label(), DeployMode::LocalExpose.label()],
    )
    .with_help_message("Cloudflare DNS is an optional later step, not a place to run the model.")
    .prompt()?;

    let mode = if mode_choice == DeployMode::RemoteRun.label() {
        DeployMode::RemoteRun
    } else {
        DeployMode::LocalExpose
    };

    let mut cfg = DeployConfig {
        version: 1,
        mode: Some(mode),
        incomplete: false,
        ..Default::default()
    };

    match mode {
        DeployMode::RemoteRun => fill_remote_run(&mut cfg)?,
        DeployMode::LocalExpose => fill_local_expose(&mut cfg, p)?,
    }

    if ui::confirm(
        "Optional: attach a domain (Cloudflare DNS / HTTPS checklist)?",
        false,
        false,
    )? {
        let domain = inquire::Text::new("Domain (leave empty to skip):")
            .prompt()
            .unwrap_or_default();
        cfg.domain = domain.trim().to_string();
    }

    cfg.save(p)?;
    ui::ok(format!("Saved {}", DeployConfig::path(p).display()));
    ui::info("You can also edit .yourai/deploy.toml by hand, then run `yourai deploy`.");

    if dry_run {
        ui::info("[dry-run] configuration saved; execution steps are listed by the run path");
    }

    Ok(cfg)
}

fn fill_remote_run(cfg: &mut DeployConfig) -> anyhow::Result<()> {
    let provider_label = inquire::Select::new(
        "Provider (you already have a droplet/VPS):",
        vec![Provider::DigitalOcean.label(), Provider::GenericVps.label()],
    )
    .prompt()?;
    cfg.provider = Some(if provider_label == Provider::DigitalOcean.label() {
        Provider::DigitalOcean
    } else {
        Provider::GenericVps
    });

    let remote = inquire::Text::new("SSH remote (user@ip or user@hostname):")
        .with_help_message("Example: root@203.0.113.10")
        .prompt()?;
    if !remote.contains('@') {
        return fail_hint(
            Exit::InvalidConfig,
            "remote must look like user@host",
            "example: root@203.0.113.10",
        );
    }
    cfg.remote = remote.trim().to_string();

    let default_id = ssh::default_identity_path();
    let id = inquire::Text::new("SSH identity file:")
        .with_default(&default_id.display().to_string())
        .prompt()?;
    cfg.ssh_identity_file = id.trim().to_string();

    let port = inquire::Text::new("Remote listen port:")
        .with_default("8080")
        .prompt()?;
    cfg.remote_port = port.parse().unwrap_or(8080);
    Ok(())
}

fn fill_local_expose(cfg: &mut DeployConfig, p: &Project) -> anyhow::Result<()> {
    let port = p.config()?.server.port;
    ui::info(format!(
        "Will bind this project to 0.0.0.0:{port} and require an API key."
    ));

    let via = inquire::Select::new(
        "How should remote clients reach this PC?",
        vec![
            "Public/LAN IP (firewall / port-forward)",
            "Tunnel (cloudflared or Tailscale Funnel)",
        ],
    )
    .prompt()?;
    if via.starts_with("Tunnel") {
        cfg.expose_via = Some(ExposeVia::Tunnel);
        let tun =
            inquire::Select::new("Tunnel tool:", vec!["cloudflared (recommended)", "Tailscale Funnel"]).prompt()?;
        cfg.tunnel = Some(if tun.starts_with("cloudflared") {
            TunnelKind::Cloudflared
        } else {
            TunnelKind::Tailscale
        });
    } else {
        cfg.expose_via = Some(ExposeVia::Ip);
    }
    Ok(())
}

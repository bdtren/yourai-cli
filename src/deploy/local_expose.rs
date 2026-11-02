use crate::commands::config as config_cmd;
use crate::core::project::Project;
use crate::core::secrets;
use crate::ui;

use super::config::{write_readme, DeployConfig, ExposeVia, TunnelKind};
use super::dns::{print_cloudflare_checklist, readme_dns_section};
use super::net;
use super::ssh;

pub fn execute(p: &Project, cfg: &DeployConfig, dry_run: bool) -> anyhow::Result<()> {
    ui::heading("Local expose (this PC + remote IP access)");
    let server = p.config()?.server;
    let port = server.port;

    ensure_lan_binding(p, dry_run)?;
    let api_key = ensure_api_key(p, dry_run)?;

    let lan = net::lan_ipv4s();
    ui::heading("Addresses");
    if lan.is_empty() {
        ui::warn("Could not detect a LAN IP automatically.");
    } else {
        for ip in &lan {
            ui::ok(format!("LAN:  http://{ip}:{port}"));
        }
    }
    let public = net::public_ipv4();
    match public {
        Some(ip) => {
            ui::ok(format!("Public IP (if reachable): http://{ip}:{port}"));
            ui::warn("Home routers usually need a port-forward for internet clients to reach this PC.");
        }
        None => ui::warn("Could not detect a public IP (offline, or carrier-grade NAT)."),
    }

    ui::heading("Firewall");
    for h in net::firewall_hints(port) {
        ui::info(h);
    }

    let expose_via = cfg.expose_via.unwrap_or(ExposeVia::Ip);
    let mut tunnel_host = String::new();
    let use_tunnel = matches!(expose_via, ExposeVia::Tunnel)
        || (!dry_run
            && public.is_none()
            && ui::confirm(
                "No usable public IP detected (or blocked). Set up a tunnel (cloudflared / Tailscale)?",
                true,
                false,
            )?);

    if use_tunnel {
        let kind = cfg.tunnel.unwrap_or(TunnelKind::Cloudflared);
        tunnel_host = guide_tunnel(kind, port, dry_run)?;
    } else if !dry_run && ui::confirm("Offer tunnel setup anyway (cloudflared preferred)?", false, false)? {
        tunnel_host = guide_tunnel(cfg.tunnel.unwrap_or(TunnelKind::Cloudflared), port, dry_run)?;
    }

    let dns_target = if !tunnel_host.is_empty() {
        tunnel_host.clone()
    } else if let Some(ip) = public {
        ip.to_string()
    } else if let Some(ip) = lan.first() {
        ip.to_string()
    } else {
        "YOUR_IP".into()
    };

    if !cfg.domain.is_empty() {
        print_cloudflare_checklist(&cfg.domain, &dns_target, port);
    } else if !dry_run
        && ui::confirm(
            "Optional: print Cloudflare DNS / HTTPS checklist for a domain?",
            false,
            false,
        )?
    {
        let domain = inquire::Text::new("Domain:").prompt().unwrap_or_default();
        if !domain.trim().is_empty() {
            print_cloudflare_checklist(domain.trim(), &dns_target, port);
        }
    }

    let lan_lines: String = lan
        .iter()
        .map(|ip| format!("- LAN URL: `http://{ip}:{port}`\n"))
        .collect();
    let readme = format!(
        "# Deploy: local_expose\n\n\
         yourai stays on this PC. `server.host` is `0.0.0.0` and an API key is required.\n\n\
         {lan_lines}\
         - Public IP URL: {}\n\
         - Tunnel: {}\n\
         - API key: `.yourai/secrets.toml` ({})\n\n\
         ## Firewall / router\n\n\
         {}\n\n\
         ## Re-run\n\n\
         ```sh\nyourai deploy\nyourai deploy -e\n```\n\n\
         {}\
         ",
        public
            .map(|ip| format!("`http://{ip}:{port}`"))
            .unwrap_or_else(|| "_not detected_".into()),
        if tunnel_host.is_empty() {
            "_not configured_".into()
        } else {
            format!("`{tunnel_host}`")
        },
        secrets::redact(&api_key),
        net::firewall_hints(port).join("\n\n"),
        readme_dns_section(if cfg.domain.is_empty() { "" } else { &cfg.domain }, &dns_target, port),
    );
    write_readme(p, &readme)?;
    ui::info(format!("Wrote {}", DeployConfig::readme_path(p).display()));
    ui::next("yourai build && yourai start   # then open a LAN/public URL with the API key");
    Ok(())
}

fn ensure_lan_binding(p: &Project, dry_run: bool) -> anyhow::Result<()> {
    let cfg = p.config()?;
    if cfg.server.host == "0.0.0.0" && cfg.server.require_api_key {
        ui::ok("server.host is already 0.0.0.0 with require_api_key");
        return Ok(());
    }
    if dry_run {
        ui::info("[dry-run] would set server.require_api_key=true and server.host=0.0.0.0");
        return Ok(());
    }
    config_cmd::set_value(p, "server.require_api_key", "true")?;
    config_cmd::set_value(p, "server.host", "0.0.0.0")?;
    ui::ok("Bound server to 0.0.0.0 and required an API key");
    Ok(())
}

fn ensure_api_key(p: &Project, dry_run: bool) -> anyhow::Result<String> {
    if let Some(k) = secrets::api_key(&p.root) {
        ui::info(format!("Using existing API key {}", secrets::redact(&k)));
        return Ok(k);
    }
    let key = secrets::generate_key();
    if dry_run {
        ui::info("[dry-run] would generate an API key in .yourai/secrets.toml");
        return Ok(key);
    }
    secrets::write_key(&p.root, &key)?;
    ui::ok(format!(
        "API key saved ({}) — send as Authorization: Bearer <key>",
        secrets::redact(&key)
    ));
    Ok(key)
}

fn guide_tunnel(kind: TunnelKind, port: u16, dry_run: bool) -> anyhow::Result<String> {
    match kind {
        TunnelKind::Cloudflared => {
            ui::heading("Tunnel: cloudflared");
            ui::info("Install: https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/");
            ui::info("Then run: cloudflared tunnel --url http://127.0.0.1:{port}");
            ui::info("Or create a named tunnel and route a hostname in Zero Trust.");
            if !dry_run && which("cloudflared") {
                ui::ok("cloudflared is on PATH");
                if ui::confirm("Open Cloudflare Zero Trust docs in a browser?", false, false)? {
                    ssh::try_open_url("https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/");
                }
            } else if !dry_run {
                ui::warn("cloudflared not found on PATH — install it, then re-run the tunnel command.");
            }
            Ok(format!("cloudflared→127.0.0.1:{port}"))
        }
        TunnelKind::Tailscale => {
            ui::heading("Tunnel: Tailscale Funnel");
            ui::info("Install Tailscale, then: tailscale funnel {port}");
            ui::info("Docs: https://tailscale.com/kb/1223/funnel");
            if !dry_run && ui::confirm("Open Tailscale Funnel docs?", false, false)? {
                ssh::try_open_url("https://tailscale.com/kb/1223/funnel");
            }
            Ok(format!("tailscale-funnel:{port}"))
        }
    }
}

fn which(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

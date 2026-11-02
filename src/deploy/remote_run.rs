use crate::core::project::Project;
use crate::core::secrets;
use crate::ui;

use super::config::{write_readme, DeployConfig, Provider};
use super::dns::{print_cloudflare_checklist, readme_dns_section};
use super::ssh;

pub fn execute(p: &Project, cfg: &DeployConfig, dry_run: bool) -> anyhow::Result<()> {
    let remote = cfg.remote.trim();
    if remote.is_empty() || !remote.contains('@') {
        anyhow::bail!("deploy.toml remote must look like user@host (run `yourai deploy -e`)");
    }
    let provider = cfg.provider.unwrap_or(Provider::GenericVps);
    let identity = if cfg.ssh_identity_file.is_empty() {
        ssh::default_identity_path()
    } else {
        ssh::expand_tilde(&cfg.ssh_identity_file)
    };
    let port = cfg.remote_port;
    let host_ip = remote.split('@').nth(1).unwrap_or(remote);

    ui::heading("Remote run (SSH)");
    ui::info(format!("Provider: {}", provider.label()));
    ui::info(format!("Remote: {remote}"));
    ui::info(format!("Identity: {}", identity.display()));

    ssh::detect_or_create_key(&identity, dry_run)?;
    let pub_path = std::path::PathBuf::from(format!("{}.pub", identity.display()));
    if pub_path.is_file() {
        let pubkey = ssh::read_pubkey(&identity)?;
        ssh::print_key_instructions(provider, &pubkey);
        if !dry_run && ui::confirm("Open the provider security page in a browser?", true, false)? {
            ssh::try_open_url(provider.ssh_keys_url());
        }
    } else if dry_run {
        ui::info("[dry-run] would print the SSH public key and provider security URL");
    }

    ssh::wait_until_connected(remote, &identity, dry_run)?;

    let remote_home = format!(
        "~/{}",
        p.root.file_name().and_then(|s| s.to_str()).unwrap_or("yourai-project")
    );
    ui::info("Ensuring yourai is installed on the remote host…");
    let install_cmd = r#"
set -e
if ! command -v yourai >/dev/null 2>&1; then
  curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash
  export PATH="$HOME/.yourai/bin:$PATH"
fi
command -v yourai
"#;
    ssh::run_remote(
        remote,
        &identity,
        &format!("bash -lc {}", shell_quote(install_cmd.trim())),
        dry_run,
    )?;

    ui::info("Syncing project files (models under yourai_modules/ are skipped)…");
    ssh::run_remote(
        remote,
        &identity,
        &format!("mkdir -p {}", shell_quote(&remote_home)),
        dry_run,
    )?;
    ssh::rsync_project(&p.root, remote, &identity, &remote_home, dry_run)?;

    let api_key = secrets::api_key(&p.root).unwrap_or_else(secrets::generate_key);
    if secrets::api_key(&p.root).is_none() && !dry_run {
        secrets::write_key(&p.root, &api_key)?;
    }

    let setup = format!(
        r#"
set -e
export PATH="$HOME/.yourai/bin:$PATH"
cd {home}
yourai config set server.require_api_key true
yourai config set server.host 0.0.0.0
yourai config set server.port {port}
mkdir -p .yourai
cat > .yourai/secrets.toml <<'EOF'
# Keep this file private.
api_key = "{key}"
EOF
chmod 600 .yourai/secrets.toml 2>/dev/null || true
yourai build --accept-license || true
yourai stop >/dev/null 2>&1 || true
yourai start
"#,
        home = remote_home,
        port = port,
        key = api_key.replace('"', ""),
    );
    ui::info("Configuring, building, and starting yourai on the remote…");
    ssh::run_remote(
        remote,
        &identity,
        &format!("bash -lc {}", shell_quote(setup.trim())),
        dry_run,
    )?;

    ui::heading("Open access");
    ui::info(format!(
        "Allow TCP {port} on the cloud firewall / security group for this droplet/VPS."
    ));
    if matches!(provider, Provider::DigitalOcean) {
        ui::info("DigitalOcean: Networking → Firewalls → inbound TCP custom port.");
    }
    let url = format!("http://{host_ip}:{port}");
    ui::ok(format!("Access URL: {url}"));
    ui::info(format!(
        "API key (Authorization: Bearer …): {}",
        secrets::redact(&api_key)
    ));

    let mut target_for_dns = host_ip.to_string();
    if !cfg.domain.is_empty() {
        print_cloudflare_checklist(&cfg.domain, host_ip, port);
        target_for_dns = host_ip.to_string();
    } else if !dry_run
        && ui::confirm(
            "Optional: set up a domain with Cloudflare DNS / HTTPS notes?",
            false,
            false,
        )?
    {
        let domain = inquire::Text::new("Domain (example.com or ai.example.com):")
            .prompt()
            .unwrap_or_default();
        if !domain.trim().is_empty() {
            print_cloudflare_checklist(domain.trim(), host_ip, port);
        }
    }

    let readme = format!(
        "# Deploy: remote_run\n\n\
         - Provider: `{}`\n\
         - Remote: `{remote}`\n\
         - SSH identity: `{}`\n\
         - Port: `{port}`\n\
         - URL: `{url}`\n\
         - API key file on remote: `.yourai/secrets.toml`\n\n\
         ## Firewall\n\n\
         Open TCP `{port}` on the provider firewall / security group.\n\n\
         ## Re-run\n\n\
         ```sh\nyourai deploy\nyourai deploy -e   # change settings\n```\n\n\
         {}\
         ",
        provider.as_str(),
        identity.display(),
        readme_dns_section(
            if cfg.domain.is_empty() { "" } else { &cfg.domain },
            &target_for_dns,
            port
        ),
    );
    write_readme(p, &readme)?;
    ui::info(format!("Wrote {}", DeployConfig::readme_path(p).display()));
    ui::next(format!("Open {url} and send the API key from .yourai/secrets.toml"));
    Ok(())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

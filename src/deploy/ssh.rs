use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

use super::config::Provider;

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

pub fn default_identity_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh")
        .join("id_ed25519_yourai")
}

pub fn detect_or_create_key(identity: &Path, dry_run: bool) -> anyhow::Result<PathBuf> {
    let pub_path = PathBuf::from(format!("{}.pub", identity.display()));
    if identity.is_file() && pub_path.is_file() {
        ui::ok(format!("Using existing SSH key {}", identity.display()));
        return Ok(identity.to_path_buf());
    }
    if dry_run {
        ui::info(format!(
            "[dry-run] would generate ed25519 key at {}",
            identity.display()
        ));
        return Ok(identity.to_path_buf());
    }
    if let Some(parent) = identity.parent() {
        std::fs::create_dir_all(parent)?;
    }
    ui::info(format!("Generating SSH key at {} …", identity.display()));
    let status = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            &identity.display().to_string(),
            "-N",
            "",
            "-C",
            "yourai-deploy",
        ])
        .status()
        .map_err(|e| {
            CliError::new(Exit::Unsupported, format!("ssh-keygen failed to start: {e}"))
                .hint("install OpenSSH client tools")
        })?;
    if !status.success() {
        return fail_hint(
            Exit::Runtime,
            "ssh-keygen did not succeed",
            "create a key manually, then pass its path in the wizard",
        );
    }
    ui::ok(format!("Created {}", identity.display()));
    Ok(identity.to_path_buf())
}

pub fn read_pubkey(identity: &Path) -> anyhow::Result<String> {
    let pub_path = PathBuf::from(format!("{}.pub", identity.display()));
    let text = std::fs::read_to_string(&pub_path).map_err(|_| {
        CliError::new(Exit::General, format!("missing public key {}", pub_path.display()))
            .hint("run ssh-keygen or pick another identity file")
    })?;
    Ok(text.trim().to_string())
}

pub fn print_key_instructions(provider: Provider, pubkey: &str) {
    ui::heading("Add this SSH public key to your server");
    println!("{pubkey}\n");
    ui::info(format!("Provider panel: {}", provider.ssh_keys_url()));
    match provider {
        Provider::DigitalOcean => {
            ui::info("DigitalOcean → Settings → Security → SSH keys → Add SSH Key, then attach it to the droplet (or add to ~/.ssh/authorized_keys on the droplet).");
        }
        Provider::GenericVps => {
            ui::info("Paste the key into the VPS console / cloud panel, or append it to ~/.ssh/authorized_keys for the login user.");
        }
    }
}

/// Probe SSH with BatchMode (no password prompts).
pub fn can_connect(remote: &str, identity: &Path) -> bool {
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-i",
        &identity.display().to_string(),
        remote,
        "true",
    ]);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn wait_until_connected(remote: &str, identity: &Path, dry_run: bool) -> anyhow::Result<()> {
    if dry_run {
        ui::info(format!("[dry-run] would wait for SSH to {remote}"));
        return Ok(());
    }
    loop {
        if can_connect(remote, identity) {
            ui::ok(format!("SSH connected to {remote}"));
            return Ok(());
        }
        ui::warn(format!("Cannot SSH to {remote} yet (BatchMode)."));
        let again = ui::confirm("Press Enter after the key is on the server — try again?", true, false)?;
        if !again {
            return fail_hint(
                Exit::Runtime,
                format!("SSH to {remote} is not ready"),
                "add the public key on the server, then re-run `yourai deploy`",
            );
        }
    }
}

pub fn run_remote(remote: &str, identity: &Path, remote_cmd: &str, dry_run: bool) -> anyhow::Result<()> {
    if dry_run {
        ui::info(format!("[dry-run] ssh {remote} -- {remote_cmd}"));
        return Ok(());
    }
    let status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=20",
            "-i",
            &identity.display().to_string(),
            remote,
            remote_cmd,
        ])
        .status()
        .map_err(|e| CliError::new(Exit::Runtime, format!("ssh failed to start: {e}")))?;
    if !status.success() {
        return fail_hint(
            Exit::Runtime,
            format!("remote command failed on {remote}"),
            "check SSH access and that the remote user can install packages / run yourai",
        );
    }
    Ok(())
}

pub fn rsync_project(
    local_root: &Path,
    remote: &str,
    identity: &Path,
    remote_dir: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    let dest = format!("{remote}:{remote_dir}/");
    if dry_run {
        ui::info(format!("[dry-run] rsync {} → {dest}", local_root.display()));
        return Ok(());
    }
    // Prefer rsync; fall back to scp -r if missing.
    let rsync = Command::new("rsync")
        .args([
            "-az",
            "--delete",
            "--exclude",
            "yourai_modules/",
            "--exclude",
            ".yourai/state.json",
            "--exclude",
            ".yourai/server.log",
            "--exclude",
            "outputs/",
            "-e",
            &format!("ssh -i {} -o BatchMode=yes", identity.display()),
            &format!("{}/", local_root.display()),
            &dest,
        ])
        .status();
    match rsync {
        Ok(s) if s.success() => {
            ui::ok(format!("Synced project to {dest}"));
            Ok(())
        }
        Ok(_) | Err(_) => {
            ui::warn("rsync failed or missing; trying scp -r …");
            let status = Command::new("scp")
                .args([
                    "-i",
                    &identity.display().to_string(),
                    "-o",
                    "BatchMode=yes",
                    "-r",
                    &local_root.display().to_string(),
                    &format!(
                        "{remote}:{}",
                        Path::new(remote_dir).parent().unwrap_or(Path::new(".")).display()
                    ),
                ])
                .status()
                .map_err(|e| CliError::new(Exit::Runtime, format!("scp failed: {e}")))?;
            if !status.success() {
                return fail_hint(
                    Exit::Runtime,
                    "could not copy the project to the remote host",
                    "install rsync on this machine, or copy the folder manually then re-run",
                );
            }
            ui::ok("Copied project with scp");
            Ok(())
        }
    }
}

pub fn try_open_url(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", url]).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };
}

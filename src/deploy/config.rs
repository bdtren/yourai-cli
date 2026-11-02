use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::project::Project;
use crate::error::{CliError, Exit};

pub const DEPLOY_FILE: &str = "deploy.toml";
pub const DEPLOY_README: &str = "deploy-README.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeployMode {
    RemoteRun,
    LocalExpose,
}

impl DeployMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoteRun => "remote_run",
            Self::LocalExpose => "local_expose",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::RemoteRun => "Build and run yourai on a remote server",
            Self::LocalExpose => "Self-host on this PC and grant remote access via an IP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    DigitalOcean,
    #[default]
    GenericVps,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DigitalOcean => "digitalocean",
            Self::GenericVps => "generic_vps",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DigitalOcean => "DigitalOcean droplet",
            Self::GenericVps => "Generic VPS / other cloud",
        }
    }

    pub fn ssh_keys_url(self) -> &'static str {
        match self {
            Self::DigitalOcean => "https://cloud.digitalocean.com/account/security",
            Self::GenericVps => "https://your-provider-console (add the public key to the VPS)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExposeVia {
    #[default]
    Ip,
    Tunnel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TunnelKind {
    #[default]
    Cloudflared,
    Tailscale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeployConfig {
    pub version: u32,
    /// Absent or empty in stubs from `init --yes --access remote`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DeployMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ssh_identity_file: String,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expose_via: Option<ExposeVia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<TunnelKind>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub domain: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub healthcheck_url: String,
    /// True when written as a non-interactive stub that still needs `yourai deploy -e`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub incomplete: bool,
}

fn default_remote_port() -> u16 {
    8080
}

impl Default for DeployConfig {
    fn default() -> Self {
        Self {
            version: 1,
            mode: None,
            provider: None,
            remote: String::new(),
            ssh_identity_file: String::new(),
            remote_port: 8080,
            expose_via: None,
            tunnel: None,
            domain: String::new(),
            healthcheck_url: String::new(),
            incomplete: false,
        }
    }
}

impl DeployConfig {
    pub fn path(p: &Project) -> PathBuf {
        p.state_dir().join(DEPLOY_FILE)
    }

    pub fn readme_path(p: &Project) -> PathBuf {
        p.state_dir().join(DEPLOY_README)
    }

    pub fn load(p: &Project) -> anyhow::Result<Option<Self>> {
        let path = Self::path(p);
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let cfg: DeployConfig = toml::from_str(&text).map_err(|e| {
            CliError::new(Exit::InvalidConfig, format!("invalid {}: {e}", path.display()))
                .hint("fix the file or run `yourai deploy -e`")
        })?;
        if cfg.version != 1 {
            return Err(CliError::new(
                Exit::InvalidConfig,
                format!("{} has unsupported version {}", path.display(), cfg.version),
            )
            .hint("run `yourai deploy -e` to recreate it")
            .into());
        }
        Ok(Some(cfg))
    }

    pub fn save(&self, p: &Project) -> anyhow::Result<()> {
        std::fs::create_dir_all(p.state_dir())?;
        let text = toml::to_string_pretty(self)
            .map_err(|e| CliError::new(Exit::General, format!("could not serialize deploy.toml: {e}")))?;
        let header = "# yourai deploy settings. Edit carefully, or run `yourai deploy -e`.\n\
# mode = remote_run | local_expose\n\n";
        std::fs::write(Self::path(p), format!("{header}{text}"))?;
        Ok(())
    }

    pub fn is_runnable(&self) -> bool {
        !self.incomplete && self.mode.is_some()
    }
}

/// Minimal stub so CI/`--yes` init does not hang; user finishes with `yourai deploy -e`.
pub fn write_stub(p: &Project) -> anyhow::Result<()> {
    let cfg = DeployConfig {
        version: 1,
        incomplete: true,
        ..Default::default()
    };
    cfg.save(p)?;
    let readme = "# Deploy stub\n\n\
This project was created with `yourai init --access remote --yes`.\n\n\
Finish setup interactively:\n\n```sh\nyourai deploy -e\n```\n\n\
Modes:\n\
- **remote_run** — install and run yourai on an existing VPS over SSH\n\
- **local_expose** — keep yourai on this PC, bind `0.0.0.0`, share an IP (optional tunnel/DNS)\n";
    std::fs::write(DeployConfig::readme_path(p), readme)?;
    Ok(())
}

pub fn write_readme(p: &Project, body: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(p.state_dir())?;
    std::fs::write(DeployConfig::readme_path(p), body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{config, project};

    fn proj() -> (tempfile::TempDir, Project) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        project::scaffold(&dir, &config::sample(), None).unwrap();
        (tmp, Project { root: dir })
    }

    #[test]
    fn round_trip_remote_run() {
        let (_t, p) = proj();
        let cfg = DeployConfig {
            version: 1,
            mode: Some(DeployMode::RemoteRun),
            provider: Some(Provider::DigitalOcean),
            remote: "root@203.0.113.10".into(),
            ssh_identity_file: "~/.ssh/id_ed25519_yourai".into(),
            remote_port: 8080,
            domain: "ai.example.com".into(),
            ..Default::default()
        };
        cfg.save(&p).unwrap();
        let loaded = DeployConfig::load(&p).unwrap().unwrap();
        assert_eq!(loaded.mode, Some(DeployMode::RemoteRun));
        assert_eq!(loaded.remote, "root@203.0.113.10");
        assert!(loaded.is_runnable());
    }

    #[test]
    fn stub_is_incomplete() {
        let (_t, p) = proj();
        write_stub(&p).unwrap();
        let loaded = DeployConfig::load(&p).unwrap().unwrap();
        assert!(loaded.incomplete);
        assert!(!loaded.is_runnable());
        assert!(DeployConfig::readme_path(&p).is_file());
    }
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::catalog::Catalog;
use super::config::Config;
use super::{downloader, paths};
use crate::error::{CliError, Exit};
use crate::ui;

/// Everything needed to download one model file.
pub struct Resolved {
    pub id: String,
    pub name: String,
    pub url: String,
    pub path: PathBuf,
    pub sha256: Option<String>,
    pub size_gb: f64,
    pub license: String,
    pub license_url: String,
}

pub fn resolve(cfg: &Config, catalog: &Catalog) -> anyhow::Result<Resolved> {
    if cfg.model.id == "custom" {
        let safe_repo = cfg.model.repo.replace('/', "__");
        if safe_repo.contains("..") || cfg.model.file.contains('/') || cfg.model.file.contains('\\') {
            anyhow::bail!("model.repo / model.file contain unsafe characters");
        }
        return Ok(Resolved {
            id: "custom".into(),
            name: format!("{}/{}", cfg.model.repo, cfg.model.file),
            url: format!(
                "https://huggingface.co/{}/resolve/main/{}",
                cfg.model.repo, cfg.model.file
            ),
            path: paths::models().join("custom").join(safe_repo).join(&cfg.model.file),
            sha256: None,
            size_gb: 0.0,
            license: "see the model page".into(),
            license_url: format!("https://huggingface.co/{}", cfg.model.repo),
        });
    }
    let m = catalog.model(&cfg.model.id).ok_or_else(|| {
        CliError::new(
            Exit::InvalidConfig,
            format!("model `{}` is not in the catalog", cfg.model.id),
        )
        .hint(
            "run `yourai update` to refresh the catalog, or pick another model with `yourai config set model.id <id>`",
        )
    })?;
    let q = m.quants.get(&cfg.model.quant).ok_or_else(|| {
        let have: Vec<&str> = m.quants.keys().map(String::as_str).collect();
        CliError::new(
            Exit::InvalidConfig,
            format!(
                "model `{}` has no {} file (available: {})",
                m.id,
                cfg.model.quant,
                have.join(", ")
            ),
        )
    })?;
    Ok(Resolved {
        id: m.id.clone(),
        name: m.name.clone(),
        url: m.download_url(&cfg.model.quant).unwrap(),
        path: paths::models().join(&m.id).join(&q.file),
        sha256: Some(q.sha256.clone()),
        size_gb: q.size_gb,
        license: m.license.clone(),
        license_url: m.license_url.clone(),
    })
}

fn accepted() -> BTreeMap<String, String> {
    std::fs::read_to_string(paths::licenses())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Shows the license and records acceptance once per model.
pub fn accept_license(r: &Resolved, assume: bool) -> anyhow::Result<()> {
    let mut all = accepted();
    if all.get(&r.id).is_some_and(|l| *l == r.license) {
        return Ok(());
    }
    ui::heading("Model license");
    println!("{} is distributed under: {}\n  {}", r.name, r.license, r.license_url);
    if !assume {
        if !ui::interactive() {
            return Err(
                CliError::new(Exit::Cancelled, "the model license has not been accepted")
                    .hint("read the license, then run again with --accept-license")
                    .into(),
            );
        }
        if !inquire::Confirm::new("Do you accept this license?")
            .with_default(true)
            .prompt()?
        {
            return Err(CliError::new(Exit::Cancelled, "license not accepted, nothing was downloaded").into());
        }
    }
    all.insert(r.id.clone(), r.license.clone());
    paths::ensure_home()?;
    std::fs::write(paths::licenses(), serde_json::to_string_pretty(&all)?)?;
    Ok(())
}

/// Downloads the model if needed. Returns its SHA-256.
pub fn ensure(r: &Resolved) -> anyhow::Result<String> {
    if r.sha256.is_none() {
        ui::warn("custom model: there is no published checksum, so the file's hash is recorded on first download");
        if r.path.is_file() {
            if let Ok(sha) = std::fs::read_to_string(format!("{}.sha256", r.path.display())) {
                return Ok(sha.trim().to_string());
            }
        }
    }
    downloader::download(&downloader::Request {
        url: &r.url,
        dest: &r.path,
        sha256: r.sha256.as_deref(),
        size_bytes: (r.size_gb * 1e9) as u64,
        label: &format!("model {}", r.name),
    })
}

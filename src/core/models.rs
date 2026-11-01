use std::collections::BTreeMap;
use std::path::PathBuf;

use super::catalog::{Catalog, FileRef, Model};
use super::config::{Capability, Config};
use super::{downloader, paths};
use crate::error::{CliError, Exit};
use crate::ui;

/// One extra file a model needs next to its weights (projector, vae, text encoder).
pub struct ModelFile {
    /// `mmproj`, or the engine argument for a component (`vae`, `t5xxl`, ...).
    pub role: String,
    pub url: String,
    pub path: PathBuf,
    pub sha256: Option<String>,
    pub size_gb: f64,
}

/// Everything needed to download one model.
pub struct Resolved {
    pub id: String,
    pub name: String,
    pub url: String,
    pub path: PathBuf,
    pub sha256: Option<String>,
    pub size_gb: f64,
    pub license: String,
    pub license_url: String,
    pub extras: Vec<ModelFile>,
    /// Engine defaults from the catalog (image and video models).
    pub defaults: serde_json::Value,
}

impl Resolved {
    pub fn download_gb(&self) -> f64 {
        self.size_gb + self.extras.iter().map(|f| f.size_gb).sum::<f64>()
    }

    pub fn extra(&self, role: &str) -> Option<&ModelFile> {
        self.extras.iter().find(|f| f.role == role)
    }

    /// The engine argument the main weights go to (`model` or `diffusion_model`).
    pub fn main_role(&self) -> String {
        self.defaults
            .get("main")
            .and_then(|v| v.as_str())
            .unwrap_or("model")
            .to_string()
    }

    /// Role -> path for every file, including the main weights.
    pub fn files(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert(self.main_role(), self.path.display().to_string());
        for f in &self.extras {
            m.insert(f.role.clone(), f.path.display().to_string());
        }
        m
    }
}

fn not_in_catalog(id: &str, key: &str) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, format!("model `{id}` is not in the catalog"))
        .hint(format!(
            "run `yourai update` to refresh the catalog, or pick another model with `yourai config set {key} <id>`"
        ))
        .into()
}

fn file_name(f: &str) -> &str {
    f.rsplit('/').next().unwrap_or(f)
}

fn extra_file(m: &Model, role: &str, f: &FileRef) -> ModelFile {
    ModelFile {
        role: role.to_string(),
        url: m.file_url(f),
        path: paths::models().join(&m.id).join(file_name(&f.file)),
        sha256: Some(f.sha256.clone()),
        size_gb: f.size_gb,
    }
}

fn from_catalog(m: &Model, quant: &str, key: &str, mmproj: bool) -> anyhow::Result<Resolved> {
    let q = m.quants.get(quant).ok_or_else(|| {
        let have: Vec<&str> = m.quants.keys().map(String::as_str).collect();
        CliError::new(
            Exit::InvalidConfig,
            format!("model `{}` has no {quant} file (available: {})", m.id, have.join(", ")),
        )
        .hint(format!("run `yourai config set {key} {}`", m.default_quant()))
    })?;
    let mut extras = Vec::new();
    if mmproj {
        if let Some(f) = &m.mmproj {
            extras.push(extra_file(m, "mmproj", f));
        }
    }
    for c in &m.components {
        extras.push(extra_file(m, &c.role, c));
    }
    Ok(Resolved {
        id: m.id.clone(),
        name: m.name.clone(),
        url: m.download_url(quant).unwrap(),
        path: paths::models().join(&m.id).join(file_name(&q.file)),
        sha256: Some(q.sha256.clone()),
        size_gb: q.size_gb,
        license: m.license.clone(),
        license_url: m.license_url.clone(),
        extras,
        defaults: m.defaults.clone(),
    })
}

fn safe_file(f: &str) -> bool {
    !f.contains('/') && !f.contains('\\') && !f.contains("..")
}

/// The always-on chat model, with its projector when `model.mmproj` asks for one.
pub fn resolve(cfg: &Config, catalog: &Catalog) -> anyhow::Result<Resolved> {
    if cfg.model.id == "custom" {
        let safe_repo = cfg.model.repo.replace('/', "__");
        if safe_repo.contains("..") || !safe_file(&cfg.model.file) {
            anyhow::bail!("model.repo / model.file contain unsafe characters");
        }
        let dir = paths::models().join("custom").join(&safe_repo);
        let hf = |f: &str| format!("https://huggingface.co/{}/resolve/main/{f}", cfg.model.repo);
        let mut extras = Vec::new();
        let mm = cfg.model.mmproj.as_str();
        if !mm.is_empty() && mm != "auto" {
            if !safe_file(mm) {
                anyhow::bail!("model.mmproj contains unsafe characters");
            }
            extras.push(ModelFile {
                role: "mmproj".into(),
                url: hf(mm),
                path: dir.join(mm),
                sha256: None,
                size_gb: 0.0,
            });
        }
        return Ok(Resolved {
            id: "custom".into(),
            name: format!("{}/{}", cfg.model.repo, cfg.model.file),
            url: hf(&cfg.model.file),
            path: dir.join(&cfg.model.file),
            sha256: None,
            size_gb: 0.0,
            license: "see the model page".into(),
            license_url: format!("https://huggingface.co/{}", cfg.model.repo),
            extras,
            defaults: serde_json::Value::Null,
        });
    }
    let m = catalog
        .model(&cfg.model.id)
        .ok_or_else(|| not_in_catalog(&cfg.model.id, "model.id"))?;
    if !m.kind.is_chat() {
        return Err(
            CliError::new(Exit::InvalidConfig, format!("model `{}` is not a chat model", m.id))
                .hint(
                    "choose a text, vision, or omni model for [model]; image and speech models go in their own tables",
                )
                .into(),
        );
    }
    let mut r = from_catalog(m, &cfg.model.quant, "model.quant", cfg.model.mmproj == "auto")?;
    let mm = cfg.model.mmproj.as_str();
    if !mm.is_empty() && mm != "auto" {
        if !safe_file(mm) {
            anyhow::bail!("model.mmproj contains unsafe characters");
        }
        r.extras.push(ModelFile {
            role: "mmproj".into(),
            url: format!("https://huggingface.co/{}/resolve/{}/{mm}", m.repo, m.revision),
            path: paths::models().join(&m.id).join(mm),
            sha256: None,
            size_gb: 0.0,
        });
    }
    Ok(r)
}

/// The model for an enabled capability.
pub fn resolve_cap(cfg: &Config, cap: Capability, catalog: &Catalog) -> anyhow::Result<Resolved> {
    let c = cfg.cap(cap);
    let key = cap.key();
    let m = catalog
        .model(c.model)
        .ok_or_else(|| not_in_catalog(c.model, &format!("{key}.model")))?;
    if m.kind != cap.kind() {
        let ids: Vec<&str> = catalog.models_of(cap.kind()).map(|m| m.id.as_str()).collect();
        return Err(CliError::new(
            Exit::InvalidConfig,
            format!("model `{}` cannot be used for {}", m.id, cap.label().to_lowercase()),
        )
        .hint(format!("choose one of: {}", ids.join(", ")))
        .into());
    }
    let quant = if c.quant.is_empty() { m.default_quant() } else { c.quant };
    from_catalog(m, quant, &format!("{key}.quant"), true)
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

fn ensure_file(
    url: &str,
    path: &std::path::Path,
    sha: Option<&str>,
    size_gb: f64,
    label: &str,
) -> anyhow::Result<String> {
    if sha.is_none() && path.is_file() {
        if let Ok(sha) = std::fs::read_to_string(format!("{}.sha256", path.display())) {
            return Ok(sha.trim().to_string());
        }
    }
    downloader::download(&downloader::Request {
        url,
        dest: path,
        sha256: sha,
        size_bytes: (size_gb * 1e9) as u64,
        label,
    })
}

/// Downloads the model and its extra files if needed. Returns the main file's SHA-256.
pub fn ensure(r: &Resolved) -> anyhow::Result<String> {
    if r.sha256.is_none() {
        ui::warn("custom model: there is no published checksum, so the file's hash is recorded on first download");
    }
    let sha = ensure_file(
        &r.url,
        &r.path,
        r.sha256.as_deref(),
        r.size_gb,
        &format!("model {}", r.name),
    )?;
    for f in &r.extras {
        ensure_file(
            &f.url,
            &f.path,
            f.sha256.as_deref(),
            f.size_gb,
            &format!("{} {}", f.role, file_name(&f.path.display().to_string())),
        )?;
    }
    Ok(sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;

    #[test]
    fn chat_model_gets_its_projector() {
        let c = Catalog::bundled();
        let mut cfg = sample();
        cfg.model.id = "qwen2.5-vl-3b".into();
        let r = resolve(&cfg, &c).unwrap();
        assert!(r.extra("mmproj").is_some());
        cfg.model.mmproj = String::new();
        assert!(resolve(&cfg, &c).unwrap().extra("mmproj").is_none());
    }

    #[test]
    fn capability_models_are_checked_by_kind() {
        let c = Catalog::bundled();
        let mut cfg = sample();
        cfg.image.enabled = true;
        cfg.image.model = "whisper-base".into();
        assert!(resolve_cap(&cfg, Capability::Image, &c).is_err());
        cfg.image.model = "flux1-schnell".into();
        let r = resolve_cap(&cfg, Capability::Image, &c).unwrap();
        let files = r.files();
        assert!(files.contains_key("diffusion_model"));
        assert!(files.contains_key("vae") && files.contains_key("t5xxl") && files.contains_key("clip_l"));
        cfg.model.id = "sd-turbo".into();
        assert!(resolve(&cfg, &c).is_err(), "image model as chat model");
    }
}

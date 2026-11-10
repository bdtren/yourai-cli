use std::collections::BTreeMap;
use std::path::PathBuf;

use super::catalog::{Catalog, FileRef, Model};
use super::config::{Capability, Config};
use super::store::Store;
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

fn extra_file(models_root: &std::path::Path, m: &Model, role: &str, f: &FileRef) -> ModelFile {
    ModelFile {
        role: role.to_string(),
        url: m.file_url(f),
        path: models_root.join(&m.id).join(file_name(&f.file)),
        sha256: Some(f.sha256.clone()),
        size_gb: f.size_gb,
    }
}

fn from_catalog(
    models_root: &std::path::Path,
    m: &Model,
    quant: &str,
    key: &str,
    mmproj: bool,
) -> anyhow::Result<Resolved> {
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
            extras.push(extra_file(models_root, m, "mmproj", f));
        }
    }
    for c in &m.components {
        extras.push(extra_file(models_root, m, &c.role, c));
    }
    Ok(Resolved {
        id: m.id.clone(),
        name: m.name.clone(),
        url: m.download_url(quant).unwrap(),
        path: models_root.join(&m.id).join(file_name(&q.file)),
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
pub fn resolve(cfg: &Config, catalog: &Catalog, models_root: &std::path::Path) -> anyhow::Result<Resolved> {
    if cfg.model.id == "custom" {
        let safe_repo = cfg.model.repo.replace('/', "__");
        if safe_repo.contains("..") || !safe_file(&cfg.model.file) {
            anyhow::bail!("model.repo / model.file contain unsafe characters");
        }
        let dir = models_root.join("custom").join(&safe_repo);
        let rev = if cfg.model.revision.is_empty() {
            "main"
        } else {
            cfg.model.revision.as_str()
        };
        let hf = |f: &str| format!("https://huggingface.co/{}/resolve/{rev}/{f}", cfg.model.repo);
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
    let mut r = from_catalog(
        models_root,
        m,
        &cfg.model.quant,
        "model.quant",
        cfg.model.mmproj == "auto",
    )?;
    let mm = cfg.model.mmproj.as_str();
    if !mm.is_empty() && mm != "auto" {
        if !safe_file(mm) {
            anyhow::bail!("model.mmproj contains unsafe characters");
        }
        r.extras.push(ModelFile {
            role: "mmproj".into(),
            url: format!("https://huggingface.co/{}/resolve/{}/{mm}", m.repo, m.revision),
            path: models_root.join(&m.id).join(mm),
            sha256: None,
            size_gb: 0.0,
        });
    }
    Ok(r)
}

/// The model for an enabled capability.
pub fn resolve_cap(
    cfg: &Config,
    cap: Capability,
    catalog: &Catalog,
    models_root: &std::path::Path,
) -> anyhow::Result<Resolved> {
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
    from_catalog(models_root, m, quant, &format!("{key}.quant"), true)
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

fn download_one(
    url: &str,
    path: &std::path::Path,
    sha: Option<&str>,
    size_gb: f64,
    label: &str,
    auth_token: Option<&str>,
) -> anyhow::Result<String> {
    downloader::download(&downloader::Request {
        url,
        dest: path,
        sha256: sha,
        size_bytes: (size_gb * 1e9) as u64,
        label,
        auth_token,
    })
}

/// Downloads the model and its extra files into the project module (using the shared
/// cache when allowed). Returns the main file's SHA-256.
pub fn ensure(store: &Store, r: &Resolved) -> anyhow::Result<String> {
    if r.sha256.is_none() {
        ui::warn("custom model: there is no published checksum, so the file's hash is recorded on first download");
    }
    let token = super::secrets::hf_token(&store.root);
    let token_ref = token.as_deref();
    let cache = store.cache_for_model(&r.path);
    let sha = super::store::ensure_file(store, &r.path, cache.as_deref(), |dest| {
        download_one(
            &r.url,
            dest,
            r.sha256.as_deref(),
            r.size_gb,
            &format!("model {}", r.name),
            token_ref,
        )
    })?;
    for f in &r.extras {
        let cache = store.cache_for_model(&f.path);
        super::store::ensure_file(store, &f.path, cache.as_deref(), |dest| {
            download_one(
                &f.url,
                dest,
                f.sha256.as_deref(),
                f.size_gb,
                &format!("{} {}", f.role, file_name(&f.path.display().to_string())),
                token_ref,
            )
        })?;
    }
    Ok(sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;

    #[test]
    fn custom_model_uses_revision_in_url() {
        let c = Catalog::bundled();
        let mut cfg = sample();
        cfg.model.id = "custom".into();
        cfg.model.quant = "custom".into();
        cfg.model.repo = "owner/name".into();
        cfg.model.file = "model.Q4_K_M.gguf".into();
        cfg.model.revision = "abc123".into();
        cfg.model.mmproj = String::new();
        let root = tempfile::tempdir().unwrap();
        let models = root.path().join("models");
        let r = resolve(&cfg, &c, &models).unwrap();
        assert!(r.url.contains("/resolve/abc123/model.Q4_K_M.gguf"));
        assert!(r.extras.is_empty());
        cfg.model.mmproj = "mmproj.gguf".into();
        let r = resolve(&cfg, &c, &models).unwrap();
        assert_eq!(r.extra("mmproj").unwrap().url.contains("/resolve/abc123/mmproj.gguf"), true);
    }

    #[test]
    fn chat_model_gets_its_projector() {
        let c = Catalog::bundled();
        let mut cfg = sample();
        cfg.model.id = "qwen2.5-vl-3b".into();
        let root = tempfile::tempdir().unwrap();
        let models = root.path().join("models");
        let r = resolve(&cfg, &c, &models).unwrap();
        assert!(r.extra("mmproj").is_some());
        cfg.model.mmproj = String::new();
        assert!(resolve(&cfg, &c, &models).unwrap().extra("mmproj").is_none());
    }

    #[test]
    fn capability_models_are_checked_by_kind() {
        let c = Catalog::bundled();
        let mut cfg = sample();
        cfg.image.enabled = true;
        cfg.image.model = "whisper-base".into();
        let root = tempfile::tempdir().unwrap();
        let models = root.path().join("models");
        assert!(resolve_cap(&cfg, Capability::Image, &c, &models).is_err());
        cfg.image.model = "flux1-schnell".into();
        let r = resolve_cap(&cfg, Capability::Image, &c, &models).unwrap();
        assert_eq!(r.main_role(), "diffusion_model");
        let roles: Vec<_> = r.extras.iter().map(|f| f.role.as_str()).collect();
        assert!(roles.contains(&"vae") && roles.contains(&"t5xxl") && roles.contains(&"clip_l"));
        cfg.model.id = "sd-turbo".into();
        assert!(resolve(&cfg, &c, &models).is_err(), "image model as chat model");
    }
}

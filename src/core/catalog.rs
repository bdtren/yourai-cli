use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const BUNDLED: &str = include_str!("../../catalog/catalog.json");

/// Engine names in the catalog.
pub const LLAMA: &str = "llama-server";
pub const SD: &str = "sd-server";
pub const WHISPER: &str = "whisper-server";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub updated: String,
    pub engines: BTreeMap<String, EngineEntry>,
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEntry {
    pub version: String,
    pub release_base: String,
    /// Key like `macos-aarch64-metal` -> archives to unpack into the same folder.
    pub assets: BTreeMap<String, Vec<Asset>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub file: String,
    pub sha256: String,
    pub size_mb: f64,
    /// Full download URL when the archive is not under `release_base`.
    /// `{base_url}` is replaced with the yourai download site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl EngineEntry {
    pub fn asset_url(&self, a: &Asset) -> String {
        match &a.url {
            Some(u) => u.replace("{base_url}", &super::paths::base_url()),
            None => format!("{}/{}", self.release_base, a.file),
        }
    }
}

/// What a model is used for. Text, vision and omni models are chat models served by
/// llama-server; the others are separate capabilities with their own engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Text,
    Vision,
    Omni,
    Image,
    Video,
    Stt,
    Tts,
}

impl Kind {
    pub fn is_chat(self) -> bool {
        matches!(self, Kind::Text | Kind::Vision | Kind::Omni)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    pub params_b: f64,
    pub repo: String,
    pub revision: String,
    pub license: String,
    pub license_url: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub context_max: u32,
    #[serde(default)]
    pub kv_gb_per_1k_ctx: f64,
    pub quants: BTreeMap<String, QuantFile>,
    /// Multimodal projector (image/audio encoder) loaded next to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmproj: Option<FileRef>,
    /// Extra files an image or video model needs (vae, text encoders).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<FileRef>,
    /// Working memory on top of the weights while generating, in GB.
    #[serde(default)]
    pub work_gb: f64,
    /// Engine defaults for image/video models (main, width, height, steps, ...).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub defaults: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantFile {
    pub file: String,
    pub sha256: String,
    pub size_gb: f64,
}

/// A file that belongs to a model but is not one of its quantizations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    /// For components: the engine argument it feeds (`vae`, `t5xxl`, `clip_l`, ...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    pub file: String,
    pub sha256: String,
    pub size_gb: f64,
    /// Defaults to the model's repo and revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// Runtime overhead (compute buffers, engine) added to every estimate, in GB.
pub const OVERHEAD_GB: f64 = 0.4;

impl Model {
    fn extras_gb(&self) -> f64 {
        self.mmproj.as_ref().map(|f| f.size_gb).unwrap_or(0.0)
            + self.components.iter().map(|c| c.size_gb).sum::<f64>()
            + self.work_gb
    }

    /// Estimated memory needed to run this model with `quant` and `ctx` tokens of context.
    pub fn memory_gb(&self, quant: &str, ctx: u32) -> Option<f64> {
        let q = self.quants.get(quant)?;
        Some(q.size_gb + self.kv_gb_per_1k_ctx * ctx as f64 / 1000.0 + self.extras_gb() + OVERHEAD_GB)
    }

    /// Total download size for one quantization, including projector and components.
    pub fn download_gb(&self, quant: &str) -> Option<f64> {
        let q = self.quants.get(quant)?;
        Some(
            q.size_gb
                + self.mmproj.as_ref().map(|f| f.size_gb).unwrap_or(0.0)
                + self.components.iter().map(|c| c.size_gb).sum::<f64>(),
        )
    }

    pub fn download_url(&self, quant: &str) -> Option<String> {
        let q = self.quants.get(quant)?;
        Some(hf_url(&self.repo, &self.revision, &q.file))
    }

    pub fn file_url(&self, f: &FileRef) -> String {
        hf_url(
            f.repo.as_deref().unwrap_or(&self.repo),
            f.revision.as_deref().unwrap_or(&self.revision),
            &f.file,
        )
    }

    /// The quantization to use when none was chosen: Q4_K_M if offered, else the smallest.
    pub fn default_quant(&self) -> &str {
        if self.quants.contains_key("Q4_K_M") {
            return "Q4_K_M";
        }
        self.quants
            .iter()
            .min_by(|a, b| a.1.size_gb.total_cmp(&b.1.size_gb))
            .map(|(k, _)| k.as_str())
            .unwrap_or("")
    }

    pub fn size_label(&self) -> &'static str {
        match self.params_b {
            p if p < 5.0 => "Small",
            p if p < 10.0 => "Medium",
            p if p < 20.0 => "Large",
            _ => "Extra large",
        }
    }
}

fn hf_url(repo: &str, revision: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/{revision}/{file}")
}

impl Catalog {
    pub fn bundled() -> Catalog {
        serde_json::from_str(BUNDLED).expect("bundled catalog is valid JSON")
    }

    /// Cached remote copy if it is at least as new as the bundled one, else the bundled copy.
    /// Never touches the network.
    pub fn load() -> Catalog {
        let bundled = Self::bundled();
        let cached = std::fs::read_to_string(super::paths::catalog_cache())
            .ok()
            .and_then(|t| serde_json::from_str::<Catalog>(&t).ok());
        match cached {
            Some(c) if c.version == bundled.version && c.updated >= bundled.updated => c,
            _ => bundled,
        }
    }

    /// Each catalog format is published as its own file so older CLIs keep working.
    pub fn url() -> String {
        std::env::var("YOURAI_CATALOG_URL")
            .unwrap_or_else(|_| format!("{}/catalog.v{}.json", super::paths::base_url(), Self::bundled().version))
    }

    /// Downloads the latest catalog and caches it. Returns the catalog in use afterwards
    /// and whether the refresh worked.
    pub fn refresh() -> (Catalog, Result<(), String>) {
        let res = (|| -> anyhow::Result<()> {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()?;
            let text = client.get(Self::url()).send()?.error_for_status()?.text()?;
            let parsed: Catalog = serde_json::from_str(&text)?;
            if parsed.version != Self::bundled().version {
                anyhow::bail!(
                    "catalog format v{} is not supported by this CLI; run `yourai self-update`",
                    parsed.version
                );
            }
            super::paths::ensure_home()?;
            std::fs::write(super::paths::catalog_cache(), text)?;
            Ok(())
        })();
        (Self::load(), res.map_err(|e| e.to_string()))
    }

    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn models_of(&self, kind: Kind) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(move |m| m.kind == kind)
    }

    pub fn engine(&self, name: &str) -> Option<&EngineEntry> {
        self.engines.get(name)
    }

    pub fn llama(&self) -> &EngineEntry {
        self.engine(LLAMA).expect("catalog has llama-server")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_complete() {
        let c = Catalog::bundled();
        assert_eq!(c.version, 2);
        assert!(c.models.len() >= 5);
        for m in &c.models {
            assert!(!m.quants.is_empty(), "{} has no files", m.id);
            if m.kind == Kind::Text {
                assert!(m.quants.contains_key("Q4_K_M"), "{} has no Q4_K_M", m.id);
            }
            let files = m
                .quants
                .values()
                .map(|q| (&q.file, &q.sha256))
                .chain(m.mmproj.iter().chain(&m.components).map(|f| (&f.file, &f.sha256)));
            for (file, sha) in files {
                assert_eq!(sha.len(), 64, "{} {file} sha256", m.id);
                assert!(
                    file.ends_with(".gguf") || file.ends_with(".bin") || file.ends_with(".safetensors"),
                    "{} {file}",
                    m.id
                );
            }
            match m.kind {
                Kind::Vision | Kind::Omni | Kind::Tts => assert!(m.mmproj.is_some(), "{} needs mmproj", m.id),
                Kind::Image | Kind::Video => {
                    let main = m.defaults.get("main").and_then(|v| v.as_str()).unwrap_or("");
                    assert!(matches!(main, "model" | "diffusion_model"), "{} main", m.id);
                }
                _ => {}
            }
        }
        for kind in [Kind::Vision, Kind::Omni, Kind::Image, Kind::Video, Kind::Stt, Kind::Tts] {
            assert!(c.models_of(kind).next().is_some(), "no {kind:?} model");
        }
        let e = c.llama();
        for key in ["macos-aarch64-metal", "linux-x86_64-cpu", "windows-x86_64-cpu"] {
            assert!(e.assets.contains_key(key), "missing {key}");
        }
        for name in [SD, WHISPER] {
            let e = c.engine(name).unwrap();
            for key in ["linux-x86_64-cpu", "windows-x86_64-cpu"] {
                assert!(e.assets.contains_key(key), "{name} missing {key}");
            }
            for assets in e.assets.values() {
                for a in assets {
                    assert_eq!(a.sha256.len(), 64, "{name} {}", a.file);
                }
            }
        }
    }

    #[test]
    fn memory_estimate_grows_with_context() {
        let c = Catalog::bundled();
        let m = c.model("qwen2.5-7b").unwrap();
        assert!(m.memory_gb("Q4_K_M", 8192).unwrap() > m.memory_gb("Q4_K_M", 2048).unwrap());
    }

    #[test]
    fn extras_count_toward_memory_and_download() {
        let c = Catalog::bundled();
        let vl = c.model("qwen2.5-vl-3b").unwrap();
        let q = &vl.quants["Q4_K_M"];
        assert!(vl.download_gb("Q4_K_M").unwrap() > q.size_gb + 0.5);
        let wan = c.model("wan2.1-t2v-1.3b").unwrap();
        assert!(wan.memory_gb(wan.default_quant(), 0).unwrap() > 8.0);
        assert!(wan.file_url(&wan.components[0]).contains("Comfy-Org"));
    }

    #[test]
    fn asset_urls_use_base_url_placeholder() {
        let e = EngineEntry {
            version: "v1".into(),
            release_base: "https://example.com/r".into(),
            assets: BTreeMap::new(),
        };
        let plain = Asset {
            file: "a.zip".into(),
            sha256: String::new(),
            size_mb: 1.0,
            url: None,
        };
        assert_eq!(e.asset_url(&plain), "https://example.com/r/a.zip");
        let own = Asset {
            url: Some("{base_url}/engines/x/a.zip".into()),
            ..plain
        };
        assert!(e.asset_url(&own).ends_with("/engines/x/a.zip"));
        assert!(!e.asset_url(&own).contains("{base_url}"));
    }
}

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const BUNDLED: &str = include_str!("../../catalog/catalog.json");

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub params_b: f64,
    pub repo: String,
    pub revision: String,
    pub license: String,
    pub license_url: String,
    pub tags: Vec<String>,
    pub context_max: u32,
    pub kv_gb_per_1k_ctx: f64,
    pub quants: BTreeMap<String, QuantFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantFile {
    pub file: String,
    pub sha256: String,
    pub size_gb: f64,
}

/// Runtime overhead (compute buffers, engine) added to every estimate, in GB.
pub const OVERHEAD_GB: f64 = 0.4;

impl Model {
    /// Estimated memory needed to run this model with `quant` and `ctx` tokens of context.
    pub fn memory_gb(&self, quant: &str, ctx: u32) -> Option<f64> {
        let q = self.quants.get(quant)?;
        Some(q.size_gb + self.kv_gb_per_1k_ctx * ctx as f64 / 1000.0 + OVERHEAD_GB)
    }

    pub fn download_url(&self, quant: &str) -> Option<String> {
        let q = self.quants.get(quant)?;
        Some(format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, q.file
        ))
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

    pub fn url() -> String {
        std::env::var("YOURAI_CATALOG_URL").unwrap_or_else(|_| format!("{}/catalog.json", super::paths::base_url()))
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

    pub fn llama(&self) -> &EngineEntry {
        self.engines.get("llama-server").expect("catalog has llama-server")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_complete() {
        let c = Catalog::bundled();
        assert!(c.models.len() >= 5);
        for m in &c.models {
            assert!(m.quants.contains_key("Q4_K_M"), "{} has no Q4_K_M", m.id);
            for q in m.quants.values() {
                assert_eq!(q.sha256.len(), 64, "{} sha256", m.id);
                assert!(q.file.ends_with(".gguf"));
            }
        }
        let e = c.llama();
        for key in ["macos-aarch64-metal", "linux-x86_64-cpu", "windows-x86_64-cpu"] {
            assert!(e.assets.contains_key(key), "missing {key}");
        }
    }

    #[test]
    fn memory_estimate_grows_with_context() {
        let c = Catalog::bundled();
        let m = c.model("qwen2.5-7b").unwrap();
        assert!(m.memory_gb("Q4_K_M", 8192).unwrap() > m.memory_gb("Q4_K_M", 2048).unwrap());
    }
}

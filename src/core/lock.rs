use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::config::{Capability, Config};
use super::rag;

/// What `yourai build` produced, plus hashes of the config it was built from.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Lock {
    pub sections: BTreeMap<String, String>,
    pub engine_version: String,
    pub engine_key: String,
    pub engine_exe: String,
    pub model_id: String,
    pub model_quant: String,
    pub model_path: String,
    pub model_sha256: String,
    /// Projector for vision and omni chat models.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mmproj_path: String,
    /// Engine name (`sd-server`, `whisper-server`, `llama-tts`) -> installed build.
    #[serde(default)]
    pub engines: BTreeMap<String, EngineLock>,
    /// Capability key (`image`, `speech_to_text`, ...) -> its model.
    #[serde(default)]
    pub models: BTreeMap<String, ModelLock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EngineLock {
    pub version: String,
    pub key: String,
    pub exe: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelLock {
    pub id: String,
    pub quant: String,
    pub sha256: String,
    /// Engine argument role (`model`, `diffusion_model`, `vae`, `mmproj`, ...) -> path.
    pub files: BTreeMap<String, String>,
    /// The role of the main weights in `files`.
    pub main: String,
    #[serde(default)]
    pub defaults: serde_json::Value,
}

impl ModelLock {
    pub fn main_path(&self) -> Option<&str> {
        self.files.get(&self.main).map(String::as_str)
    }
}

fn hash<T: Serialize>(v: &T) -> String {
    let json = serde_json::to_vec(v).expect("serializable");
    hex::encode(Sha256::digest(&json))
}

/// Sections that need downloads or engine installs when they change.
pub const BUILD_SECTIONS: [&str; 6] = ["model", "runtime", "image", "video", "speech_to_text", "text_to_speech"];

/// Hashes each part of the config that changes what rebuild must do.
/// `root` is the project folder (used for the RAG docs fingerprint when enabled).
pub fn section_hashes(cfg: &Config, system_prompt_text: &str, root: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("model".into(), hash(&cfg.model));
    m.insert("runtime".into(), hash(&cfg.runtime));
    m.insert("generation".into(), hash(&cfg.generation));
    m.insert("server".into(), hash(&cfg.server));
    m.insert("system_prompt".into(), hash(&system_prompt_text));
    for c in Capability::ALL {
        let v = match c {
            Capability::Image => hash(&cfg.image),
            Capability::Video => hash(&cfg.video),
            Capability::SpeechToText => hash(&cfg.speech_to_text),
            Capability::TextToSpeech => hash(&cfg.text_to_speech),
        };
        m.insert(c.key().into(), v);
    }
    // Config + docs fingerprint so rebuild re-indexes when files under docs_dir change.
    let rag_key = if cfg.rag.enabled {
        format!("{}:{}", hash(&cfg.rag), rag::fingerprint(root, &cfg.rag))
    } else {
        hash(&cfg.rag)
    };
    m.insert("rag".into(), rag_key);
    m
}

/// Sections whose hash differs from the lock.
pub fn changed(lock: &Lock, current: &BTreeMap<String, String>) -> Vec<String> {
    current
        .iter()
        .filter(|(k, v)| lock.sections.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect()
}

pub fn load(path: &Path) -> Option<Lock> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save(path: &Path, lock: &Lock) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string_pretty(lock)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;

    #[test]
    fn detects_changed_sections() {
        let cfg = sample();
        let lock = Lock {
            sections: section_hashes(&cfg, "hi", Path::new(".")),
            ..Default::default()
        };
        assert!(changed(&lock, &section_hashes(&cfg, "hi", Path::new("."))).is_empty());

        let mut c2 = cfg.clone();
        c2.generation.temperature = 0.2;
        assert_eq!(changed(&lock, &section_hashes(&c2, "hi", Path::new("."))), vec!["generation"]);

        let mut c3 = cfg.clone();
        c3.model.quant = "Q8_0".into();
        let ch = changed(&lock, &section_hashes(&c3, "bye", Path::new(".")));
        assert_eq!(ch, vec!["model", "system_prompt"]);

        let mut c4 = cfg.clone();
        c4.image.steps = 4;
        assert_eq!(changed(&lock, &section_hashes(&c4, "hi", Path::new("."))), vec!["image"]);
    }

    #[test]
    fn v1_lock_loads() {
        let text = r#"{"sections":{},"engine_version":"b1","engine_key":"k","engine_exe":"/e",
            "model_id":"m","model_quant":"Q4_K_M","model_path":"/m","model_sha256":"x"}"#;
        let l: Lock = serde_json::from_str(text).unwrap();
        assert!(l.engines.is_empty() && l.models.is_empty());
    }
}

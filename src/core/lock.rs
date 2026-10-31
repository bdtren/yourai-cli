use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::config::Config;

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
}

fn hash<T: Serialize>(v: &T) -> String {
    let json = serde_json::to_vec(v).expect("serializable");
    hex::encode(Sha256::digest(&json))
}

/// Hashes each part of the config that changes what rebuild must do.
pub fn section_hashes(cfg: &Config, system_prompt_text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("model".into(), hash(&cfg.model));
    m.insert("runtime".into(), hash(&cfg.runtime));
    m.insert("generation".into(), hash(&cfg.generation));
    m.insert("server".into(), hash(&cfg.server));
    m.insert("system_prompt".into(), hash(&system_prompt_text));
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
            sections: section_hashes(&cfg, "hi"),
            ..Default::default()
        };
        assert!(changed(&lock, &section_hashes(&cfg, "hi")).is_empty());

        let mut c2 = cfg.clone();
        c2.generation.temperature = 0.2;
        assert_eq!(changed(&lock, &section_hashes(&c2, "hi")), vec!["generation"]);

        let mut c3 = cfg.clone();
        c3.model.quant = "Q8_0".into();
        let ch = changed(&lock, &section_hashes(&c3, "bye"));
        assert_eq!(ch, vec!["model", "system_prompt"]);
    }
}

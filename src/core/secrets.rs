use std::path::Path;

/// Reads the API key: YOURAI_API_KEY wins over .yourai/secrets.toml.
pub fn api_key(project_root: &Path) -> Option<String> {
    if let Ok(k) = std::env::var("YOURAI_API_KEY") {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    read_field(project_root, "api_key")
}

/// Hugging Face token: `HF_TOKEN` env wins over `.yourai/secrets.toml` `hf_token`.
pub fn hf_token(project_root: &Path) -> Option<String> {
    if let Ok(k) = std::env::var("HF_TOKEN") {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    read_field(project_root, "hf_token")
}

fn read_field(project_root: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path(project_root)).ok()?;
    let value: toml::Table = toml::from_str(&text).ok()?;
    value
        .get(key)?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub fn path(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".yourai").join("secrets.toml")
}

pub fn generate_key() -> String {
    let bytes: [u8; 24] = rand::random();
    format!("yai-{}", hex::encode(bytes))
}

fn load_table(project_root: &Path) -> toml::Table {
    std::fs::read_to_string(path(project_root))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_table(project_root: &Path, table: &toml::Table) -> anyhow::Result<()> {
    let p = path(project_root);
    std::fs::create_dir_all(p.parent().unwrap())?;
    let mut out = String::from("# Keep this file private. It is gitignored.\n");
    out.push_str(&toml::to_string_pretty(table)?);
    std::fs::write(&p, out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn set_field(project_root: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let mut table = load_table(project_root);
    table.insert(key.to_string(), toml::Value::String(value.to_string()));
    write_table(project_root, &table)
}

/// Writes a new API key, preserving other secrets (e.g. `hf_token`).
pub fn write_key(project_root: &Path, key: &str) -> anyhow::Result<()> {
    set_field(project_root, "api_key", key)
}

/// Writes an HF token, preserving `api_key` and other fields.
pub fn write_hf_token(project_root: &Path, token: &str) -> anyhow::Result<()> {
    set_field(project_root, "hf_token", token)
}

/// Shows only the first characters of a secret.
pub fn redact(key: &str) -> String {
    let shown: String = key.chars().take(8).collect();
    format!("{shown}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrip_and_redaction() {
        let dir = tempfile::tempdir().unwrap();
        let key = generate_key();
        assert!(key.len() > 40);
        write_key(dir.path(), &key).unwrap();
        if std::env::var("YOURAI_API_KEY").is_err() {
            assert_eq!(api_key(dir.path()).unwrap(), key);
        }
        assert!(!redact(&key).contains(&key[10..]));
    }

    #[test]
    fn hf_token_merges_with_api_key() {
        let dir = tempfile::tempdir().unwrap();
        write_key(dir.path(), "yai-testkey000000000000000000000000").unwrap();
        write_hf_token(dir.path(), "hf_secret_token").unwrap();
        if std::env::var("YOURAI_API_KEY").is_err() {
            assert_eq!(api_key(dir.path()).unwrap(), "yai-testkey000000000000000000000000");
        }
        if std::env::var("HF_TOKEN").is_err() {
            assert_eq!(hf_token(dir.path()).unwrap(), "hf_secret_token");
        }
        // rewriting api_key must keep hf_token
        write_key(dir.path(), "yai-otherkey00000000000000000000000").unwrap();
        if std::env::var("HF_TOKEN").is_err() {
            assert_eq!(hf_token(dir.path()).unwrap(), "hf_secret_token");
        }
    }
}

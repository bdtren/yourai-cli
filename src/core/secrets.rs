use std::path::Path;

/// Reads the API key: YOURAI_API_KEY wins over .yourai/secrets.toml.
pub fn api_key(project_root: &Path) -> Option<String> {
    if let Ok(k) = std::env::var("YOURAI_API_KEY") {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    let text = std::fs::read_to_string(path(project_root)).ok()?;
    let value: toml::Table = toml::from_str(&text).ok()?;
    value
        .get("api_key")?
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

/// Writes a new key with owner-only permissions where the OS supports it.
pub fn write_key(project_root: &Path, key: &str) -> anyhow::Result<()> {
    let p = path(project_root);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(
        &p,
        format!("# Keep this file private. It is gitignored.\napi_key = \"{key}\"\n"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
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
}

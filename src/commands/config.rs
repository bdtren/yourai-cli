use toml_edit::{value, DocumentMut, Item, Value};

use crate::cli::ConfigCmd;
use crate::core::config::{change_impact, Config};
use crate::core::project::Project;
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

pub fn run(p: &Project, cmd: ConfigCmd) -> anyhow::Result<()> {
    match cmd {
        ConfigCmd::Show => {
            print!("{}", std::fs::read_to_string(p.config_path())?);
            Ok(())
        }
        ConfigCmd::Get { key } => {
            let doc = read_doc(p)?;
            let item = lookup(&doc, &key).ok_or_else(|| unknown_key(&key))?;
            match item {
                Item::Value(Value::String(s)) => println!("{}", s.value()),
                Item::Value(v) => println!("{}", v.to_string().trim()),
                other => println!("{}", other.to_string().trim()),
            }
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            set_value(p, &key, &value)?;
            ui::ok(format!("{key} = {value}"));
            ui::info(change_impact(&key));
            Ok(())
        }
        ConfigCmd::Edit => {
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| if cfg!(windows) { "notepad".into() } else { "vi".into() });
            let mut parts = editor.split_whitespace();
            let exe = parts.next().unwrap_or("vi");
            let status = std::process::Command::new(exe)
                .args(parts)
                .arg(p.config_path())
                .status()?;
            if !status.success() {
                return fail_hint(
                    Exit::General,
                    format!("{editor} exited with an error"),
                    "set $EDITOR to your editor",
                );
            }
            match p.config() {
                Ok(_) => {
                    ui::ok("yourai.toml is valid");
                    ui::next("yourai rebuild");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        ConfigCmd::Validate => {
            let cfg = p.config()?;
            let prompt_path = crate::core::paths::inside(&p.root, &cfg.system_prompt)?;
            if !prompt_path.is_file() {
                ui::warn(format!(
                    "{} does not exist; chat will run without a system prompt",
                    cfg.system_prompt
                ));
            }
            if cfg.server.require_api_key && crate::core::secrets::api_key(&p.root).is_none() {
                return fail_hint(
                    Exit::Security,
                    "server.require_api_key is true but no API key is set",
                    "set YOURAI_API_KEY or create .yourai/secrets.toml with api_key = \"...\"",
                );
            }
            ui::ok("yourai.toml is valid");
            Ok(())
        }
    }
}

fn unknown_key(key: &str) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, format!("unknown setting `{key}`"))
        .hint("see the keys with `yourai config show`")
        .into()
}

fn read_doc(p: &Project) -> anyhow::Result<DocumentMut> {
    let text = std::fs::read_to_string(p.config_path())?;
    text.parse::<DocumentMut>()
        .map_err(|e| CliError::new(Exit::InvalidConfig, format!("yourai.toml is not valid TOML:\n{e}")).into())
}

fn lookup<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for part in key.split('.') {
        item = item.get(part)?;
    }
    Some(item)
}

/// Sets `key` to `raw`, keeping the type of the existing value and all comments.
/// The file is only written if the result is a valid config.
pub fn set_value(p: &Project, key: &str, raw: &str) -> anyhow::Result<()> {
    if key.contains("api_key") && !key.ends_with("require_api_key") {
        return fail_hint(
            Exit::Security,
            "API keys are never stored in yourai.toml",
            "set YOURAI_API_KEY, or put api_key = \"...\" in .yourai/secrets.toml",
        );
    }
    p.upgrade_config()?;
    let mut doc = read_doc(p)?;
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() > 2 || parts.iter().any(|s| s.is_empty()) {
        return Err(unknown_key(key));
    }
    let (table, leaf) = if parts.len() == 2 {
        (Some(parts[0]), parts[1])
    } else {
        (None, parts[0])
    };
    let container = match table {
        Some(t) => doc
            .get_mut(t)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| unknown_key(key))?,
        None => doc.as_table_mut(),
    };
    let existing = container.get(leaf).and_then(Item::as_value).cloned();
    let creatable = matches!(key, "model.repo" | "model.file");
    let new_value = match existing {
        Some(Value::Integer(_)) => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| type_err(key, "a whole number"))?,
        Some(Value::Float(_)) => raw
            .parse::<f64>()
            .map(Value::from)
            .map_err(|_| type_err(key, "a number"))?,
        Some(Value::Boolean(_)) => raw
            .parse::<bool>()
            .map(Value::from)
            .map_err(|_| type_err(key, "true or false"))?,
        Some(Value::String(_)) => Value::from(raw),
        Some(Value::Array(_)) => {
            let mut arr = toml_edit::Array::new();
            for s in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                arr.push(s);
            }
            Value::Array(arr)
        }
        Some(_) => return Err(type_err(key, "a simple value")),
        None if creatable => Value::from(raw),
        None => return Err(unknown_key(key)),
    };
    match container.get_mut(leaf) {
        Some(slot) => {
            let decor = slot.as_value().map(|v| v.decor().clone());
            *slot = value(new_value);
            if let (Some(d), Some(v)) = (decor, slot.as_value_mut()) {
                *v.decor_mut() = d;
            }
        }
        None => {
            container.insert(leaf, value(new_value));
        }
    }

    let text = doc.to_string();
    Config::parse(&text)?;
    std::fs::write(p.config_path(), text)?;
    Ok(())
}

fn type_err(key: &str, want: &str) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, format!("{key} must be {want}")).into()
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
    fn set_keeps_comments_and_types() {
        let (_t, p) = proj();
        set_value(&p, "generation.temperature", "0.5").unwrap();
        set_value(&p, "server.port", "9000").unwrap();
        let text = std::fs::read_to_string(p.config_path()).unwrap();
        assert!(text.contains("# Higher is more creative"));
        let cfg = p.config().unwrap();
        assert_eq!(cfg.generation.temperature, 0.5);
        assert_eq!(cfg.server.port, 9000);
    }

    #[test]
    fn set_upgrades_v1_and_sets_lists() {
        let (_t, p) = proj();
        std::fs::write(p.config_path(), config::V1_SAMPLE).unwrap();
        set_value(&p, "agent.tools", "read_file, list_files").unwrap();
        set_value(&p, "image.model", "sd-turbo").unwrap();
        set_value(&p, "image.enabled", "true").unwrap();
        let text = std::fs::read_to_string(p.config_path()).unwrap();
        assert!(text.contains("# my favourite model"));
        let cfg = p.config().unwrap();
        assert_eq!(cfg.agent.tools, vec!["read_file", "list_files"]);
        assert!(cfg.image.enabled);
        assert!(set_value(&p, "agent.tools", "format_disk").is_err());
    }

    #[test]
    fn set_rejects_bad_values_without_writing() {
        let (_t, p) = proj();
        let before = std::fs::read_to_string(p.config_path()).unwrap();
        assert!(set_value(&p, "server.port", "abc").is_err());
        assert!(set_value(&p, "generation.temperature", "9").is_err());
        assert!(set_value(&p, "server.host", "0.0.0.0").is_err());
        assert!(set_value(&p, "nope.key", "1").is_err());
        assert!(set_value(&p, "server.api_key", "secret").is_err());
        assert_eq!(before, std::fs::read_to_string(p.config_path()).unwrap());
    }
}

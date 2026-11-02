//! Project `api/routes.toml` — public HTTP/WS mounts for the gateway.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

use crate::core::paths;
use crate::error::{CliError, Exit};

pub const BUILTIN_UPSTREAMS: &[&str] = &["openai_chat", "openai_chat_ws", "claude_messages", "gemini_generate"];

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RoutesFile {
    #[serde(default)]
    pub route: Vec<RouteEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RouteEntry {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub upstream: Option<String>,
    #[serde(default)]
    pub handler: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl RouteEntry {
    pub fn method_norm(&self) -> String {
        self.method.trim().to_ascii_uppercase()
    }

    pub fn default_timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(30_000)
    }
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, msg)
        .hint("fix api/routes.toml, then run `yourai rebuild`")
        .into()
}

/// Load `api/routes.toml` when present. `Ok(None)` if the file is missing.
pub fn load(root: &Path) -> anyhow::Result<Option<RoutesFile>> {
    let path = root.join("api/routes.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| invalid(format!("could not read api/routes.toml: {e}")))?;
    let file: RoutesFile = toml::from_str(&text).map_err(|e| invalid(format!("api/routes.toml is invalid:\n{e}")))?;
    validate(root, &file)?;
    Ok(Some(file))
}

pub fn validate(root: &Path, file: &RoutesFile) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    for (i, r) in file.route.iter().enumerate() {
        let method = r.method_norm();
        if method.is_empty() {
            return Err(invalid(format!("route[{i}]: method is required")));
        }
        if !r.path.starts_with('/') {
            return Err(invalid(format!(
                "route[{i}]: path must start with / (got {:?})",
                r.path
            )));
        }
        let has_up = r.upstream.as_ref().is_some_and(|s| !s.is_empty());
        let has_h = r.handler.as_ref().is_some_and(|s| !s.is_empty());
        match (has_up, has_h) {
            (true, false) => {
                let u = r.upstream.as_deref().unwrap();
                if !BUILTIN_UPSTREAMS.contains(&u) {
                    return Err(invalid(format!(
                        "route[{i}]: unknown upstream {u:?} (choose from {})",
                        BUILTIN_UPSTREAMS.join(", ")
                    )));
                }
            }
            (false, true) => {
                let h = r.handler.as_deref().unwrap();
                paths::inside(root, h).map_err(|e| invalid(format!("route[{i}]: handler: {e}")))?;
            }
            (true, true) => {
                return Err(invalid(format!("route[{i}]: set either upstream or handler, not both")));
            }
            (false, false) => {
                return Err(invalid(format!("route[{i}]: set upstream or handler")));
            }
        }
        let key = format!("{method} {}", r.path);
        if !seen.insert(key.clone()) {
            return Err(invalid(format!("route[{i}]: duplicate method+path {key}")));
        }
    }
    Ok(())
}

/// Default `api/routes.toml` body for the given API styles.
pub fn scaffold_toml(api_styles: &[String]) -> String {
    let mut out = String::from(
        r#"# Public HTTP/WS mounts for this project. Loaded when the gateway starts
# (`yourai start` / `yourai rebuild`). Edit paths, remap built-ins, or add handlers.
# Built-in upstreams: openai_chat | openai_chat_ws | claude_messages | gemini_generate
# Media routes (/v1/images|videos|audio…) stay fixed in the gateway for now.
#
# Custom handlers: JSON on stdin/stdout (see handlers/). Prefer an executable
# script, or a .js/.mjs file run with `node` when available.

"#,
    );
    // Chat Completions always — used by `yourai chat` and most clients.
    out.push_str(
        r#"[[route]]
method = "POST"
path = "/v1/chat/completions"
upstream = "openai_chat"

"#,
    );
    if api_styles.iter().any(|s| s == "openai") {
        out.push_str(
            r#"[[route]]
method = "GET"
path = "/v1/chat/ws"
upstream = "openai_chat_ws"

"#,
        );
    }
    if api_styles.iter().any(|s| s == "claude") {
        out.push_str(
            r#"[[route]]
method = "POST"
path = "/v1/messages"
upstream = "claude_messages"

"#,
        );
    }
    if api_styles.iter().any(|s| s == "gemini") {
        out.push_str(
            r#"[[route]]
method = "POST"
path = "/v1/models/{*path}"
upstream = "gemini_generate"

"#,
        );
    }
    out.push_str(
        r#"[[route]]
method = "POST"
path = "/v2/echo"
handler = "api/handlers/example_echo.js"
# optional: timeout_ms = 30000
"#,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_reject_bad_routes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("api/handlers")).unwrap();
        std::fs::write(root.join("api/handlers/example_echo.js"), "x").unwrap();

        let good = r#"
[[route]]
method = "POST"
path = "/v1/chat/completions"
upstream = "openai_chat"

[[route]]
method = "POST"
path = "/v2/echo"
handler = "api/handlers/example_echo.js"
"#;
        let file: RoutesFile = toml::from_str(good).unwrap();
        validate(root, &file).unwrap();

        let dup = good.to_string()
            + r#"
[[route]]
method = "POST"
path = "/v1/chat/completions"
upstream = "openai_chat"
"#;
        let file: RoutesFile = toml::from_str(&dup).unwrap();
        assert!(validate(root, &file).is_err());

        let bad_up = r#"
[[route]]
method = "POST"
path = "/v1/x"
upstream = "nope"
"#;
        let file: RoutesFile = toml::from_str(bad_up).unwrap();
        assert!(validate(root, &file).is_err());

        let trav = r#"
[[route]]
method = "POST"
path = "/v2/x"
handler = "../outside.js"
"#;
        let file: RoutesFile = toml::from_str(trav).unwrap();
        assert!(validate(root, &file).is_err());
    }

    #[test]
    fn load_missing_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn scaffold_includes_styles() {
        let t = scaffold_toml(&["openai".into(), "claude".into()]);
        assert!(t.contains("path = \"/v1/chat/ws\""));
        assert!(t.contains("upstream = \"openai_chat_ws\""));
        assert!(t.contains("upstream = \"claude_messages\""));
        assert!(!t.contains("upstream = \"gemini_generate\""));
        assert!(t.contains("/v2/echo"));
    }
}

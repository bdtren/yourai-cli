use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CliError, Exit};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub name: String,
    pub goal: String,
    pub system_prompt: String,
    pub model: ModelCfg,
    pub runtime: RuntimeCfg,
    pub generation: GenerationCfg,
    pub server: ServerCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelCfg {
    /// Catalog id, or "custom" to use `repo` + `file`.
    pub id: String,
    pub quant: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCfg {
    pub engine: String,
    /// "auto", "all", or a number of layers.
    pub gpu_layers: String,
    pub context_length: u32,
    /// 0 = let the engine decide.
    pub threads: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GenerationCfg {
    pub temperature: f64,
    pub top_p: f64,
    pub max_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServerCfg {
    pub host: String,
    pub port: u16,
    pub web_ui: bool,
    pub require_api_key: bool,
}

impl ServerCfg {
    pub fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
    }

    /// Address clients on this machine should use.
    pub fn local_url(&self) -> String {
        let host = if self.host == "0.0.0.0" || self.host == "::" {
            "127.0.0.1"
        } else {
            &self.host
        };
        format!("http://{host}:{}", self.port)
    }
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, msg)
        .hint("fix yourai.toml, then run `yourai config validate`")
        .into()
}

impl Config {
    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let cfg: Config = toml::from_str(text).map_err(|e| invalid(format!("yourai.toml is invalid:\n{e}")))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let text =
            std::fs::read_to_string(path).map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
        Self::parse(&text)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid(format!(
                "schema_version {} is not supported (this CLI supports {SCHEMA_VERSION}); run `yourai self-update`",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(invalid("name must not be empty"));
        }
        if !["chat", "code"].contains(&self.goal.as_str()) {
            return Err(invalid("goal must be \"chat\" or \"code\""));
        }
        if self.runtime.engine != "llamacpp" {
            return Err(invalid(format!(
                "runtime.engine \"{}\" is not supported yet; use \"llamacpp\"",
                self.runtime.engine
            )));
        }
        let gl = self.runtime.gpu_layers.as_str();
        if gl != "auto" && gl != "all" && gl.parse::<u32>().is_err() {
            return Err(invalid(
                "runtime.gpu_layers must be \"auto\", \"all\", or a whole number like \"20\"",
            ));
        }
        if !(256..=1_048_576).contains(&self.runtime.context_length) {
            return Err(invalid("runtime.context_length must be between 256 and 1048576"));
        }
        if !(0.0..=2.0).contains(&self.generation.temperature) {
            return Err(invalid("generation.temperature must be between 0.0 and 2.0"));
        }
        if !(0.0..=1.0).contains(&self.generation.top_p) || self.generation.top_p == 0.0 {
            return Err(invalid("generation.top_p must be greater than 0.0 and at most 1.0"));
        }
        if self.generation.max_tokens == 0 || self.generation.max_tokens < -1 {
            return Err(invalid(
                "generation.max_tokens must be -1 (no limit) or a positive number",
            ));
        }
        if self.server.port == 0 {
            return Err(invalid("server.port must be 1-65535"));
        }
        if self.server.host.parse::<std::net::IpAddr>().is_err() && self.server.host != "localhost" {
            return Err(invalid(
                "server.host must be an IP address such as 127.0.0.1 or 0.0.0.0",
            ));
        }
        if !self.server.is_loopback() && !self.server.require_api_key {
            return Err(CliError::new(
                Exit::Security,
                "server.host exposes the AI to your network, so server.require_api_key must be true",
            )
            .hint("run `yourai config set server.require_api_key true`")
            .into());
        }
        if self.model.id == "custom" && (self.model.repo.is_empty() || !self.model.file.ends_with(".gguf")) {
            return Err(invalid(
                "a custom model needs model.repo and a model.file ending in .gguf",
            ));
        }
        super::paths::inside(Path::new("."), &self.system_prompt).map_err(|e| invalid(e.to_string()))?;
        Ok(())
    }
}

/// Writes a fresh, commented yourai.toml.
pub fn render(cfg: &Config) -> String {
    let custom = if cfg.model.id == "custom" {
        format!("repo = \"{}\"\nfile = \"{}\"\n", cfg.model.repo, cfg.model.file)
    } else {
        String::new()
    };
    format!(
        r#"# YourAI project configuration. Edit freely, then run `yourai rebuild`.
# Reference: run `yourai config --help`. Secrets never go in this file
# (API keys live in .yourai/secrets.toml or the YOURAI_API_KEY variable).

schema_version = {schema}
name = "{name}"
# What this AI is for: "chat" or "code"
goal = "{goal}"
# The instructions the AI follows in `yourai chat`. Edit the file freely.
system_prompt = "{prompt}"

[model]
# A model id from the catalog (see `yourai init`), or "custom" with repo + file.
id = "{model_id}"
# Compression level. Q4_K_M is the best balance; Q5_K_M and Q8_0 are larger and more accurate.
quant = "{quant}"
{custom}
[runtime]
engine = "llamacpp"
# "auto" lets the engine put as much as fits on the GPU; "all", or a number of layers.
gpu_layers = "{gpu_layers}"
# How many tokens of text the AI can keep in mind at once. More uses more memory.
context_length = {ctx}
# CPU threads, 0 = automatic
threads = {threads}

[generation]
# Higher is more creative, lower is more focused (0.0 - 2.0)
temperature = {temp}
top_p = {top_p}
# Longest answer in tokens, -1 = no limit
max_tokens = {max_tokens}

[server]
# 127.0.0.1 = only this computer. 0.0.0.0 = other devices on your network (needs an API key).
host = "{host}"
port = {port}
# Browser chat page at http://{host_url}:{port}
web_ui = {web_ui}
require_api_key = {req_key}
"#,
        schema = cfg.schema_version,
        name = cfg.name,
        goal = cfg.goal,
        prompt = cfg.system_prompt,
        model_id = cfg.model.id,
        quant = cfg.model.quant,
        custom = custom,
        gpu_layers = cfg.runtime.gpu_layers,
        ctx = cfg.runtime.context_length,
        threads = cfg.runtime.threads,
        temp = fmt_f(cfg.generation.temperature),
        top_p = fmt_f(cfg.generation.top_p),
        max_tokens = cfg.generation.max_tokens,
        host = cfg.server.host,
        host_url = if cfg.server.is_loopback() {
            "127.0.0.1"
        } else {
            "<this-computer-ip>"
        },
        port = cfg.server.port,
        web_ui = cfg.server.web_ui,
        req_key = cfg.server.require_api_key,
    )
}

fn fmt_f(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// What has to happen after a key changes.
pub fn change_impact(key: &str) -> &'static str {
    match key.split('.').next().unwrap_or("") {
        "model" | "runtime" => "Engine or model change: run `yourai rebuild` (may download files).",
        "generation" | "server" | "system_prompt" | "name" | "goal" => {
            "Restart needed: run `yourai rebuild` (fast, nothing is downloaded)."
        }
        _ => "Run `yourai rebuild` to apply.",
    }
}

#[cfg(test)]
pub fn sample() -> Config {
    Config {
        schema_version: 1,
        name: "my-ai".into(),
        goal: "chat".into(),
        system_prompt: "prompts/system.md".into(),
        model: ModelCfg {
            id: "qwen2.5-0.5b".into(),
            quant: "Q4_K_M".into(),
            repo: String::new(),
            file: String::new(),
        },
        runtime: RuntimeCfg {
            engine: "llamacpp".into(),
            gpu_layers: "auto".into(),
            context_length: 4096,
            threads: 0,
        },
        generation: GenerationCfg {
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 1024,
        },
        server: ServerCfg {
            host: "127.0.0.1".into(),
            port: 8080,
            web_ui: true,
            require_api_key: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_roundtrips() {
        let cfg = sample();
        let parsed = Config::parse(&render(&cfg)).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let text = render(&sample()).replace("threads = 0", "threads = 0\ntypo_key = 1");
        assert!(Config::parse(&text).is_err());
    }

    #[test]
    fn exposed_host_needs_key() {
        let mut cfg = sample();
        cfg.server.host = "0.0.0.0".into();
        assert!(cfg.validate().is_err());
        cfg.server.require_api_key = true;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn bad_values() {
        let mut cfg = sample();
        cfg.generation.temperature = 5.0;
        assert!(cfg.validate().is_err());
        let mut cfg = sample();
        cfg.runtime.gpu_layers = "lots".into();
        assert!(cfg.validate().is_err());
        let mut cfg = sample();
        cfg.system_prompt = "../secret".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rendered_config_has_no_secrets() {
        let mut cfg = sample();
        cfg.server.require_api_key = true;
        let text = render(&cfg);
        assert!(!text.lines().any(|l| l.trim_start().starts_with("api_key")));
    }
}

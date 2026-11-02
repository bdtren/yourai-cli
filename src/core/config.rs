use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CliError, Exit};

pub const SCHEMA_VERSION: u32 = 2;

/// Chat API dialects the gateway can speak under `/v1/`.
pub const API_STYLES: [&str; 3] = ["openai", "claude", "gemini"];

/// Tools the chat agent can use. Media tools also need their capability enabled.
pub const AGENT_TOOLS: [&str; 7] = [
    "list_files",
    "read_file",
    "write_file",
    "generate_image",
    "generate_video",
    "speak",
    "transcribe",
];

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
    #[serde(default)]
    pub store: StoreCfg,
    #[serde(default)]
    pub agent: AgentCfg,
    #[serde(default)]
    pub image: ImageCfg,
    #[serde(default)]
    pub video: VideoCfg,
    #[serde(default)]
    pub speech_to_text: SttCfg,
    #[serde(default)]
    pub text_to_speech: TtsCfg,
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
    /// Image/audio projector: "auto" (from the catalog), "" (none), or a file in `repo`.
    #[serde(default = "auto")]
    pub mmproj: String,
}

fn auto() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct AgentCfg {
    pub enabled: bool,
    /// Folder the agent may read (and write, if allowed), relative to the project.
    pub workspace: String,
    pub allow_write: bool,
    /// Tools to offer; empty = every tool whose capability is enabled.
    pub tools: Vec<String>,
}

impl Default for AgentCfg {
    fn default() -> Self {
        AgentCfg {
            enabled: false,
            workspace: ".".into(),
            allow_write: false,
            tools: Vec::new(),
        }
    }
}

/// `0` in a size or step setting means "use the model's default".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct ImageCfg {
    pub enabled: bool,
    pub model: String,
    pub quant: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct VideoCfg {
    pub enabled: bool,
    pub model: String,
    pub quant: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub fps: u32,
    pub steps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct SttCfg {
    pub enabled: bool,
    pub model: String,
    pub quant: String,
    /// "auto" or a language code like "en".
    pub language: String,
}

impl Default for SttCfg {
    fn default() -> Self {
        SttCfg {
            enabled: false,
            model: String::new(),
            quant: String::new(),
            language: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct TtsCfg {
    pub enabled: bool,
    pub model: String,
    pub quant: String,
    pub language: String,
    /// Optional voice sample file (relative to the project) to clone.
    pub speaker_file: String,
}

impl Default for TtsCfg {
    fn default() -> Self {
        TtsCfg {
            enabled: false,
            model: String::new(),
            quant: String::new(),
            language: "en".into(),
            speaker_file: String::new(),
        }
    }
}

/// Optional capabilities next to the always-on chat model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    Image,
    Video,
    SpeechToText,
    TextToSpeech,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::Image,
        Capability::Video,
        Capability::SpeechToText,
        Capability::TextToSpeech,
    ];

    /// Name of the yourai.toml table and lock entry.
    pub fn key(self) -> &'static str {
        match self {
            Capability::Image => "image",
            Capability::Video => "video",
            Capability::SpeechToText => "speech_to_text",
            Capability::TextToSpeech => "text_to_speech",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Capability::Image => "Image generation",
            Capability::Video => "Video generation",
            Capability::SpeechToText => "Speech to text",
            Capability::TextToSpeech => "Text to speech",
        }
    }

    pub fn kind(self) -> super::catalog::Kind {
        use super::catalog::Kind;
        match self {
            Capability::Image => Kind::Image,
            Capability::Video => Kind::Video,
            Capability::SpeechToText => Kind::Stt,
            Capability::TextToSpeech => Kind::Tts,
        }
    }

    pub fn engine(self) -> &'static str {
        use super::catalog::{LLAMA, SD, WHISPER};
        match self {
            Capability::Image | Capability::Video => SD,
            Capability::SpeechToText => WHISPER,
            Capability::TextToSpeech => LLAMA,
        }
    }
}

/// The model chosen for one capability.
#[derive(Debug, Clone, PartialEq)]
pub struct CapModel<'a> {
    pub enabled: bool,
    pub model: &'a str,
    pub quant: &'a str,
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
    /// When true (default), serve the project `ui_dir` if it has `index.html`.
    /// When false, keep using the default llama.cpp WebUI even if `web/` exists.
    #[serde(default = "default_customize_web_ui")]
    pub customize_web_ui: bool,
    /// Project-relative folder with `index.html` for the browser chat. Empty or missing
    /// falls back to llama.cpp's WebUI when `web_ui` is true.
    #[serde(default = "default_ui_dir")]
    pub ui_dir: String,
    /// Chat API dialects to expose under `/v1/` (`openai`, `claude`, `gemini`).
    #[serde(default = "default_api_styles")]
    pub api_styles: Vec<String>,
    pub require_api_key: bool,
    /// Stop image, video, and speech engines after this many idle minutes, 0 = never.
    #[serde(default = "default_idle")]
    pub idle_unload_minutes: u32,
}

fn default_idle() -> u32 {
    10
}

fn default_customize_web_ui() -> bool {
    true
}

fn default_ui_dir() -> String {
    "web".into()
}

fn default_api_styles() -> Vec<String> {
    vec!["openai".into()]
}

/// Where models and engines are installed for this project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct StoreCfg {
    /// `true` (default): install and run from `./yourai_modules/`.
    /// `false`: install and run from `~/.yourai` only (no project module folder).
    pub project_modules: bool,
}

impl Default for StoreCfg {
    fn default() -> Self {
        StoreCfg { project_modules: true }
    }
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

    pub fn has_api_style(&self, style: &str) -> bool {
        self.api_styles.iter().any(|s| s == style)
    }
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    CliError::new(Exit::InvalidConfig, msg)
        .hint("fix yourai.toml, then run `yourai config validate`")
        .into()
}

impl Config {
    /// Parses and validates. Schema 1 files load with every capability disabled.
    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let mut cfg: Config = toml::from_str(text).map_err(|e| invalid(format!("yourai.toml is invalid:\n{e}")))?;
        if cfg.schema_version == 1 {
            cfg.schema_version = SCHEMA_VERSION;
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn cap(&self, c: Capability) -> CapModel<'_> {
        let (enabled, model, quant) = match c {
            Capability::Image => (self.image.enabled, &self.image.model, &self.image.quant),
            Capability::Video => (self.video.enabled, &self.video.model, &self.video.quant),
            Capability::SpeechToText => (
                self.speech_to_text.enabled,
                &self.speech_to_text.model,
                &self.speech_to_text.quant,
            ),
            Capability::TextToSpeech => (
                self.text_to_speech.enabled,
                &self.text_to_speech.model,
                &self.text_to_speech.quant,
            ),
        };
        CapModel { enabled, model, quant }
    }

    pub fn enabled_caps(&self) -> Vec<Capability> {
        Capability::ALL.into_iter().filter(|c| self.cap(*c).enabled).collect()
    }

    /// Agent tools that are on: the configured list (or all), minus tools whose
    /// capability is off and `write_file` unless writing is allowed.
    pub fn agent_tools(&self) -> Vec<&'static str> {
        if !self.agent.enabled {
            return Vec::new();
        }
        AGENT_TOOLS
            .into_iter()
            .filter(|t| self.agent.tools.is_empty() || self.agent.tools.iter().any(|x| x == t))
            .filter(|t| match *t {
                "write_file" => self.agent.allow_write,
                "generate_image" => self.image.enabled,
                "generate_video" => self.video.enabled,
                "speak" => self.text_to_speech.enabled,
                "transcribe" => self.speech_to_text.enabled,
                _ => true,
            })
            .collect()
    }
}

/// Upgrades an older yourai.toml to the current schema, keeping the user's comments
/// and values. Returns `None` when the text is already current.
pub fn migrate(text: &str) -> anyhow::Result<Option<String>> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| invalid(format!("yourai.toml is not valid TOML:\n{e}")))?;
    if doc.get("schema_version").and_then(|v| v.as_integer()) != Some(1) {
        return Ok(None);
    }
    let cfg = Config::parse(text)?;
    doc["schema_version"] = toml_edit::value(SCHEMA_VERSION as i64);
    if let Some(server) = doc.get_mut("server").and_then(|s| s.as_table_mut()) {
        if !server.contains_key("idle_unload_minutes") {
            server.insert("idle_unload_minutes", toml_edit::value(10));
            if let Some(mut k) = server.key_mut("idle_unload_minutes") {
                k.leaf_decor_mut()
                    .set_prefix("# Stop image, video, and speech engines after this many idle minutes, 0 = never\n");
            }
        }
    }
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&render_capabilities(&cfg));
    Config::parse(&out)?;
    Ok(Some(out))
}

impl Config {
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
        if self.model.mmproj != "auto" && !self.model.mmproj.is_empty() && !self.model.mmproj.ends_with(".gguf") {
            return Err(invalid("model.mmproj must be \"auto\", \"\", or a .gguf file name"));
        }
        super::paths::inside(Path::new("."), &self.system_prompt).map_err(|e| invalid(e.to_string()))?;
        super::paths::inside(Path::new("."), &self.agent.workspace)
            .map_err(|e| invalid(format!("agent.workspace: {e}")))?;
        for t in &self.agent.tools {
            if !AGENT_TOOLS.contains(&t.as_str()) {
                return Err(invalid(format!(
                    "agent.tools: unknown tool \"{t}\" (choose from {})",
                    AGENT_TOOLS.join(", ")
                )));
            }
        }
        for c in Capability::ALL {
            let m = self.cap(c);
            if m.enabled && m.model.is_empty() {
                return Err(invalid(format!(
                    "{}.enabled is true but {}.model is empty",
                    c.key(),
                    c.key()
                )));
            }
        }
        for (key, v) in [
            ("image.width", self.image.width),
            ("image.height", self.image.height),
            ("video.width", self.video.width),
            ("video.height", self.video.height),
        ] {
            if v != 0 && (v % 16 != 0 || !(64..=2048).contains(&v)) {
                return Err(invalid(format!(
                    "{key} must be 0 (model default) or a multiple of 16 between 64 and 2048"
                )));
            }
        }
        let lang = |l: &str| l.len() <= 8 && l.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
        if !lang(&self.speech_to_text.language) || !lang(&self.text_to_speech.language) {
            return Err(invalid(
                "languages must be a code like \"en\" (or \"auto\" for speech_to_text)",
            ));
        }
        if !self.text_to_speech.speaker_file.is_empty() {
            super::paths::inside(Path::new("."), &self.text_to_speech.speaker_file)
                .map_err(|e| invalid(format!("text_to_speech.speaker_file: {e}")))?;
        }
        if !self.server.ui_dir.is_empty() {
            super::paths::inside(Path::new("."), &self.server.ui_dir)
                .map_err(|e| invalid(format!("server.ui_dir: {e}")))?;
        }
        if self.server.api_styles.is_empty() {
            return Err(invalid(
                "server.api_styles must list at least one of openai, claude, gemini",
            ));
        }
        for s in &self.server.api_styles {
            if !API_STYLES.contains(&s.as_str()) {
                return Err(invalid(format!(
                    "server.api_styles: unknown style \"{s}\" (choose from {})",
                    API_STYLES.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// Project browser UI folder when `web_ui` and `customize_web_ui` are on and
    /// `<ui_dir>/index.html` exists. Otherwise the gateway uses llama.cpp's WebUI.
    pub fn project_ui_dir(&self, root: &Path) -> Option<std::path::PathBuf> {
        if !self.server.web_ui || !self.server.customize_web_ui || self.server.ui_dir.is_empty() {
            return None;
        }
        let dir = super::paths::inside(root, &self.server.ui_dir).ok()?;
        dir.join("index.html").is_file().then_some(dir)
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
# Image/audio input for vision and omni models: "auto" (from the catalog) or "" (off)
mmproj = {mmproj}
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
# true = serve project ui_dir (edit web/). false = llama.cpp default UI (web/ may still exist).
customize_web_ui = {customize_web_ui}
# Project folder with index.html for the browser chat (edit freely). Empty = llama.cpp WebUI.
ui_dir = {ui_dir}
# Chat dialects under /v1/: openai (SSE+WS), claude (SSE), gemini (SSE). Leave room for your own /v2/.
api_styles = [{api_styles}]
require_api_key = {req_key}
# Stop image, video, and speech engines after this many idle minutes, 0 = never
idle_unload_minutes = {idle}

[store]
# true = download/run from ./yourai_modules/ (default). false = use ~/.yourai only.
project_modules = {project_modules}
{caps}"#,
        schema = cfg.schema_version,
        name = cfg.name,
        goal = cfg.goal,
        prompt = cfg.system_prompt,
        model_id = cfg.model.id,
        quant = cfg.model.quant,
        mmproj = q(&cfg.model.mmproj),
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
        customize_web_ui = cfg.server.customize_web_ui,
        ui_dir = q(&cfg.server.ui_dir),
        api_styles = cfg
            .server
            .api_styles
            .iter()
            .map(|s| q(s))
            .collect::<Vec<_>>()
            .join(", "),
        req_key = cfg.server.require_api_key,
        idle = cfg.server.idle_unload_minutes,
        project_modules = cfg.store.project_modules,
        caps = render_capabilities(cfg),
    )
}

/// The capability tables, shared by `render` and the v1 -> v2 upgrade.
fn render_capabilities(cfg: &Config) -> String {
    let tools = cfg.agent.tools.iter().map(|t| q(t)).collect::<Vec<_>>().join(", ");
    format!(
        r#"
[agent]
# Let `yourai chat` read files in the workspace and use the tools below.
enabled = {a_on}
# Folder the agent can see, relative to this project.
workspace = {a_ws}
# Allow the agent to create or change files (it always asks first).
allow_write = {a_write}
# Empty = all tools whose capability is enabled. Choose from:
# list_files, read_file, write_file, generate_image, generate_video, speak, transcribe
tools = [{tools}]

[image]
# Create images from text with `yourai image` or at /v1/images/generations.
enabled = {i_on}
model = {i_model}
quant = {i_quant}
# 0 = the model's default
width = {i_w}
height = {i_h}
steps = {i_steps}

[video]
# Create short videos from text with `yourai video`. Needs a strong GPU or 16 GB+ Apple Silicon.
enabled = {v_on}
model = {v_model}
quant = {v_quant}
# 0 = the model's default
width = {v_w}
height = {v_h}
frames = {v_frames}
fps = {v_fps}
steps = {v_steps}

[speech_to_text]
# Turn recordings into text with `yourai transcribe` or at /v1/audio/transcriptions.
enabled = {s_on}
model = {s_model}
quant = {s_quant}
# "auto" or a language code like "en"
language = {s_lang}

[text_to_speech]
# Read text aloud with `yourai speak` or at /v1/audio/speech.
enabled = {t_on}
model = {t_model}
quant = {t_quant}
language = {t_lang}
# Optional voice sample (WAV) to imitate, relative to this project
speaker_file = {t_spk}
"#,
        a_on = cfg.agent.enabled,
        a_ws = q(&cfg.agent.workspace),
        a_write = cfg.agent.allow_write,
        i_on = cfg.image.enabled,
        i_model = q(&cfg.image.model),
        i_quant = q(&cfg.image.quant),
        i_w = cfg.image.width,
        i_h = cfg.image.height,
        i_steps = cfg.image.steps,
        v_on = cfg.video.enabled,
        v_model = q(&cfg.video.model),
        v_quant = q(&cfg.video.quant),
        v_w = cfg.video.width,
        v_h = cfg.video.height,
        v_frames = cfg.video.frames,
        v_fps = cfg.video.fps,
        v_steps = cfg.video.steps,
        s_on = cfg.speech_to_text.enabled,
        s_model = q(&cfg.speech_to_text.model),
        s_quant = q(&cfg.speech_to_text.quant),
        s_lang = q(&cfg.speech_to_text.language),
        t_on = cfg.text_to_speech.enabled,
        t_model = q(&cfg.text_to_speech.model),
        t_quant = q(&cfg.text_to_speech.quant),
        t_lang = q(&cfg.text_to_speech.language),
        t_spk = q(&cfg.text_to_speech.speaker_file),
    )
}

/// A TOML string literal.
fn q(s: &str) -> String {
    toml_edit::Value::from(s).to_string().trim().to_string()
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
        "model" | "runtime" | "image" | "video" | "speech_to_text" | "text_to_speech" => {
            "Engine or model change: run `yourai rebuild` (may download files)."
        }
        "store" => "Store location change: run `yourai rebuild` (may download or copy files).",
        "agent" => "Used the next time you run `yourai chat`.",
        "generation" | "server" | "system_prompt" | "name" | "goal" => {
            "Restart needed: run `yourai rebuild` (fast, nothing is downloaded)."
        }
        _ => "Run `yourai rebuild` to apply.",
    }
}

#[cfg(test)]
pub fn sample() -> Config {
    Config {
        schema_version: SCHEMA_VERSION,
        name: "my-ai".into(),
        goal: "chat".into(),
        system_prompt: "prompts/system.md".into(),
        model: ModelCfg {
            id: "qwen2.5-0.5b".into(),
            quant: "Q4_K_M".into(),
            repo: String::new(),
            file: String::new(),
            mmproj: "auto".into(),
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
            customize_web_ui: true,
            ui_dir: "web".into(),
            api_styles: default_api_styles(),
            require_api_key: false,
            idle_unload_minutes: 10,
        },
        store: StoreCfg::default(),
        agent: AgentCfg::default(),
        image: ImageCfg::default(),
        video: VideoCfg::default(),
        speech_to_text: SttCfg::default(),
        text_to_speech: TtsCfg::default(),
    }
}

/// A schema 1 file as written by yourai 0.0.1.
#[cfg(test)]
pub const V1_SAMPLE: &str = r#"# YourAI project configuration.
schema_version = 1
name = "old-ai"
goal = "chat"
system_prompt = "prompts/system.md"

[model]
# my favourite model
id = "qwen2.5-0.5b"
quant = "Q4_K_M"

[runtime]
engine = "llamacpp"
gpu_layers = "auto"
context_length = 4096
threads = 0

[generation]
temperature = 0.7
top_p = 0.9
max_tokens = 1024

[server]
host = "127.0.0.1"
port = 8080
web_ui = true
require_api_key = false
"#;

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
    fn render_roundtrips_with_capabilities() {
        let mut cfg = sample();
        cfg.agent.enabled = true;
        cfg.agent.tools = vec!["read_file".into(), "speak".into()];
        cfg.image = ImageCfg {
            enabled: true,
            model: "sd-turbo".into(),
            quant: "Q8_0".into(),
            width: 512,
            height: 512,
            steps: 2,
        };
        cfg.text_to_speech.enabled = true;
        cfg.text_to_speech.model = "qwen3-tts-1.7b".into();
        cfg.text_to_speech.speaker_file = "voice \"me\".wav".into();
        let parsed = Config::parse(&render(&cfg)).unwrap();
        assert_eq!(parsed, cfg);
        assert_eq!(parsed.agent_tools(), vec!["read_file", "speak"]);
        assert_eq!(parsed.enabled_caps(), vec![Capability::Image, Capability::TextToSpeech]);
    }

    #[test]
    fn v1_loads_with_capabilities_off() {
        let cfg = Config::parse(V1_SAMPLE).unwrap();
        assert_eq!(cfg.schema_version, SCHEMA_VERSION);
        assert_eq!(cfg.model.mmproj, "auto");
        assert_eq!(cfg.server.idle_unload_minutes, 10);
        assert!(cfg.enabled_caps().is_empty());
        assert!(cfg.agent_tools().is_empty());
    }

    #[test]
    fn v1_migrates_in_place() {
        let out = migrate(V1_SAMPLE).unwrap().expect("v1 needs upgrading");
        assert!(out.contains("# my favourite model"));
        assert!(out.contains("schema_version = 2"));
        assert!(out.contains("idle_unload_minutes = 10"));
        assert!(out.contains("[text_to_speech]"));
        let cfg = Config::parse(&out).unwrap();
        assert_eq!(cfg.name, "old-ai");
        assert!(migrate(&out).unwrap().is_none());
        assert!(migrate(&render(&sample())).unwrap().is_none());
    }

    #[test]
    fn capability_checks() {
        let mut cfg = sample();
        cfg.image.enabled = true;
        assert!(cfg.validate().is_err(), "enabled without a model");
        cfg.image.model = "sd-turbo".into();
        assert!(cfg.validate().is_ok());
        cfg.image.width = 500;
        assert!(cfg.validate().is_err());
        let mut cfg = sample();
        cfg.agent.workspace = "../outside".into();
        assert!(cfg.validate().is_err());
        let mut cfg = sample();
        cfg.agent.tools = vec!["rm_rf".into()];
        assert!(cfg.validate().is_err());
        let mut cfg = sample();
        cfg.agent.enabled = true;
        assert!(!cfg.agent_tools().contains(&"write_file"));
        assert!(!cfg.agent_tools().contains(&"generate_image"));
        cfg.agent.allow_write = true;
        assert!(cfg.agent_tools().contains(&"write_file"));
    }

    #[test]
    fn rendered_config_has_no_secrets() {
        let mut cfg = sample();
        cfg.server.require_api_key = true;
        let text = render(&cfg);
        assert!(!text.lines().any(|l| l.trim_start().starts_with("api_key")));
    }

    #[test]
    fn customize_web_ui_gates_project_ui_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/index.html"), "<html></html>").unwrap();

        let mut cfg = sample();
        assert_eq!(cfg.project_ui_dir(root).as_deref(), Some(root.join("web").as_path()));

        cfg.server.customize_web_ui = false;
        assert!(cfg.project_ui_dir(root).is_none());

        cfg.server.customize_web_ui = true;
        cfg.server.web_ui = false;
        assert!(cfg.project_ui_dir(root).is_none());
    }

    #[test]
    fn customize_web_ui_defaults_true_when_missing() {
        let text = render(&sample()).replace("customize_web_ui = true\n", "");
        let cfg = Config::parse(&text).unwrap();
        assert!(cfg.server.customize_web_ui);
    }
}

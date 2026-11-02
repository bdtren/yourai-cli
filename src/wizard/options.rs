use crate::core::catalog::Catalog;
use crate::core::hardware::{self, Hardware};

/// One choice in a question.
#[derive(Debug, Clone)]
pub struct Opt {
    pub value: String,
    pub label: String,
    pub explain: String,
    /// Some(reason) = cannot be chosen on this machine; listed with the reason.
    pub unavailable: Option<String>,
}

impl Opt {
    pub fn new(value: impl Into<String>, label: impl Into<String>, explain: impl Into<String>) -> Self {
        Opt {
            value: value.into(),
            label: label.into(),
            explain: explain.into(),
            unavailable: None,
        }
    }

    pub fn unavailable_if(mut self, reason: Option<String>) -> Self {
        self.unavailable = reason;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Question {
    /// Some(pre-selected option indexes) for a multi-select question.
    pub multi: Option<Vec<usize>>,
    pub key: &'static str,
    pub title: String,
    pub why: String,
    pub options: Vec<Opt>,
    /// Index into `options` of the recommended (and pre-selected) option.
    pub recommended: usize,
    /// Shown under the options, e.g. "Recommended because it fits your 8 GB GPU".
    pub reason: String,
}

impl Question {
    pub fn available(&self) -> impl Iterator<Item = &Opt> {
        self.options.iter().filter(|o| o.unavailable.is_none())
    }

    pub fn recommended_opt(&self) -> &Opt {
        &self.options[self.recommended]
    }

    pub fn find(&self, value: &str) -> Option<&Opt> {
        self.options.iter().find(|o| o.value.eq_ignore_ascii_case(value))
    }

    /// The recommended answer as stored in `Answers`: one value, or a comma list
    /// ("none" when nothing is selected) for multi-select questions.
    pub fn recommended_value(&self) -> String {
        if let Some(sel) = &self.multi {
            let v: Vec<&str> = sel.iter().map(|i| self.options[*i].value.as_str()).collect();
            if v.is_empty() {
                "none".into()
            } else {
                v.join(",")
            }
        } else {
            self.recommended_opt().value.clone()
        }
    }
}

/// Everything the wizard knows so far.
#[derive(Debug, Clone, Default)]
pub struct Answers {
    pub vram: Option<String>,
    pub goal: String,
    /// Comma list of capability values, or "none".
    pub capabilities: String,
    pub model: String,
    pub custom_repo: String,
    pub custom_file: String,
    pub quant: String,
    pub runtime: String,
    pub ctx: String,
    pub image_model: String,
    pub video_model: String,
    pub stt_model: String,
    pub tts_model: String,
    pub interface: String,
    /// "true" | "false" — use project `web/` vs default llama UI.
    pub customize_web_ui: String,
    /// Comma list of API styles: openai, claude, gemini.
    pub api_styles: String,
    pub access: String,
    pub port: String,
}

impl Answers {
    pub fn get(&self, key: &str) -> &str {
        match key {
            "vram" => self.vram.as_deref().unwrap_or(""),
            "goal" => &self.goal,
            "capabilities" => &self.capabilities,
            "model" => &self.model,
            "quant" => &self.quant,
            "runtime" => &self.runtime,
            "ctx" => &self.ctx,
            "image_model" => &self.image_model,
            "video_model" => &self.video_model,
            "stt_model" => &self.stt_model,
            "tts_model" => &self.tts_model,
            "interface" => &self.interface,
            "customize_web_ui" => &self.customize_web_ui,
            "api_styles" => &self.api_styles,
            "access" => &self.access,
            "port" => &self.port,
            _ => "",
        }
    }

    pub fn set(&mut self, key: &str, v: String) {
        match key {
            "vram" => self.vram = Some(v),
            "goal" => self.goal = v,
            "capabilities" => self.capabilities = v,
            "model" => self.model = v,
            "quant" => self.quant = v,
            "runtime" => self.runtime = v,
            "ctx" => self.ctx = v,
            "image_model" => self.image_model = v,
            "video_model" => self.video_model = v,
            "stt_model" => self.stt_model = v,
            "tts_model" => self.tts_model = v,
            "interface" => self.interface = v,
            "customize_web_ui" => self.customize_web_ui = v,
            "api_styles" => self.api_styles = v,
            "access" => self.access = v,
            "port" => self.port = v,
            _ => {}
        }
    }

    pub fn api_style_list(&self) -> Vec<String> {
        if self.api_styles.is_empty() || self.api_styles == "none" {
            return vec!["openai".into()];
        }
        self.api_styles
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "none")
            .map(str::to_string)
            .collect()
    }

    pub fn has_cap(&self, cap: &str) -> bool {
        self.capabilities.split(',').any(|c| c.trim() == cap)
    }

    pub fn ctx_tokens(&self) -> u32 {
        self.ctx.parse().unwrap_or(crate::core::recommender::DEFAULT_CTX)
    }
}

pub struct Ctx<'a> {
    pub hw: &'a Hardware,
    pub catalog: &'a Catalog,
    pub answers: &'a Answers,
}

impl Ctx<'_> {
    pub fn budget_gb(&self) -> f64 {
        hardware::budget_gb(self.hw)
    }

    pub fn cpu_only(&self) -> bool {
        hardware::cpu_only(self.hw)
    }

    pub fn memory_word(&self) -> &'static str {
        if self.cpu_only() {
            "RAM for models"
        } else {
            "GPU memory"
        }
    }
}

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
}

/// Everything the wizard knows so far.
#[derive(Debug, Clone, Default)]
pub struct Answers {
    pub vram: Option<String>,
    pub goal: String,
    pub model: String,
    pub custom_repo: String,
    pub custom_file: String,
    pub quant: String,
    pub runtime: String,
    pub ctx: String,
    pub interface: String,
    pub features: String,
    pub access: String,
    pub port: String,
}

impl Answers {
    pub fn get(&self, key: &str) -> &str {
        match key {
            "vram" => self.vram.as_deref().unwrap_or(""),
            "goal" => &self.goal,
            "model" => &self.model,
            "quant" => &self.quant,
            "runtime" => &self.runtime,
            "ctx" => &self.ctx,
            "interface" => &self.interface,
            "features" => &self.features,
            "access" => &self.access,
            "port" => &self.port,
            _ => "",
        }
    }

    pub fn set(&mut self, key: &str, v: String) {
        match key {
            "vram" => self.vram = Some(v),
            "goal" => self.goal = v,
            "model" => self.model = v,
            "quant" => self.quant = v,
            "runtime" => self.runtime = v,
            "ctx" => self.ctx = v,
            "interface" => self.interface = v,
            "features" => self.features = v,
            "access" => self.access = v,
            "port" => self.port = v,
            _ => {}
        }
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

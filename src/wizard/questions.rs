use super::options::{Ctx, Opt, Question};
use crate::core::catalog::{Kind, Model};
use crate::core::config::Capability;
use crate::core::engine;
use crate::core::hardware::GpuVendor;
use crate::core::process;
use crate::core::recommender::{self, DEFAULT_CTX};

/// Questions in the order they are asked. Builders return None when a question does not apply.
pub const ORDER: &[&str] = &[
    "vram",
    "goal",
    "capabilities",
    "model",
    "quant",
    "runtime",
    "ctx",
    "image_model",
    "video_model",
    "stt_model",
    "tts_model",
    "interface",
    "api_styles",
    "access",
    "port",
];

/// Capability answer values -> the yourai.toml capability they turn on.
pub const CAP_VALUES: [(&str, Capability); 4] = [
    ("image", Capability::Image),
    ("video", Capability::Video),
    ("stt", Capability::SpeechToText),
    ("tts", Capability::TextToSpeech),
];

/// The wizard question that picks the model for a capability.
pub fn cap_question(cap: Capability) -> &'static str {
    match cap {
        Capability::Image => "image_model",
        Capability::Video => "video_model",
        Capability::SpeechToText => "stt_model",
        Capability::TextToSpeech => "tts_model",
    }
}

pub fn build(key: &str, c: &Ctx) -> anyhow::Result<Option<Question>> {
    Ok(match key {
        "vram" => vram(c),
        "goal" => Some(goal()),
        "capabilities" => Some(capabilities(c)),
        "model" => Some(model(c)?),
        "quant" => quant(c),
        "runtime" => Some(runtime(c)),
        "ctx" => Some(ctx(c)),
        "image_model" => cap_model(c, Capability::Image),
        "video_model" => cap_model(c, Capability::Video),
        "stt_model" => cap_model(c, Capability::SpeechToText),
        "tts_model" => cap_model(c, Capability::TextToSpeech),
        "interface" => Some(interface(c)),
        "api_styles" => Some(api_styles()),
        "access" => Some(access()),
        "port" => Some(port(c)),
        _ => None,
    })
}

fn pick(options: &[Opt], value: &str) -> usize {
    options
        .iter()
        .position(|o| o.value == value && o.unavailable.is_none())
        .unwrap_or_else(|| options.iter().position(|o| o.unavailable.is_none()).unwrap_or(0))
}

fn vram(c: &Ctx) -> Option<Question> {
    let needs = matches!(c.hw.gpu_vendor, GpuVendor::Amd | GpuVendor::Intel) && c.hw.vram_gb.is_none();
    if !needs && c.answers.vram.is_none() {
        return None;
    }
    let mut options = vec![Opt::new(
        "unknown",
        "Not sure",
        "Plan for running on the CPU with system RAM. Always works, just slower.",
    )];
    for gb in ["4", "6", "8", "12", "16", "24", "48"] {
        options.push(Opt::new(
            gb,
            format!("{gb} GB"),
            format!("My GPU has {gb} GB of dedicated memory."),
        ));
    }
    Some(Question {
        multi: None,
        key: "vram",
        title: "How much memory does your GPU have?".into(),
        why: format!(
            "We found a {} GPU ({}) but could not read its memory. This decides which models fit.",
            c.hw.gpu_vendor.label(),
            c.hw.gpu_name
        ),
        options,
        recommended: 0,
        reason: "Recommended when you are not sure: a wrong guess can make the model fail to load.".into(),
    })
}

fn goal() -> Question {
    let options = vec![
        Opt::new(
            "chat",
            "Chat assistant",
            "A general assistant you talk to in the terminal, a browser, or via an API. Simplest setup and lowest hardware needs.",
        ),
        Opt::new(
            "code",
            "Coding assistant",
            "Uses a model tuned for code with a longer context. Better for programming help.",
        ),
    ];
    Question {
        multi: None,
        key: "goal",
        title: "What do you want to do?".into(),
        why: "Your goal decides which models and settings we suggest.".into(),
        options,
        recommended: 0,
        reason: "Recommended for first-time users: it works on almost any computer.".into(),
    }
}

/// Why no model for `kind` can run here, or None.
fn kind_blocker(c: &Ctx, kind: Kind) -> Option<String> {
    let budget = c.budget_gb();
    let models: Vec<&Model> = c.catalog.models.iter().filter(|m| m.kind == kind).collect();
    if models.is_empty() {
        return Some("no model in the catalog yet; run `yourai update`".into());
    }
    if models
        .iter()
        .any(|m| recommender::model_blocker(c.hw, budget, m).is_none())
    {
        return None;
    }
    let smallest = models.iter().min_by(|a, b| a.params_b.total_cmp(&b.params_b)).unwrap();
    recommender::model_blocker(c.hw, budget, smallest)
}

fn engine_blocker(c: &Ctx, spec: &engine::EngineSpec) -> Option<String> {
    (!engine::available(c.hw, c.catalog, spec)).then(|| format!("{} has no build for this computer yet", spec.label))
}

fn capabilities(c: &Ctx) -> Question {
    let budget = c.budget_gb();
    let cpu = c.cpu_only();
    let vision_block = kind_blocker(c, Kind::Vision);
    let defs: [(&str, &str, &str, Option<String>, bool); 6] = [
        (
            "agent",
            "Read files (agent)",
            "`yourai chat` can list and read files in the project (text, code, PDF) and use the tools you turn on below. Writing files is off until you allow it.",
            None,
            true,
        ),
        (
            "vision",
            "See images and hear audio",
            "Picks a chat model that understands pictures (and, with omni models, audio). Attach files with /image and /audio in `yourai chat`.",
            vision_block.clone(),
            budget >= 8.0 && vision_block.is_none(),
        ),
        (
            "image",
            "Create images",
            "Make pictures from a text description. Runs only while you use it.",
            engine_blocker(c, &engine::SD).or_else(|| kind_blocker(c, Kind::Image)),
            !cpu && budget >= 8.0,
        ),
        (
            "stt",
            "Speech to text",
            "Turn recordings into text with Whisper (many languages).",
            engine_blocker(c, &engine::WHISPER).or_else(|| kind_blocker(c, Kind::Stt)),
            budget >= 6.0,
        ),
        (
            "tts",
            "Text to speech",
            "Read answers aloud and save them as WAV files.",
            kind_blocker(c, Kind::Tts),
            !cpu && budget >= 8.0,
        ),
        (
            "video",
            "Create short videos",
            "A few seconds of video from a text description. Slow: minutes per clip even on a good GPU.",
            engine_blocker(c, &engine::SD).or_else(|| kind_blocker(c, Kind::Video)),
            !cpu && budget >= 20.0,
        ),
    ];
    let mut options = Vec::new();
    let mut selected = Vec::new();
    for (i, (value, label, explain, blocker, recommend)) in defs.into_iter().enumerate() {
        if recommend && blocker.is_none() {
            selected.push(i);
        }
        options.push(Opt::new(value, label, explain).unavailable_if(blocker));
    }
    let reason = if cpu {
        "Recommended for a computer without a usable GPU: the light capabilities that stay fast on the CPU.".to_string()
    } else {
        format!(
            "Recommended for {budget:.0} GB of {}: everything that runs comfortably on it.",
            c.memory_word()
        )
    };
    Question {
        multi: Some(selected),
        key: "capabilities",
        title: "What else should it be able to do?".into(),
        why: "Pick any. Each one adds a download; image, video, and speech engines only load while you use them. Change them later in yourai.toml.".into(),
        options,
        recommended: 0,
        reason,
    }
}

fn cap_model(c: &Ctx, cap: Capability) -> Option<Question> {
    let value = CAP_VALUES.iter().find(|(_, k)| *k == cap)?.0;
    if !c.answers.has_cap(value) {
        return None;
    }
    let budget = c.budget_gb();
    let chat_gb = c
        .catalog
        .model(&c.answers.model)
        .and_then(|m| m.memory_gb(&c.answers.quant, c.answers.ctx_tokens()))
        .unwrap_or(0.0);
    let mut models: Vec<&Model> = c.catalog.models_of(cap.kind()).collect();
    models.sort_by(|a, b| a.params_b.total_cmp(&b.params_b));
    let rec = recommender::recommend_cap_model(c.hw, c.catalog, cap, budget, budget - chat_gb);
    let options: Vec<Opt> = models
        .iter()
        .map(|m| {
            let q = m.default_quant();
            let dl = m.download_gb(q).unwrap_or(0.0);
            let mem = m.memory_gb(q, 0).unwrap_or(0.0);
            Opt::new(
                m.id.clone(),
                format!("{} ({:.1} GB)", m.name, dl),
                format!("Uses about {mem:.1} GB while working. License: {}.", m.license),
            )
            .unavailable_if(recommender::model_blocker(c.hw, budget, m))
        })
        .collect();
    let recommended = rec.map(|m| pick(&options, &m.id)).unwrap_or(0);
    let reason = match rec {
        Some(m) if m.memory_gb(m.default_quant(), 0).unwrap_or(0.0) + chat_gb <= budget => format!(
            "Recommended because it fits next to the chat model in your {budget:.1} GB of {}.",
            c.memory_word()
        ),
        Some(_) => "Recommended because it is the lightest choice; the chat model may slow down while it runs.".into(),
        None => "No model fits this computer.".into(),
    };
    let (title, why) = match cap {
        Capability::Image => (
            "Which image model?",
            "Bigger models draw better pictures but take longer.",
        ),
        Capability::Video => (
            "Which video model?",
            "Video models are large; a short clip takes minutes.",
        ),
        Capability::SpeechToText => (
            "Which speech-to-text model?",
            "Bigger Whisper models make fewer mistakes but are slower.",
        ),
        Capability::TextToSpeech => ("Which voice model?", "The model that reads text aloud."),
    };
    Some(Question {
        multi: None,
        key: cap_question(cap),
        title: title.into(),
        why: why.into(),
        options,
        recommended,
        reason,
    })
}

fn tier_explain(size: &str) -> &'static str {
    match size {
        "Small" => "Fast and light, runs on almost any laptop. Good for simple Q&A, weaker at complex reasoning.",
        "Medium" => "The best balance of quality and speed for most people.",
        "Large" => "Noticeably smarter answers, needs a mid-range GPU or a Mac with 16 GB+.",
        _ => "Near top-tier local quality. Needs a high-end GPU or a Mac with lots of memory.",
    }
}

fn model(c: &Ctx) -> anyhow::Result<Question> {
    let goal = if c.answers.goal.is_empty() {
        "chat"
    } else {
        c.answers.goal.as_str()
    };
    let budget = c.budget_gb();
    let vision = c.answers.has_cap("vision");
    let mut models = recommender::chat_models(c.catalog, goal, vision);
    models.sort_by(|a, b| a.params_b.total_cmp(&b.params_b));
    let rec = recommender::recommend_model(c.hw, c.catalog, goal, budget, vision).ok_or_else(|| {
        crate::error::CliError::new(
            crate::error::Exit::Resources,
            format!(
                "no model in the catalog fits in about {budget:.1} GB of {}",
                c.memory_word()
            ),
        )
        .hint("close other apps and try again, or use `--vram` if your GPU memory was not detected")
    })?;
    let mut options: Vec<Opt> = models
        .iter()
        .map(|m| {
            let q4 = m.quants.get("Q4_K_M").map(|q| q.size_gb).unwrap_or(0.0);
            Opt::new(
                m.id.clone(),
                format!("{} ({}, {:.1} GB)", m.name, m.size_label(), q4),
                format!("{} License: {}.", tier_explain(m.size_label()), m.license),
            )
            .unavailable_if(recommender::model_blocker(c.hw, budget, m))
        })
        .collect();
    options.push(Opt::new(
        "custom",
        "Custom (Hugging Face GGUF)",
        "Enter any GGUF model yourself. For advanced users; no published checksum.",
    ));
    let recommended = pick(&options, &rec.id);
    let mut reason = format!(
        "Recommended because it is the most capable model that fits in your {:.1} GB of {}.",
        budget,
        c.memory_word()
    );
    if c.cpu_only() {
        reason.push_str(" Without a usable GPU, bigger models would fit but answer too slowly.");
    }
    Ok(Question {
        multi: None,
        key: "model",
        title: "Which model?".into(),
        why: "The model is the AI's brain. Bigger models answer better but need more memory and are slower.".into(),
        options,
        recommended,
        reason,
    })
}

fn quant(c: &Ctx) -> Option<Question> {
    let m = c.catalog.model(&c.answers.model)?;
    let budget = c.budget_gb();
    let ctx = DEFAULT_CTX;
    let fit = |q: &str| -> Option<String> {
        let need = m.memory_gb(q, ctx)?;
        (need > budget).then(|| {
            format!(
                "needs about {need:.1} GB, you have about {budget:.1} GB of {}",
                c.memory_word()
            )
        })
    };
    let defs = [
        (
            "Q4_K_M",
            "Best balance of size and quality. Small quality loss, big memory savings.",
        ),
        (
            "Q5_K_M",
            "A bit larger and a bit more accurate. Good if you have spare memory.",
        ),
        ("Q8_0", "Near original quality, about twice the size of Q4."),
        (
            "Q3_K_M",
            "Very small, noticeably lower quality. Only for very tight memory.",
        ),
    ];
    let options: Vec<Opt> = defs
        .iter()
        .filter_map(|(q, explain)| {
            let f = m.quants.get(*q)?;
            Some(Opt::new(*q, *q, format!("{explain} ({:.1} GB download)", f.size_gb)).unavailable_if(fit(q)))
        })
        .collect();
    let rec = recommender::recommend_quant(m, budget, ctx);
    let recommended = pick(&options, rec);
    let reason = match rec {
        "Q5_K_M" => format!(
            "Recommended because {} fits with plenty of room to spare in {budget:.1} GB.",
            m.name
        ),
        "Q3_K_M" => "Recommended because it is the only size that fits; expect lower quality.".into(),
        _ => format!(
            "Recommended because it fits comfortably in your {budget:.1} GB of {}.",
            c.memory_word()
        ),
    };
    Some(Question {
        multi: None,
        key: "quant",
        title: "Which quantization? (compression level)".into(),
        why: "Quantization shrinks the model so it fits in memory. Lower numbers are smaller and faster but slightly less accurate.".into(),
        options,
        recommended,
        reason,
    })
}

fn runtime(c: &Ctx) -> Question {
    let ollama_reason = if c.hw.ollama {
        "Ollama is installed, but yourai cannot manage it yet (planned)".to_string()
    } else {
        "Ollama is not installed; support is planned".to_string()
    };
    let vllm_reason = if c.hw.os_family == "linux" && c.hw.gpu_vendor == GpuVendor::Nvidia {
        "planned for a later version".to_string()
    } else {
        "needs an NVIDIA GPU on Linux, and is planned for a later version".to_string()
    };
    Question {
        multi: None,
        key: "runtime",
        title: "Which runtime? (the engine that runs the model)".into(),
        why: "The runtime loads the model and serves answers.".into(),
        options: vec![
            Opt::new(
                "llamacpp",
                "llama.cpp",
                "Built-in engine, works on CPU, NVIDIA, AMD, Intel, and Apple. Downloaded automatically.",
            ),
            Opt::new("ollama", "Ollama", "Use an existing Ollama install.").unavailable_if(Some(ollama_reason)),
            Opt::new("vllm", "vLLM", "High-throughput serving for many users at once.")
                .unavailable_if(Some(vllm_reason)),
        ],
        recommended: 0,
        reason: "Recommended because it supports your hardware with no extra installs.".into(),
    }
}

fn ctx(c: &Ctx) -> Question {
    let budget = c.budget_gb();
    let m = c.catalog.model(&c.answers.model);
    let quant = if c.answers.quant.is_empty() {
        "Q4_K_M"
    } else {
        c.answers.quant.as_str()
    };
    let defs: [(u32, &str); 5] = [
        (2048, "Short chats only. Lowest memory use."),
        (4096, "Good for normal conversations."),
        (
            8192,
            "Longer chats and medium documents or code files. Uses noticeably more memory.",
        ),
        (16384, "Large documents. Heavy on memory and slower."),
        (32768, "Very large inputs. Only with lots of memory to spare."),
    ];
    let options: Vec<Opt> = defs
        .iter()
        .filter(|(n, _)| m.is_none_or(|m| *n <= m.context_max))
        .map(|(n, explain)| {
            let reason = m.and_then(|m| {
                let need = m.memory_gb(quant, *n)?;
                (need > budget).then(|| format!("needs about {need:.1} GB with this model, you have {budget:.1} GB"))
            });
            let est = m
                .and_then(|m| m.memory_gb(quant, *n))
                .map(|g| format!(" (about {g:.1} GB total)"))
                .unwrap_or_default();
            Opt::new(n.to_string(), format!("{n} tokens"), format!("{explain}{est}")).unavailable_if(reason)
        })
        .collect();
    let wanted = if c.answers.goal == "code" { "8192" } else { "4096" };
    let mut recommended = pick(&options, wanted);
    if options[recommended].value != wanted {
        recommended = pick(&options, "4096");
    }
    let reason = match options[recommended].value.as_str() {
        "8192" => "Recommended for coding: room for whole files, and it fits your memory.".to_string(),
        "4096" => "Recommended for everyday chat; leaves memory for the model.".to_string(),
        _ => "Recommended because memory is tight with this model.".to_string(),
    };
    Question {
        multi: None,
        key: "ctx",
        title: "Context length (how much text the AI can remember at once)".into(),
        why: "Longer context lets the AI handle bigger documents and longer chats, but uses more memory.".into(),
        options,
        recommended,
        reason,
    }
}

fn interface(c: &Ctx) -> Question {
    let options = vec![
        Opt::new(
            "webui",
            "API + browser chat page",
            "A local OpenAI-compatible API plus a chat page in your browser. Friendliest option. `yourai chat` works too.",
        ),
        Opt::new(
            "api",
            "API only",
            "Only the OpenAI-compatible API for your apps and tools, plus `yourai chat` in the terminal. No web page.",
        ),
    ];
    let code = c.answers.goal == "code";
    Question {
        multi: None,
        key: "interface",
        title: "How will you use it?".into(),
        why: "Both options only listen on this computer unless you change network access next.".into(),
        recommended: if code { 1 } else { 0 },
        reason: if code {
            "Recommended for coding: connect your editor or tools to the API.".into()
        } else {
            "Recommended for most people: chat in the browser, no setup.".into()
        },
        options,
    }
}

fn api_styles() -> Question {
    let options = vec![
        Opt::new(
            "openai",
            "GPT (OpenAI) — SSE + WebSocket",
            "OpenAI Chat Completions under /v1/chat/completions (SSE) and /v1/chat/ws (WebSocket). Scaffold api/openai/ and api/realtime/.",
        ),
        Opt::new(
            "claude",
            "Claude (Anthropic) — SSE",
            "Anthropic Messages API under /v1/messages with SSE streaming. Scaffold api/claude/.",
        ),
        Opt::new(
            "gemini",
            "Gemini — SSE",
            "Gemini generateContent under /v1/models/… with SSE streaming. Scaffold api/gemini/.",
        ),
    ];
    Question {
        multi: Some(vec![0]),
        key: "api_styles",
        title: "Which API styles should this project speak?".into(),
        why: "All styles use the /v1/ prefix so you can add your own /v2/ later. Pick any combination.".into(),
        options,
        recommended: 0,
        reason: "Recommended: OpenAI is the most widely supported client format.".into(),
    }
}

fn access() -> Question {
    Question {
        multi: None,
        key: "access",
        title: "Who can connect to the server? (network access)".into(),
        why: "Controls whether other devices can reach your AI.".into(),
        options: vec![
            Opt::new(
                "local",
                "Only this computer (127.0.0.1)",
                "Safest. Nothing else on your network can reach it.",
            ),
            Opt::new(
                "lan",
                "Other devices on my network (0.0.0.0)",
                "Lets your phone or other computers connect. An API key is generated and required.",
            ),
            Opt::new(
                "remote",
                "Deploy to remote",
                "After creating the project, walk through running on a VPS or exposing this PC over an IP.",
            ),
        ],
        recommended: 0,
        reason: "Recommended always: the secure default. You can change it later.".into(),
    }
}

fn port(c: &Ctx) -> Question {
    let host = if c.answers.access == "lan" {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let free = process::port_free(host, 8080);
    let alt = process::next_free_port(host, 8081).unwrap_or(8081);
    let mut options = vec![Opt::new("8080", "8080", "The usual default port.")
        .unavailable_if((!free).then(|| "8080 is already used by another program".to_string()))];
    if !free {
        options.push(Opt::new(
            alt.to_string(),
            alt.to_string(),
            "The next free port on this computer.",
        ));
    }
    options.push(Opt::new(
        "custom",
        "Another port",
        "Type a port number yourself (1024-65535).",
    ));
    let recommended = if free { 0 } else { 1 };
    Question {
        multi: None,
        key: "port",
        title: "Port".into(),
        why: "The local address your AI listens on, like http://127.0.0.1:8080.".into(),
        options,
        recommended,
        reason: if free {
            "Recommended because 8080 is free.".into()
        } else {
            format!("Recommended because 8080 is already used by another program; {alt} is free.")
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::catalog::Catalog;
    use crate::core::hardware::fixtures;
    use crate::wizard::options::Answers;

    /// For every hardware profile and question: exactly one recommended option,
    /// and it is never an unavailable one.
    #[test]
    fn recommendations_are_valid_for_all_profiles() {
        let catalog = Catalog::bundled();
        for (name, hw) in fixtures::all() {
            for goal in ["chat", "code"] {
                let mut answers = Answers {
                    goal: goal.into(),
                    ..Default::default()
                };
                for key in ORDER {
                    let c = Ctx {
                        hw: &hw,
                        catalog: &catalog,
                        answers: &answers,
                    };
                    let Some(q) = build(key, &c).unwrap() else { continue };
                    let rec = q.recommended_opt();
                    assert!(
                        rec.unavailable.is_none() || q.multi.is_some(),
                        "{name}/{goal}/{key}: recommended `{}` is unavailable",
                        rec.value
                    );
                    for i in q.multi.iter().flatten() {
                        assert!(q.options[*i].unavailable.is_none(), "{name}/{goal}/{key}: {i}");
                    }
                    assert!(q.recommended < q.options.len());
                    answers.set(key, q.recommended_value());
                }
            }
        }
    }

    #[test]
    fn access_includes_remote_deploy_option() {
        let q = access();
        assert!(q.find("local").is_some());
        assert!(q.find("lan").is_some());
        assert!(q.find("remote").is_some());
        assert_eq!(q.find("remote").unwrap().label, "Deploy to remote");
    }

    #[test]
    fn big_quant_unavailable_on_small_gpu() {
        let catalog = Catalog::bundled();
        let hw = fixtures::nvidia_8gb();
        let answers = Answers {
            goal: "chat".into(),
            model: "llama3.1-8b".into(),
            ..Default::default()
        };
        let c = Ctx {
            hw: &hw,
            catalog: &catalog,
            answers: &answers,
        };
        let q = build("quant", &c).unwrap().unwrap();
        assert!(q.find("Q8_0").unwrap().unavailable.is_some());
        assert!(q.find("Q4_K_M").unwrap().unavailable.is_none());
    }
}

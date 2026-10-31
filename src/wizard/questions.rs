use super::options::{Ctx, Opt, Question};
use crate::core::hardware::GpuVendor;
use crate::core::process;
use crate::core::recommender::{self, DEFAULT_CTX};

/// Questions in the order they are asked. Builders return None when a question does not apply.
pub const ORDER: &[&str] = &[
    "vram",
    "goal",
    "model",
    "quant",
    "runtime",
    "ctx",
    "interface",
    "features",
    "access",
    "port",
];

pub fn build(key: &str, c: &Ctx) -> anyhow::Result<Option<Question>> {
    Ok(match key {
        "vram" => vram(c),
        "goal" => Some(goal(c)),
        "model" => Some(model(c)?),
        "quant" => quant(c),
        "runtime" => Some(runtime(c)),
        "ctx" => Some(ctx(c)),
        "interface" => Some(interface(c)),
        "features" => Some(features(c)),
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

fn goal(c: &Ctx) -> Question {
    let ft_reason = if c.cpu_only() || c.budget_gb() < 12.0 {
        format!(
            "needs 12 GB or more of GPU memory, you have {:.0} GB; also coming in a later version",
            c.budget_gb()
        )
    } else {
        "coming in a later version of yourai".into()
    };
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
        Opt::new(
            "rag",
            "Chat over my documents (RAG)",
            "Answers using your own files without retraining.",
        )
        .unavailable_if(Some("coming in a later version of yourai".into())),
        Opt::new("finetune", "Customize with fine-tuning", "Teaches the model your style with your own examples.")
            .unavailable_if(Some(ft_reason)),
    ];
    Question {
        key: "goal",
        title: "What do you want to do?".into(),
        why: "Your goal decides which models and settings we suggest.".into(),
        options,
        recommended: 0,
        reason: "Recommended for first-time users: it works on almost any computer.".into(),
    }
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
    let mut models: Vec<_> = c
        .catalog
        .models
        .iter()
        .filter(|m| m.tags.iter().any(|t| t == goal))
        .collect();
    models.sort_by(|a, b| a.params_b.total_cmp(&b.params_b));
    let rec = recommender::recommend_model(c.hw, c.catalog, goal, budget).ok_or_else(|| {
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

fn features(c: &Ctx) -> Question {
    let ft = if c.cpu_only() || c.budget_gb() < 12.0 {
        format!(
            "needs 12 GB+ of GPU memory (you have {:.0} GB) and is coming later",
            c.budget_gb()
        )
    } else {
        "coming in a later version".into()
    };
    Question {
        key: "features",
        title: "Extra features".into(),
        why: "Optional add-ons. You can turn them on later in yourai.toml when they ship.".into(),
        options: vec![
            Opt::new(
                "none",
                "No extras for now",
                "Keep it simple. Nothing extra is downloaded.",
            ),
            Opt::new(
                "rag",
                "Document search (RAG)",
                "Lets the AI use your files. Adds about 1-2 GB of downloads.",
            )
            .unavailable_if(Some("coming in a later version".into())),
            Opt::new(
                "finetune",
                "Fine-tuning tools",
                "Installs training libraries (several GB).",
            )
            .unavailable_if(Some(ft)),
            Opt::new(
                "autostart",
                "Auto-start on login",
                "Starts your AI when the computer starts.",
            )
            .unavailable_if(Some("coming in a later version".into())),
        ],
        recommended: 0,
        reason: "Recommended: start simple, add features later.".into(),
    }
}

fn access() -> Question {
    Question {
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
                        rec.unavailable.is_none(),
                        "{name}/{goal}/{key}: recommended `{}` is unavailable",
                        rec.value
                    );
                    assert!(q.recommended < q.options.len());
                    let v = rec.value.clone();
                    answers.set(key, v);
                }
            }
        }
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

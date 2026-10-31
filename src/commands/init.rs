use std::path::PathBuf;

use console::style;

use crate::cli::InitArgs;
use crate::core::catalog::Catalog;
use crate::core::config::{Config, GenerationCfg, ModelCfg, RuntimeCfg, ServerCfg, SCHEMA_VERSION};
use crate::core::hardware::{self, Confidence, Hardware};
use crate::core::{engine, process, project, secrets};
use crate::error::{fail_hint, Exit};
use crate::ui;
use crate::wizard::options::{Answers, Ctx};
use crate::wizard::{prompt, questions};

fn flag<'a>(args: &'a InitArgs, key: &str) -> Option<&'a str> {
    match key {
        "vram" => args.vram.as_deref(),
        "goal" => args.goal.as_deref(),
        "model" => args.model.as_deref(),
        "quant" => args.quant.as_deref(),
        "runtime" => args.runtime.as_deref(),
        "ctx" => args.ctx.as_deref(),
        "interface" => args.interface.as_deref(),
        "access" => args.access.as_deref(),
        "port" => args.port.as_deref(),
        _ => None,
    }
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn apply_vram(hw: &mut Hardware, base: &Hardware, value: &str) {
    *hw = base.clone();
    if let Ok(v) = value.parse::<f64>() {
        hw.vram_gb = Some(v);
        hw.vram_source = "you entered it".into();
        hw.vram_confidence = Confidence::Medium;
    }
}

struct Wizard<'a> {
    base_hw: Hardware,
    hw: Hardware,
    catalog: Catalog,
    answers: Answers,
    args: &'a InitArgs,
    yes: bool,
}

impl Wizard<'_> {
    /// Asks one question (or applies its flag). Returns false if the question does not apply.
    fn ask(&mut self, key: &str, force_prompt: bool) -> anyhow::Result<bool> {
        let q = {
            let c = Ctx {
                hw: &self.hw,
                catalog: &self.catalog,
                answers: &self.answers,
            };
            questions::build(key, &c)?
        };
        let Some(q) = q else { return Ok(false) };

        let value = match flag(self.args, key).filter(|_| !force_prompt) {
            Some(v) if key == "port" && q.find(v).is_none() => parse_port(v, &self.answers)?,
            Some(v) => prompt::check_flag(&q, v)?,
            None => prompt::ask(&q, self.yes && !force_prompt)?,
        };
        let value = match (key, value.as_str()) {
            ("port", "custom") => ask_port(&self.answers)?,
            ("model", "custom") => {
                if !ui::interactive() {
                    return fail_hint(
                        Exit::InvalidConfig,
                        "a custom model needs an interactive terminal",
                        "run `yourai init` without --yes",
                    );
                }
                self.answers.custom_repo = inquire::Text::new("Hugging Face repo (owner/name):")
                    .with_validator(|s: &str| {
                        Ok(if s.split('/').count() == 2 && !s.contains("..") {
                            inquire::validator::Validation::Valid
                        } else {
                            inquire::validator::Validation::Invalid("use the form owner/name".into())
                        })
                    })
                    .prompt()?;
                self.answers.custom_file = inquire::Text::new("GGUF file name in that repo:")
                    .with_validator(|s: &str| {
                        Ok(if s.ends_with(".gguf") && !s.contains('/') {
                            inquire::validator::Validation::Valid
                        } else {
                            inquire::validator::Validation::Invalid("must be a file name ending in .gguf".into())
                        })
                    })
                    .prompt()?;
                value
            }
            _ => value,
        };
        if key == "vram" {
            let base = self.base_hw.clone();
            apply_vram(&mut self.hw, &base, &value);
        }
        if key == "access" && value == "lan" {
            ui::warn("Other devices on your network will be able to reach this AI. An API key will be generated and required.");
        }
        self.answers.set(key, value);
        Ok(true)
    }

    /// True if the current answer for `key` is still a valid, available choice.
    fn still_valid(&self, key: &str) -> anyhow::Result<bool> {
        let c = Ctx {
            hw: &self.hw,
            catalog: &self.catalog,
            answers: &self.answers,
        };
        let Some(q) = questions::build(key, &c)? else {
            return Ok(true);
        };
        let cur = self.answers.get(key);
        if key == "port" && cur.parse::<u16>().is_ok() {
            return Ok(true);
        }
        Ok(q.find(cur).is_some_and(|o| o.unavailable.is_none()))
    }
}

fn parse_port(v: &str, a: &Answers) -> anyhow::Result<String> {
    let host = if a.access == "lan" { "0.0.0.0" } else { "127.0.0.1" };
    match v.parse::<u16>() {
        Ok(p) if p >= 1024 && process::port_free(host, p) => Ok(p.to_string()),
        Ok(p) if p >= 1024 => fail_hint(
            Exit::Resources,
            format!("port {p} is already in use"),
            "pick another --port",
        ),
        _ => fail_hint(
            Exit::InvalidConfig,
            format!("--port {v} is not valid"),
            "use a number from 1024 to 65535",
        ),
    }
}

fn ask_port(a: &Answers) -> anyhow::Result<String> {
    let host = if a.access == "lan" { "0.0.0.0" } else { "127.0.0.1" }.to_string();
    let p = inquire::Text::new("Port number:")
        .with_validator(move |s: &str| {
            Ok(match s.parse::<u16>() {
                Ok(p) if p >= 1024 && process::port_free(&host, p) => inquire::validator::Validation::Valid,
                Ok(p) if p >= 1024 => inquire::validator::Validation::Invalid(format!("port {p} is in use").into()),
                _ => inquire::validator::Validation::Invalid("use a number from 1024 to 65535".into()),
            })
        })
        .prompt()?;
    Ok(p)
}

pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let yes = args.yes;
    if !yes && !ui::interactive() {
        return fail_hint(
            Exit::InvalidConfig,
            "`yourai init` needs an interactive terminal",
            "run it in a terminal, or pass --yes to accept the recommended answers",
        );
    }

    let hw = hardware::detect();
    ui::heading("Your computer");
    println!("{}", super::doctor::hardware_table(&hw));

    let pb = ui::spinner("Checking for the latest model list…");
    let (catalog, refreshed) = Catalog::refresh();
    pb.finish_and_clear();
    if refreshed.is_err() {
        println!(
            "{}",
            style("Could not fetch the latest model list; using the built-in one.").dim()
        );
    }

    let name = match (&args.name, &args.dir) {
        (Some(n), _) => n.clone(),
        (None, Some(d)) => d
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "my-ai".into()),
        (None, None) if yes => "my-ai".into(),
        (None, None) => inquire::Text::new("Project name:")
            .with_default("my-ai")
            .with_help_message("letters, numbers, - and _; a folder with this name is created")
            .with_validator(|s: &str| {
                Ok(if valid_name(s) {
                    inquire::validator::Validation::Valid
                } else {
                    inquire::validator::Validation::Invalid("use letters, numbers, - and _ only".into())
                })
            })
            .prompt()?,
    };
    if !valid_name(&name) {
        return fail_hint(
            Exit::InvalidConfig,
            format!("invalid project name `{name}`"),
            "use letters, numbers, - and _ only",
        );
    }
    let dir: PathBuf = args.dir.clone().unwrap_or_else(|| PathBuf::from(&name));
    if dir.exists() && std::fs::read_dir(&dir)?.next().is_some() {
        return fail_hint(
            Exit::General,
            format!("{} already exists and is not empty", dir.display()),
            "choose another name or folder; existing files are never overwritten",
        );
    }

    let mut w = Wizard {
        base_hw: hw.clone(),
        hw,
        catalog,
        answers: Answers::default(),
        args: &args,
        yes,
    };
    for key in questions::ORDER {
        w.ask(key, false)?;
    }

    loop {
        print_review(&w, &dir);
        if yes {
            break;
        }
        let choice = inquire::Select::new(
            "Create project?",
            vec!["Create project (Recommended)", "Go back and change something", "Cancel"],
        )
        .prompt()?;
        match choice {
            "Cancel" => return Err(crate::error::CliError::new(Exit::Cancelled, "cancelled").into()),
            "Go back and change something" => {
                let asked: Vec<&str> = questions::ORDER
                    .iter()
                    .copied()
                    .filter(|k| !w.answers.get(k).is_empty())
                    .collect();
                let key = inquire::Select::new("What do you want to change?", asked.clone()).prompt()?;
                w.ask(key, true)?;
                let pos = questions::ORDER.iter().position(|k| *k == key).unwrap_or(0);
                for later in &questions::ORDER[pos + 1..] {
                    if w.answers.get(later).is_empty() || !w.still_valid(later)? {
                        w.ask(later, true)?;
                    }
                }
            }
            _ => break,
        }
    }

    let a = &w.answers;
    let lan = a.access == "lan";
    let cfg = Config {
        schema_version: SCHEMA_VERSION,
        name: name.clone(),
        goal: a.goal.clone(),
        system_prompt: "prompts/system.md".into(),
        model: ModelCfg {
            id: a.model.clone(),
            quant: if a.model == "custom" {
                "custom".into()
            } else {
                a.quant.clone()
            },
            repo: a.custom_repo.clone(),
            file: a.custom_file.clone(),
        },
        runtime: RuntimeCfg {
            engine: a.runtime.clone(),
            gpu_layers: "auto".into(),
            context_length: a.ctx_tokens(),
            threads: 0,
        },
        generation: GenerationCfg {
            temperature: if a.goal == "code" { 0.2 } else { 0.7 },
            top_p: 0.9,
            max_tokens: 1024,
        },
        server: ServerCfg {
            host: if lan { "0.0.0.0".into() } else { "127.0.0.1".into() },
            port: a.port.parse().unwrap_or(8080),
            web_ui: a.interface == "webui",
            require_api_key: lan,
        },
    };
    cfg.validate()?;
    let key = lan.then(secrets::generate_key);
    project::scaffold(&dir, &cfg, key.as_deref())?;

    ui::ok(format!("Created {}", dir.display()));
    if let Some(k) = key {
        ui::info(format!(
            "API key saved to {} ({}). Apps must send it as `Authorization: Bearer <key>`.",
            secrets::path(&dir).display(),
            secrets::redact(&k)
        ));
    }
    ui::next(format!("cd {} && yourai build && yourai start", dir.display()));
    Ok(())
}

fn label_for(w: &Wizard, key: &str) -> String {
    let c = Ctx {
        hw: &w.hw,
        catalog: &w.catalog,
        answers: &w.answers,
    };
    let v = w.answers.get(key);
    if key == "model" && v == "custom" {
        return format!("{}/{}", w.answers.custom_repo, w.answers.custom_file);
    }
    questions::build(key, &c)
        .ok()
        .flatten()
        .and_then(|q| q.find(v).map(|o| o.label.clone()))
        .unwrap_or_else(|| v.to_string())
}

fn print_review(w: &Wizard, dir: &std::path::Path) {
    ui::heading("Review");
    let mut t = ui::table();
    t.set_header(vec!["Setting", "Your choice"]);
    t.add_row(vec!["Project folder".to_string(), dir.display().to_string()]);
    for key in questions::ORDER {
        if w.answers.get(key).is_empty() {
            continue;
        }
        let title = match *key {
            "vram" => "GPU memory",
            "goal" => "Goal",
            "model" => "Model",
            "quant" => "Quantization",
            "runtime" => "Runtime",
            "ctx" => "Context length",
            "interface" => "Interface",
            "features" => "Extra features",
            "access" => "Network access",
            "port" => "Port",
            _ => key,
        };
        t.add_row(vec![title.to_string(), label_for(w, key)]);
    }
    println!("{t}");

    let entry = w.catalog.llama();
    let engine_mb: f64 = engine::select_key(&w.hw, entry)
        .ok()
        .filter(|(k, _)| !engine::install_dir(&entry.version, k).join(".installed").is_file())
        .and_then(|(k, _)| entry.assets.get(&k).map(|v| v.iter().map(|a| a.size_mb).sum()))
        .unwrap_or(0.0);
    let model = w.catalog.model(&w.answers.model);
    let model_gb = model.and_then(|m| m.quants.get(&w.answers.quant)).map(|q| q.size_gb);
    let mem = model.and_then(|m| m.memory_gb(&w.answers.quant, w.answers.ctx_tokens()));

    let mut e = ui::table();
    e.set_header(vec!["Estimate", "Amount"]);
    let engine_text = if engine_mb > 0.0 {
        format!("{} engine", ui::gb(engine_mb / 1000.0))
    } else {
        "engine already installed".to_string()
    };
    e.add_row(vec![
        "Download".to_string(),
        match model_gb {
            Some(g) => format!("{} model + {engine_text}", ui::gb(g)),
            None => format!("custom model (size unknown) + {engine_text}"),
        },
    ]);
    e.add_row(vec![
        "Memory while running".to_string(),
        mem.map(|m| format!("about {} (budget {})", ui::gb(m), ui::gb(hardware::budget_gb(&w.hw))))
            .unwrap_or_else(|| "unknown for custom models".into()),
    ]);
    e.add_row(vec![
        "Disk use".to_string(),
        format!(
            "about {} in ~/.yourai (shared by all projects)",
            ui::gb(model_gb.unwrap_or(0.0) + engine_mb * 2.5 / 1000.0)
        ),
    ]);
    println!("{e}");
    println!(
        "{}",
        style("Nothing is downloaded yet. `yourai build` downloads after you confirm the model license.").dim()
    );
}

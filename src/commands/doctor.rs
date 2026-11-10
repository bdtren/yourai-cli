use std::path::Path;
use std::time::Duration;

use comfy_table::{Cell, Color};
use console::style;

use crate::core::catalog::Catalog;
use crate::core::config::Capability;
use crate::core::hardware::{self, Confidence, GpuVendor, Hardware};
use crate::core::project::Project;
use crate::core::{engine, paths, process, recommender};
use crate::ui;

pub fn hardware_table(hw: &Hardware) -> comfy_table::Table {
    let mut t = ui::table();
    t.set_header(vec!["Item", "Value", "Source", "Confidence"]);
    let row = |t: &mut comfy_table::Table, item: &str, value: String, source: &str, c: Confidence| {
        t.add_row(vec![item.to_string(), value, source.to_string(), c.label().to_string()]);
    };
    row(
        &mut t,
        "Operating system",
        format!("{} ({})", hw.os, hw.arch),
        "system",
        Confidence::High,
    );
    row(
        &mut t,
        "CPU",
        format!("{} · {} cores / {} threads", hw.cpu, hw.cpu_cores, hw.cpu_threads),
        "system",
        Confidence::High,
    );
    row(
        &mut t,
        "RAM",
        format!("{:.1} GB total, {:.1} GB free now", hw.ram_gb, hw.ram_available_gb),
        "system",
        Confidence::High,
    );
    let gpu = if hw.gpu_vendor == GpuVendor::None {
        "none detected".to_string()
    } else {
        format!("{} {}", hw.gpu_vendor.label(), hw.gpu_name)
    };
    row(
        &mut t,
        "GPU",
        gpu,
        "drivers / system tools",
        if hw.gpu_vendor == GpuVendor::None {
            Confidence::Low
        } else {
            Confidence::High
        },
    );
    let vram = match hw.vram_gb {
        Some(v) if hw.gpu_vendor == GpuVendor::Apple => format!("about {v:.1} GB usable (estimate)"),
        Some(v) => format!("{v:.1} GB"),
        None => "unknown".into(),
    };
    row(&mut t, "GPU memory", vram, &hw.vram_source, hw.vram_confidence);
    let disk_label = format!("Free disk ({})", paths::home().display());
    row(
        &mut t,
        &disk_label,
        format!("{:.1} GB", hw.disk_free_gb),
        "system",
        Confidence::High,
    );
    row(
        &mut t,
        "Memory budget for models",
        format!(
            "{:.1} GB ({})",
            hardware::budget_gb(hw),
            if hardware::cpu_only(hw) {
                "half of RAM, CPU mode"
            } else {
                "GPU memory"
            }
        ),
        "yourai estimate",
        Confidence::Medium,
    );
    t
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix: Option<String>,
}

fn reachable(url: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;
    client.head(url).send().map(|_| ()).map_err(|e| e.to_string())
}

pub fn run(fix: bool, network: bool, project: Option<&Path>) -> anyhow::Result<()> {
    let hw = hardware::detect();
    let catalog = Catalog::load();
    ui::heading("Your computer");
    println!("{}", hardware_table(&hw));

    let mut checks = vec![
        Check {
            name: "RAM",
            ok: hw.ram_gb >= 8.0,
            detail: format!("{:.1} GB", hw.ram_gb),
            fix: (hw.ram_gb < 8.0).then(|| "Only tiny models will run well. Close other apps before starting.".into()),
        },
        Check {
            name: "Disk space",
            ok: hw.disk_free_gb >= 20.0,
            detail: format!("{:.1} GB free", hw.disk_free_gb),
            fix: (hw.disk_free_gb < 20.0).then(|| "Models are 1-40 GB. Free some space before downloading.".into()),
        },
    ];

    match hw.gpu_vendor {
        GpuVendor::Nvidia => checks.push(Check {
            name: "GPU driver",
            ok: true,
            detail: "nvidia-smi works".into(),
            fix: None,
        }),
        GpuVendor::Amd | GpuVendor::Intel => checks.push(Check {
            name: "GPU driver",
            ok: hw.vram_gb.is_some(),
            detail: if hw.vram_gb.is_some() {
                "GPU memory readable".into()
            } else {
                "GPU memory unknown".into()
            },
            fix: hw.vram_gb.is_none().then(|| {
                "The Vulkan build will be tried. `yourai init` will ask for your GPU memory size.".to_string()
            }),
        }),
        GpuVendor::None if hw.os_family != "macos" => checks.push(Check {
            name: "GPU",
            ok: false,
            detail: "no supported GPU detected".into(),
            fix: Some(
                "CPU mode works but is slower. NVIDIA users: install the driver from https://www.nvidia.com/drivers"
                    .into(),
            ),
        }),
        _ => {}
    }

    let entry = catalog.llama();
    match engine::select_key(&hw, entry) {
        Ok((key, note)) => checks.push(Check {
            name: "Engine build",
            ok: note.is_none(),
            detail: format!("llama.cpp {} ({key})", entry.version),
            fix: note,
        }),
        Err(e) => checks.push(Check {
            name: "Engine build",
            ok: false,
            detail: e.to_string(),
            fix: None,
        }),
    }

    let port_ok = process::port_free("127.0.0.1", 8080);
    let alt = process::next_free_port("127.0.0.1", 8081);
    checks.push(Check {
        name: "Port 8080",
        ok: port_ok,
        detail: if port_ok {
            "free".into()
        } else {
            "in use by another program".into()
        },
        fix: (!port_ok).then(|| {
            format!(
                "Use port {} (yourai init suggests it automatically)",
                alt.unwrap_or(8081)
            )
        }),
    });

    if network {
        for (name, url) in [
            ("Network: yourai.bdtren.com", paths::base_url() + "/latest.txt"),
            ("Network: Hugging Face", "https://huggingface.co".to_string()),
            ("Network: GitHub", "https://github.com".to_string()),
        ] {
            let r = reachable(&url);
            checks.push(Check {
                name,
                ok: r.is_ok(),
                detail: r
                    .as_ref()
                    .map(|_| "reachable".to_string())
                    .unwrap_or_else(|e| e.clone()),
                fix: r
                    .err()
                    .map(|_| "Check your connection, or set HTTPS_PROXY if you are behind a proxy.".into()),
            });
        }
    }

    checks.push(Check {
        name: "Ollama",
        ok: true,
        detail: if hw.ollama {
            "installed (optional runtime; project models stay under yourai_modules/)".into()
        } else {
            "not installed (not needed for llama.cpp)".into()
        },
        fix: None,
    });

    ui::heading("Checks");
    let mut t = ui::table();
    t.set_header(vec!["Check", "Result", "Details", "What to do"]);
    for c in &checks {
        // Color via Cell (not ANSI-in-string): embedded escapes inflate column
        // width under comfy-table without `custom_styling` and shift separators left.
        let mark = if c.ok {
            Cell::new("ok").fg(Color::Green)
        } else {
            Cell::new("warn").fg(Color::Yellow)
        };
        t.add_row(vec![
            Cell::new(c.name),
            mark,
            Cell::new(&c.detail),
            Cell::new(c.fix.as_deref().unwrap_or("")),
        ]);
    }
    println!("{t}");

    let budget = hardware::budget_gb(&hw);
    ui::heading("What you can run");
    for (label, goal, vision) in [
        ("chat", "chat", false),
        ("code", "code", false),
        ("vision", "chat", true),
    ] {
        match recommender::recommend_model(&hw, &catalog, goal, budget, vision) {
            Some(m) => ui::info(format!(
                "{label}: {} with {} (about {:.1} GB of memory)",
                m.name,
                recommender::recommend_quant(m, budget, recommender::DEFAULT_CTX),
                m.memory_gb("Q4_K_M", recommender::DEFAULT_CTX).unwrap_or(0.0)
            )),
            None => ui::warn(format!("{label}: no catalog model fits in {budget:.1} GB")),
        }
    }
    for cap in Capability::ALL {
        let label = cap.label().to_lowercase();
        let spec = engine::spec(cap.engine()).expect("known engine");
        if !engine::available(&hw, &catalog, spec) {
            ui::warn(format!("{label}: {} has no build for this computer yet", spec.label));
            continue;
        }
        match recommender::recommend_cap_model(&hw, &catalog, cap, budget, budget) {
            Some(m) => ui::info(format!(
                "{label}: {} (about {:.1} GB of memory while working)",
                m.name,
                m.memory_gb(m.default_quant(), 0).unwrap_or(0.0)
            )),
            None => {
                let why = catalog
                    .models_of(cap.kind())
                    .filter_map(|m| recommender::model_blocker(&hw, budget, m))
                    .last()
                    .unwrap_or_else(|| "no model in the catalog".into());
                ui::warn(format!("{label}: not on this computer ({why})"));
            }
        }
    }
    if !network {
        println!(
            "{}",
            style("Network was not tested. Run `yourai doctor --network` to check downloads.").dim()
        );
    }

    if fix {
        ui::heading("Safe fixes");
        paths::ensure_home()?;
        ui::ok(format!("folders ready in {}", paths::home().display()));
        if let Ok(p) = Project::discover(project) {
            let cfg = p.config()?;
            if !process::port_free(&cfg.server.host, cfg.server.port) && p.state().state != "running" {
                if let Some(port) = process::next_free_port(&cfg.server.host, cfg.server.port + 1) {
                    super::config::set_value(&p, "server.port", &port.to_string())?;
                    ui::ok(format!("port {} was busy, project now uses {port}", cfg.server.port));
                }
            }
        }
        println!(
            "{}",
            style("doctor --fix never installs drivers or system software.").dim()
        );
    }

    ui::next("yourai init");
    Ok(())
}

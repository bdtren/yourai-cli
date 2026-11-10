use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use console::style;

use crate::core::config::{Capability, Config};
use crate::core::project::{Project, State};
use crate::core::{gateway, lock, process, secrets};
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(300);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns the live state, cleaning up a stale "running" entry if the process is gone.
pub fn live_state(p: &Project) -> State {
    let mut s = p.state();
    if s.state == "running" {
        let alive = s.pid.is_some_and(|pid| process::is_ours(pid, s.pid_start_time));
        if !alive {
            s = State {
                state: "stopped".into(),
                ..Default::default()
            };
            let _ = p.save_state(&s);
        }
    }
    s
}

pub fn health(cfg: &Config) -> Option<u16> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    client
        .get(format!("{}/health", cfg.server.local_url()))
        .send()
        .ok()
        .map(|r| r.status().as_u16())
}

fn tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(f) = std::fs::File::open(path) else {
        return vec![];
    };
    let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

fn explain_failure(log_lines: &[String]) -> String {
    let all = log_lines.join("\n").to_lowercase();
    if all.contains("out of memory") || all.contains("failed to allocate") || all.contains("unable to allocate") {
        "the model did not fit in memory: try a smaller model, a lower quantization, a shorter \
         runtime.context_length, or set runtime.gpu_layers to a smaller number"
            .into()
    } else if all.contains("address already in use") || all.contains("couldn't bind") {
        "the port is in use: run `yourai doctor --fix` or `yourai config set server.port <port>`".into()
    } else if all.contains("cuda") || all.contains("vulkan") || all.contains("driver") {
        "the GPU build could not start; update your GPU driver, or run `yourai rebuild` to fall back".into()
    } else {
        "see the full log with `yourai logs`".into()
    }
}

fn api_key_file(p: &Project, cfg: &Config) -> anyhow::Result<Option<PathBuf>> {
    if !cfg.server.require_api_key {
        return Ok(None);
    }
    let Some(key) = secrets::api_key(&p.root) else {
        return fail_hint(
            Exit::Security,
            "server.require_api_key is true but no API key is set",
            "set YOURAI_API_KEY or create .yourai/secrets.toml with api_key = \"...\"",
        );
    };
    let path = p.state_dir().join("api-keys.txt");
    std::fs::write(&path, format!("{key}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(Some(path))
}

/// Starts the engine and waits for /health. Used by start, resume and rebuild.
pub fn launch(p: &Project, cfg: &Config) -> anyhow::Result<()> {
    let Some(lk) = p.lock() else {
        return fail_hint(
            Exit::Runtime,
            "this project has not been built yet",
            "run `yourai build`",
        );
    };
    let mut cfg = cfg.clone();
    let changed = lock::changed(&lk, &lock::section_hashes(&cfg, &p.system_prompt(&cfg), &p.root));
    if changed.iter().any(|s| lock::BUILD_SECTIONS.contains(&s.as_str())) {
        return fail_hint(
            Exit::Runtime,
            format!("{} changed since the last build", changed.join(", ")),
            "run `yourai rebuild` to apply the changes",
        );
    }
    let resolve = |s: &str| -> anyhow::Result<PathBuf> { p.resolve_path(s) };
    let engine_exe = resolve(&lk.engine_exe)?;
    let model = resolve(&lk.model_path)?;
    let mut needed: Vec<PathBuf> = vec![engine_exe, model];
    if !lk.mmproj_path.is_empty() {
        needed.push(resolve(&lk.mmproj_path)?);
    }
    for e in lk.engines.values() {
        needed.push(resolve(&e.exe)?);
    }
    for m in lk.models.values() {
        for f in m.files.values() {
            needed.push(resolve(f)?);
        }
    }
    if let Some(missing) = needed.iter().find(|f| !f.is_file()) {
        return fail_hint(
            Exit::Runtime,
            "this project has not been built yet",
            format!("run `yourai build` (missing {})", missing.display()),
        );
    }
    gateway::kill_leftovers(p);
    if !process::port_free(&cfg.server.host, cfg.server.port) {
        let alt = process::next_free_port(&cfg.server.host, cfg.server.port + 1).unwrap_or(cfg.server.port + 1);
        match pick_listen_port(cfg.server.port, false, alt, ui::interactive()) {
            PortPick::Keep(_) => {}
            PortPick::Offer { busy, alt } => {
                if !ui::confirm(&format!("Port {busy} is in use. Start on {alt} instead?"), true, false)? {
                    return Err(busy_port_err(busy, alt).into());
                }
                super::config::set_value(p, "server.port", &alt.to_string())?;
                cfg.server.port = alt;
                ui::ok(format!("server.port is now {alt}"));
            }
            PortPick::Abort { busy, alt } => {
                return Err(busy_port_err(busy, alt).into());
            }
        }
    }
    if !cfg.server.is_loopback() {
        ui::warn(format!(
            "listening on {} — other devices on your network can connect (API key required)",
            cfg.server.host
        ));
    }

    api_key_file(p, &cfg)?;
    let exe = std::env::current_exe()?;
    let args = vec![
        "--project".to_string(),
        p.root.display().to_string(),
        "__gateway".to_string(),
    ];
    let log = p.log_path();
    let log_start = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
    let pid = process::spawn_detached(&exe, &args, &log, &p.root)?;
    let started = process::start_time(pid);
    let state = State {
        state: "running".into(),
        pid: Some(pid),
        pid_start_time: started,
        port: Some(cfg.server.port),
        host: Some(cfg.server.host.clone()),
        engine_version: Some(lk.engine_version.clone()),
        started_at: Some(now()),
    };
    p.save_state(&state)?;

    let pb = ui::spinner("Loading the model (first start can take a minute)…");
    let t0 = Instant::now();
    loop {
        if !process::is_ours(pid, started) {
            pb.finish_and_clear();
            p.save_state(&State {
                state: "stopped".into(),
                ..Default::default()
            })?;
            let lines = new_log_lines(&log, log_start);
            for l in lines.iter().rev().take(15).rev() {
                eprintln!("  {}", style(l).dim());
            }
            return Err(CliError::new(Exit::Runtime, "the AI server stopped while starting")
                .hint(explain_failure(&lines))
                .into());
        }
        if health(&cfg) == Some(200) {
            break;
        }
        if t0.elapsed() > HEALTH_TIMEOUT {
            pb.finish_and_clear();
            process::stop(pid);
            p.save_state(&State {
                state: "stopped".into(),
                ..Default::default()
            })?;
            return fail_hint(
                Exit::Runtime,
                "the model did not finish loading in 5 minutes",
                "check `yourai logs`",
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    pb.finish_and_clear();
    let mut lk = lk;
    lk.sections = lock::section_hashes(&cfg, &p.system_prompt(&cfg), &p.root);
    lock::save(&p.lock_path(), &lk)?;
    ui::ok(format!(
        "{} is running (pid {pid}, loaded in {:.0}s)",
        cfg.name,
        t0.elapsed().as_secs_f64()
    ));
    let url = cfg.server.local_url();
    if cfg.server.web_ui {
        ui::info(format!("Chat in your browser: {url}"));
    }
    ui::info(format!("OpenAI-compatible API: {url}/v1"));
    let extras: Vec<&str> = cfg
        .enabled_caps()
        .into_iter()
        .map(|c| match c {
            Capability::Image => "`yourai image`",
            Capability::Video => "`yourai video`",
            Capability::SpeechToText => "`yourai transcribe`",
            Capability::TextToSpeech => "`yourai speak`",
        })
        .collect();
    if !extras.is_empty() {
        ui::info(format!(
            "Also ready: {} (engines load on first use, unload after {} idle minutes)",
            extras.join(", "),
            cfg.server.idle_unload_minutes
        ));
    }
    if cfg.server.require_api_key {
        ui::info("API key required: send `Authorization: Bearer <key>` (key is in .yourai/secrets.toml)");
    }
    Ok(())
}

fn new_log_lines(log: &Path, from: u64) -> Vec<String> {
    let Ok(mut f) = std::fs::File::open(log) else {
        return vec![];
    };
    let _ = f.seek(SeekFrom::Start(from));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    s.lines().map(str::to_string).collect()
}

pub fn start(p: &Project) -> anyhow::Result<()> {
    let cfg = p.config()?;
    let s = live_state(p);
    if s.state == "running" {
        ui::ok(format!("already running on port {}", s.port.unwrap_or(cfg.server.port)));
        return Ok(());
    }
    launch(p, &cfg)?;
    ui::next("yourai chat   (or open the URL above)");
    Ok(())
}

pub fn stop_process(p: &Project, s: &State) {
    if let Some(pid) = s.pid {
        if process::is_ours(pid, s.pid_start_time) {
            process::stop(pid);
        }
    }
    gateway::kill_leftovers(p);
    let _ = std::fs::remove_file(p.state_dir().join("api-keys.txt"));
}

fn engine_statuses(cfg: &Config, p: &Project) -> Option<Vec<gateway::EngineStatus>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let mut rb = client.get(format!("{}/yourai/status", cfg.server.local_url()));
    if let Some(k) = secrets::api_key(&p.root) {
        rb = rb.bearer_auth(k);
    }
    let v: serde_json::Value = rb.send().ok()?.error_for_status().ok()?.json().ok()?;
    serde_json::from_value(v.get("engines")?.clone()).ok()
}

pub fn stop(p: &Project) -> anyhow::Result<()> {
    let s = live_state(p);
    if s.state == "stopped" {
        ui::ok("already stopped");
        return Ok(());
    }
    let pb = ui::spinner("Stopping…");
    stop_process(p, &s);
    pb.finish_and_clear();
    p.save_state(&State {
        state: "stopped".into(),
        ..Default::default()
    })?;
    ui::ok("stopped");
    Ok(())
}

pub fn pause(p: &Project) -> anyhow::Result<()> {
    let s = live_state(p);
    match s.state.as_str() {
        "paused" => {
            ui::ok("already paused");
            Ok(())
        }
        "running" => {
            let pb = ui::spinner("Unloading the model…");
            stop_process(p, &s);
            pb.finish_and_clear();
            p.save_state(&State {
                state: "paused".into(),
                port: s.port,
                ..Default::default()
            })?;
            ui::ok("paused: the model is unloaded and its memory is free");
            ui::next("yourai resume");
            Ok(())
        }
        _ => fail_hint(
            Exit::Runtime,
            "the AI is not running, so there is nothing to pause",
            "run `yourai start`",
        ),
    }
}

pub fn resume(p: &Project) -> anyhow::Result<()> {
    let s = live_state(p);
    match s.state.as_str() {
        "running" => {
            ui::ok("already running");
            Ok(())
        }
        "paused" => launch(p, &p.config()?),
        _ => fail_hint(Exit::Runtime, "the AI is stopped, not paused", "run `yourai start`"),
    }
}

pub fn status(p: &Project) -> anyhow::Result<()> {
    let cfg = p.config()?;
    let s = live_state(p);
    let lk = p.lock();
    let mut t = ui::table();
    t.set_header(vec!["", ""]);
    let state = match s.state.as_str() {
        "running" => style("running").green().to_string(),
        "paused" => style("paused (model unloaded)").yellow().to_string(),
        _ => style("stopped").dim().to_string(),
    };
    t.add_row(vec!["State".to_string(), state]);
    t.add_row(vec![
        "Project".to_string(),
        format!("{} ({})", cfg.name, p.root.display()),
    ]);
    t.add_row(vec![
        "Model".to_string(),
        format!("{} {}", cfg.model.id, cfg.model.quant),
    ]);
    t.add_row(vec![
        "Built".to_string(),
        match &lk {
            Some(l) => format!("yes, engine llama.cpp {} ({})", l.engine_version, l.engine_key),
            None => "no, run `yourai build`".into(),
        },
    ]);
    if cfg.store.project_modules {
        t.add_row(vec![
            "Module".to_string(),
            format!("{} (models and engines)", p.module_dir().display()),
        ]);
        t.add_row(vec![
            "Shared cache".to_string(),
            format!(
                "{} (optional, with `yourai build -s`)",
                crate::core::paths::home().display()
            ),
        ]);
    } else {
        t.add_row(vec![
            "Store".to_string(),
            format!(
                "{} (shared; store.project_modules = false)",
                crate::core::paths::home().display()
            ),
        ]);
    }
    t.add_row(vec![
        "Address".to_string(),
        format!("{}  (API: {}/v1)", cfg.server.local_url(), cfg.server.local_url()),
    ]);
    if s.state == "running" {
        let engines = engine_statuses(&cfg, p).unwrap_or_default();
        if let Some(pid) = s.pid {
            t.add_row(vec!["Process".to_string(), format!("pid {pid}")]);
            let chat = engines.iter().find(|e| e.name == "chat");
            if let Some(mem) = chat.and_then(|e| e.memory_bytes).or_else(|| process::memory(pid)) {
                t.add_row(vec!["Memory (RAM)".to_string(), ui::gb(mem as f64 / 1e9)]);
            }
        }
        if let Some(at) = s.started_at {
            let up = now().saturating_sub(at);
            t.add_row(vec![
                "Uptime".to_string(),
                format!("{}h {}m {}s", up / 3600, (up % 3600) / 60, up % 60),
            ]);
        }
        let h = match health(&cfg) {
            Some(200) => style("healthy").green().to_string(),
            Some(503) => style("loading model").yellow().to_string(),
            Some(c) => format!("HTTP {c}"),
            None => style("not responding").red().to_string(),
        };
        t.add_row(vec!["Health".to_string(), h]);
        for e in engines {
            if !e.enabled || e.name == "chat" {
                continue;
            }
            let state = match (e.running, e.memory_bytes) {
                (true, Some(m)) => format!("loaded, {} RAM, port {}", ui::gb(m as f64 / 1e9), e.port.unwrap_or(0)),
                (true, None) => format!("loaded, port {}", e.port.unwrap_or(0)),
                (false, _) if e.name == "text_to_speech" => "runs per request".into(),
                (false, _) => "ready (loads on first use)".into(),
            };
            t.add_row(vec![e.name.replace('_', " "), format!("{} — {state}", e.model)]);
        }
    } else {
        for c in cfg.enabled_caps() {
            let m = cfg.cap(c);
            t.add_row(vec![c.key().replace('_', " "), m.model.to_string()]);
        }
    }
    if let Some(l) = &lk {
        let changed = lock::changed(l, &lock::section_hashes(&cfg, &p.system_prompt(&cfg), &p.root));
        if !changed.is_empty() {
            t.add_row(vec![
                "Pending changes".to_string(),
                format!("{} (run `yourai rebuild`)", changed.join(", ")),
            ]);
        }
    }
    println!("{t}");
    Ok(())
}

pub fn logs(p: &Project, follow: bool, lines: usize) -> anyhow::Result<()> {
    let path = p.log_path();
    if !path.is_file() {
        ui::info("no logs yet; they appear after `yourai start`");
        return Ok(());
    }
    for l in tail(&path, lines) {
        println!("{l}");
    }
    if follow {
        let mut f = std::fs::File::open(&path)?;
        let mut pos = f.seek(SeekFrom::End(0))?;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let len = std::fs::metadata(&path)?.len();
            if len < pos {
                pos = 0;
            }
            if len > pos {
                f.seek(SeekFrom::Start(pos))?;
                let mut buf = String::new();
                f.read_to_string(&mut buf)?;
                print!("{buf}");
                pos = len;
            }
        }
    }
    Ok(())
}

/// What to do when the configured listen port may already be taken.
#[derive(Debug, PartialEq, Eq)]
enum PortPick {
    Keep(u16),
    /// Interactive: ask before writing `server.port` and continuing.
    Offer {
        busy: u16,
        alt: u16,
    },
    /// Non-interactive: refuse with a hint to set the port.
    Abort {
        busy: u16,
        alt: u16,
    },
}

fn pick_listen_port(configured: u16, free: bool, alt: u16, interactive: bool) -> PortPick {
    if free {
        PortPick::Keep(configured)
    } else if interactive {
        PortPick::Offer { busy: configured, alt }
    } else {
        PortPick::Abort { busy: configured, alt }
    }
}

fn busy_port_err(busy: u16, alt: u16) -> CliError {
    CliError::new(Exit::Runtime, format!("port {busy} is already used by another program")).hint(format!(
        "run `yourai config set server.port {alt}`, then `yourai start`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_hints() {
        assert!(explain_failure(&["ggml: failed to allocate buffer".into()]).contains("smaller model"));
        assert!(explain_failure(&["couldn't bind HTTP server socket".into()]).contains("port"));
    }

    #[test]
    fn busy_port_offers_alt_only_when_interactive() {
        assert_eq!(pick_listen_port(8080, true, 8081, false), PortPick::Keep(8080));
        assert_eq!(
            pick_listen_port(8080, false, 8081, true),
            PortPick::Offer { busy: 8080, alt: 8081 }
        );
        assert_eq!(
            pick_listen_port(8080, false, 8081, false),
            PortPick::Abort { busy: 8080, alt: 8081 }
        );
        let err = busy_port_err(8080, 8081);
        assert!(err.to_string().contains("8080"));
    }
}

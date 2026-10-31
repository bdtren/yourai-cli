use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use console::style;
use serde_json::{json, Value};

use super::lifecycle;
use crate::core::project::Project;
use crate::core::secrets;
use crate::error::{fail_hint, Exit};
use crate::ui;

pub fn run(p: &Project) -> anyhow::Result<()> {
    let cfg = p.config()?;
    if lifecycle::live_state(p).state != "running" {
        if !ui::interactive() {
            return fail_hint(Exit::Runtime, "the AI is not running", "run `yourai start` first");
        }
        if !ui::confirm("The AI is not running. Start it now?", true, false)? {
            return fail_hint(Exit::Cancelled, "the AI is not running", "run `yourai start` first");
        }
        lifecycle::launch(p, &cfg)?;
    }
    if !ui::interactive() {
        return fail_hint(
            Exit::General,
            "`yourai chat` needs an interactive terminal",
            "use the API instead",
        );
    }

    let system = p.system_prompt(&cfg);
    let key = cfg.server.require_api_key.then(|| secrets::api_key(&p.root)).flatten();
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(None)
        .build()?;
    let url = format!("{}/v1/chat/completions", cfg.server.local_url());
    let reset = || -> Vec<Value> {
        if system.trim().is_empty() {
            vec![]
        } else {
            vec![json!({"role": "system", "content": system})]
        }
    };
    let mut messages = reset();

    println!(
        "{}",
        style(format!(
            "Chatting with {} ({}). Type /exit to quit, /reset to start over.",
            cfg.name, cfg.model.id
        ))
        .dim()
    );
    loop {
        let input = match inquire::Text::new("you ›").prompt() {
            Ok(s) => s,
            Err(inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted) => break,
            Err(e) => return Err(e.into()),
        };
        let input = input.trim();
        match input {
            "" => continue,
            "/exit" | "/quit" => break,
            "/reset" => {
                messages = reset();
                ui::ok("conversation cleared");
                continue;
            }
            _ => {}
        }
        messages.push(json!({"role": "user", "content": input}));
        let body = json!({
            "messages": messages,
            "stream": true,
            "temperature": cfg.generation.temperature,
            "top_p": cfg.generation.top_p,
            "max_tokens": cfg.generation.max_tokens,
        });
        let mut req = client.post(&url).json(&body);
        if let Some(k) = &key {
            req = req.bearer_auth(k);
        }
        let resp = match req.send() {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let code = r.status();
                let text = r.text().unwrap_or_default();
                ui::warn(format!(
                    "the server answered {code}: {}",
                    text.chars().take(300).collect::<String>()
                ));
                messages.pop();
                continue;
            }
            Err(e) => {
                ui::warn(format!(
                    "could not reach the AI ({e}); is it still running? try `yourai status`"
                ));
                messages.pop();
                continue;
            }
        };
        print!("{} ", style("ai ›").cyan().bold());
        std::io::stdout().flush()?;
        let mut answer = String::new();
        for line in BufReader::new(resp).lines() {
            let line = line?;
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                print!("{piece}");
                std::io::stdout().flush()?;
                answer.push_str(piece);
            }
        }
        println!("\n");
        messages.push(json!({"role": "assistant", "content": answer}));
    }
    Ok(())
}

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use console::style;
use serde_json::{json, Value};

use super::agent::{self, Agent, Sandbox};
use super::lifecycle;
use super::media::Gateway;
use crate::core::project::Project;
use crate::core::secrets;
use crate::error::{fail_hint, Exit};
use crate::ui;

/// Tool rounds per question before the agent has to answer.
const MAX_TOOL_ROUNDS: usize = 8;

const HELP: &str = "/image <file>  attach a picture to your next message (vision models)
/audio <file>  attach a .wav or .mp3 recording (omni models)
/files         list the files the agent can see
/reset         start a new conversation
/exit          quit";

fn post(
    client: &reqwest::blocking::Client,
    url: &str,
    key: Option<&str>,
    body: &Value,
) -> Result<reqwest::blocking::Response, String> {
    let mut req = client.post(url).json(body);
    if let Some(k) = key {
        req = req.bearer_auth(k);
    }
    match req.send() {
        Ok(r) if r.status().is_success() => Ok(r),
        Ok(r) => {
            let code = r.status();
            let text = r.text().unwrap_or_default();
            Err(format!(
                "the server answered {code}: {}",
                text.chars().take(300).collect::<String>()
            ))
        }
        Err(e) => Err(format!(
            "could not reach the AI ({e}); is it still running? try `yourai status`"
        )),
    }
}

fn stream_answer(resp: reqwest::blocking::Response) -> anyhow::Result<String> {
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
    Ok(answer)
}

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
    let gateway = Gateway::connect(p)?;
    let tools = agent::definitions(&cfg);
    let agent = Agent {
        cfg: &cfg,
        root: &p.root,
        sandbox: Sandbox::new(&p.root, &cfg.agent.workspace)?,
        gateway: &gateway,
        max_chars: (cfg.runtime.context_length as usize * 2).max(2000),
    };
    let sees = p.lock().is_some_and(|l| !l.mmproj_path.is_empty());

    let reset = || -> Vec<Value> {
        let mut sys = system.trim().to_string();
        if !tools.is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str(
                "You can use tools to work with the user's files and media. Paths are relative to the \
                 user's workspace. Use a tool only when it helps answer the request.",
            );
        }
        if sys.is_empty() {
            vec![]
        } else {
            vec![json!({"role": "system", "content": sys})]
        }
    };
    let mut messages = reset();
    let mut attachments: Vec<Value> = Vec::new();

    let mut intro = format!(
        "Chatting with {} ({}). Type /help for commands, /exit to quit.",
        cfg.name, cfg.model.id
    );
    if !tools.is_empty() {
        let names: Vec<&str> = tools.iter().filter_map(|t| t["function"]["name"].as_str()).collect();
        intro.push_str(&format!("\nAgent tools: {}.", names.join(", ")));
    }
    println!("{}", style(intro).dim());
    loop {
        let input = match inquire::Text::new("you ›").prompt() {
            Ok(s) => s,
            Err(inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted) => break,
            Err(e) => return Err(e.into()),
        };
        let input = input.trim();
        let (cmd, rest) = input.split_once(' ').unwrap_or((input, ""));
        match cmd {
            "" => continue,
            "/exit" | "/quit" => break,
            "/help" => {
                println!("{}", style(HELP).dim());
                continue;
            }
            "/reset" => {
                messages = reset();
                attachments.clear();
                ui::ok("conversation cleared");
                continue;
            }
            "/files" => {
                match agent.sandbox.list(if rest.is_empty() { "." } else { rest.trim() }) {
                    Ok(list) => println!("{}", style(list).dim()),
                    Err(e) => ui::warn(e),
                }
                continue;
            }
            "/image" | "/audio" => {
                if rest.trim().is_empty() {
                    ui::warn(format!("usage: {cmd} <file>"));
                    continue;
                }
                if !sees {
                    ui::warn(
                        "this model cannot see images or hear audio; pick a vision or omni model, \
                         for example `yourai config set model.id qwen2.5-vl-3b`, then `yourai rebuild`",
                    );
                    continue;
                }
                let path = PathBuf::from(rest.trim().trim_matches('"'));
                match agent::attachment(&path, cmd == "/audio") {
                    Ok(part) => {
                        attachments.push(part);
                        ui::ok(format!("attached {}; now type your message", path.display()));
                    }
                    Err(e) => ui::warn(e.to_string()),
                }
                continue;
            }
            c if c.len() > 1 && c.starts_with('/') && !c[1..].contains('/') => {
                ui::warn(format!("unknown command {cmd}; type /help"));
                continue;
            }
            _ => {}
        }

        let user = if attachments.is_empty() {
            json!({"role": "user", "content": input})
        } else {
            let mut parts = vec![json!({"type": "text", "text": input})];
            parts.append(&mut attachments);
            json!({"role": "user", "content": parts})
        };
        let before = messages.len();
        if cfg.rag.enabled {
            let budget = (cfg.runtime.context_length as usize * 2).max(2000).min(6000);
            match crate::core::rag::retrieve(p, &cfg.rag, input) {
                Ok(hits) => {
                    if let Some(block) = crate::core::rag::context_block(&hits, budget) {
                        messages.push(json!({"role": "system", "content": block}));
                    }
                }
                Err(e) => ui::warn(format!("document search skipped: {e}")),
            }
        }
        messages.push(user);

        let mut body = json!({
            "messages": messages,
            "temperature": cfg.generation.temperature,
            "top_p": cfg.generation.top_p,
            "max_tokens": cfg.generation.max_tokens,
        });
        if tools.is_empty() {
            body["stream"] = json!(true);
            match post(&client, &url, key.as_deref(), &body).and_then(|r| stream_answer(r).map_err(|e| e.to_string())) {
                Ok(answer) if answer.trim().is_empty() => {
                    ui::warn(
                        "the model returned an empty reply (slow or interrupted stream). Try again, \
                         or ask something the local model can answer without live web data.",
                    );
                    messages.truncate(before);
                }
                Ok(answer) => messages.push(json!({"role": "assistant", "content": answer})),
                Err(e) => {
                    ui::warn(e);
                    messages.truncate(before);
                }
            }
            continue;
        }

        body["tools"] = json!(tools);
        let mut failed = false;
        for round in 0..=MAX_TOOL_ROUNDS {
            body["messages"] = json!(messages);
            if round == MAX_TOOL_ROUNDS {
                body.as_object_mut().unwrap().remove("tools");
            }
            let pb = ui::spinner("thinking…");
            let resp =
                post(&client, &url, key.as_deref(), &body).and_then(|r| r.json::<Value>().map_err(|e| e.to_string()));
            pb.finish_and_clear();
            let v = match resp {
                Ok(v) => v,
                Err(e) => {
                    ui::warn(e);
                    failed = true;
                    break;
                }
            };
            let msg = v["choices"][0]["message"].clone();
            let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
            if calls.is_empty() {
                let text = msg["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    ui::warn(
                        "the model returned an empty reply (slow or interrupted stream). Try again, \
                         or ask something the local model can answer without live web data.",
                    );
                    failed = true;
                    break;
                }
                println!("{} {text}\n", style("ai ›").cyan().bold());
                messages.push(json!({"role": "assistant", "content": text}));
                break;
            }
            messages.push(msg);
            for c in calls {
                let id = c["id"].as_str().unwrap_or("").to_string();
                let name = c["function"]["name"].as_str().unwrap_or("");
                let args = c["function"]["arguments"].as_str().unwrap_or("{}");
                let out = agent.call(name, args);
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": out}));
            }
        }
        if failed {
            messages.truncate(before);
        }
    }
    Ok(())
}

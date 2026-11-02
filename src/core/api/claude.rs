//! Anthropic Messages API under `POST /v1/messages`.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use super::chat_upstream::{self, collect_openai_stream};
use super::content_text;
use crate::core::gateway::{ApiError, Shared};

pub fn openai_from_anthropic(body: &Value) -> anyhow::Result<Value> {
    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("max_tokens is required"))?;
    let mut messages = Vec::new();
    if let Some(system) = body.get("system") {
        let text = match system {
            Value::String(s) => s.clone(),
            other => content_text(other)?,
        };
        if !text.is_empty() {
            messages.push(json!({"role":"system","content": text}));
        }
    }
    let incoming = body
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| anyhow::anyhow!("messages must be an array"))?;
    for m in incoming {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        if role == "system" {
            continue;
        }
        let content = m
            .get("content")
            .ok_or_else(|| anyhow::anyhow!("each message needs content"))?;
        let text = content_text(content)?;
        let role = if role == "assistant" { "assistant" } else { "user" };
        messages.push(json!({"role": role, "content": text}));
    }
    if messages.is_empty() {
        anyhow::bail!("messages must not be empty");
    }
    let mut out = json!({
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false),
    });
    if let Some(t) = body.get("temperature") {
        out["temperature"] = t.clone();
    }
    if let Some(t) = body.get("top_p") {
        out["top_p"] = t.clone();
    }
    Ok(out)
}

pub fn anthropic_from_openai_completion(openai: &Value, model: &str) -> Value {
    let text = openai
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let id = openai.get("id").and_then(|i| i.as_str()).unwrap_or("msg_local");
    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": text}],
        "model": model,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": openai.pointer("/usage/prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": openai.pointer("/usage/completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })
}

fn anthropic_sse(text: &str, model: &str) -> String {
    let msg_id = format!("msg_{}", chrono_like_id());
    let mut out = String::new();
    out.push_str(&format!(
        "event: message_start\ndata: {}\n\n",
        json!({
            "type": "message_start",
            "message": {
                "id": msg_id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        })
    ));
    out.push_str(&format!(
        "event: content_block_start\ndata: {}\n\n",
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})
    ));
    out.push_str(&format!(
        "event: content_block_delta\ndata: {}\n\n",
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text": text}})
    ));
    out.push_str(&format!(
        "event: content_block_stop\ndata: {}\n\n",
        json!({"type":"content_block_stop","index":0})
    ));
    out.push_str(&format!(
        "event: message_delta\ndata: {}\n\n",
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":0}})
    ));
    out.push_str(&format!(
        "event: message_stop\ndata: {}\n\n",
        json!({"type":"message_stop"})
    ));
    out
}

fn chrono_like_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

pub async fn messages(State(gw): State<Shared>, Json(body): Json<Value>) -> Response {
    if !gw.cfg.server.has_api_style("claude") {
        return ApiError::new(StatusCode::NOT_FOUND, "claude API style is not enabled").into_response();
    }
    let openai = match openai_from_anthropic(&body) {
        Ok(v) => v,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let stream = openai.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let model = gw.cfg.name.clone();
    if stream {
        match collect_openai_stream(&gw, openai).await {
            Ok((text, _)) => {
                let body = anthropic_sse(&text, &model);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header(header::CACHE_CONTROL, "no-cache")
                    .body(Body::from(body))
                    .unwrap_or_else(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
            }
            Err(e) => e.into_response(),
        }
    } else {
        match chat_upstream::chat_completions(&gw, openai).await {
            Ok((status, bytes)) => {
                if !status.is_success() {
                    return Response::builder()
                        .status(status)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(bytes))
                        .unwrap_or_else(|e| {
                            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                        });
                }
                let openai_json: Value = serde_json::from_slice(&bytes).unwrap_or(json!({}));
                Json(anthropic_from_openai_completion(&openai_json, &model)).into_response()
            }
            Err(e) => e.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_system_and_messages() {
        let body = json!({
            "model": "claude",
            "max_tokens": 64,
            "system": "be brief",
            "messages": [{"role":"user","content":"hi"}]
        });
        let o = openai_from_anthropic(&body).unwrap();
        assert_eq!(o["messages"][0]["role"], "system");
        assert_eq!(o["messages"][1]["content"], "hi");
        assert_eq!(o["max_tokens"], 64);
    }
}

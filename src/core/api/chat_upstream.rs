//! Call the local llama-server OpenAI Chat Completions endpoint.

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::core::gateway::{ApiError, Gw, Slot};

/// POST `/v1/chat/completions` on the chat engine. Returns status + full body bytes.
pub async fn chat_completions(gw: &Gw, mut body: Value) -> Result<(StatusCode, Vec<u8>), ApiError> {
    let Some(port) = gw.running_port(Slot::Chat) else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the chat model is still loading",
        ));
    };
    body["model"] = json!(gw.cfg.name);
    let r = gw
        .http
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = r
        .bytes()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok((status, bytes.to_vec()))
}

/// Open a streaming chat completion; returns a byte stream of the upstream SSE body.
pub async fn chat_completions_raw_stream(
    gw: &Gw,
    mut body: Value,
) -> Result<impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>>, ApiError> {
    let Some(port) = gw.running_port(Slot::Chat) else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the chat model is still loading",
        ));
    };
    body["model"] = json!(gw.cfg.name);
    body["stream"] = json!(true);
    let r = gw
        .http
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    if !r.status().is_success() {
        let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let t = r.text().await.unwrap_or_default();
        return Err(ApiError::new(status, t));
    }
    Ok(r.bytes_stream())
}

/// Collect OpenAI SSE `data:` payloads (excluding `[DONE]`) into chat.completion.chunk JSON values,
/// and return the concatenated assistant text.
pub async fn collect_openai_stream(gw: &Gw, body: Value) -> Result<(String, Vec<Value>), ApiError> {
    let stream = chat_completions_raw_stream(gw, body).await?;
    let mut buf = String::new();
    let mut text = String::new();
    let mut chunks = Vec::new();
    tokio::pin!(stream);
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(i) = buf.find('\n') {
            let mut line = buf[..i].to_string();
            buf.drain(..=i);
            if line.ends_with('\r') {
                line.pop();
            }
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(delta) = v.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
                    text.push_str(delta);
                }
                chunks.push(v);
            }
        }
    }
    Ok((text, chunks))
}

/// Parse SSE `data:` lines from a complete buffer.
pub fn parse_sse_data(buf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in buf.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if !data.is_empty() {
                out.push(data.to_string());
            }
        }
    }
    out
}

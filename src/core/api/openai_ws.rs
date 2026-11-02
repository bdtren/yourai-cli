//! OpenAI-shaped chat over WebSocket at `/v1/chat/ws`.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::chat_upstream::{self, collect_openai_stream, parse_sse_data};
use crate::core::gateway::{ApiError, Shared};

pub async fn chat_ws(ws: WebSocketUpgrade, State(gw): State<Shared>) -> Response {
    if !gw.cfg.server.has_api_style("openai") {
        return ApiError::new(StatusCode::NOT_FOUND, "openai API style is not enabled").into_response();
    }
    ws.on_upgrade(move |socket| handle(socket, gw))
}

async fn handle(mut socket: WebSocket, gw: Shared) {
    while let Some(Ok(msg)) = socket.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            Message::Ping(p) => {
                let _ = socket.send(Message::Pong(p)).await;
                continue;
            }
            _ => continue,
        };
        let body: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                let _ = socket
                    .send(Message::Text(
                        json!({"error":{"message": e.to_string(), "type":"invalid_request_error"}})
                            .to_string()
                            .into(),
                    ))
                    .await;
                break;
            }
        };
        let stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(true);
        if stream {
            match stream_to_ws(&gw, body, &mut socket).await {
                Ok(()) => {}
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            json!({"error":{"message": e.1, "type":"server_error"}})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    break;
                }
            }
        } else {
            match chat_upstream::chat_completions(&gw, body).await {
                Ok((status, bytes)) => {
                    let payload = if status.is_success() {
                        String::from_utf8_lossy(&bytes).into_owned()
                    } else {
                        json!({"error":{"message": String::from_utf8_lossy(&bytes), "type":"server_error"}}).to_string()
                    };
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            json!({"error":{"message": e.1, "type":"server_error"}})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    break;
                }
            }
        }
    }
}

async fn stream_to_ws(gw: &Shared, body: Value, socket: &mut WebSocket) -> Result<(), ApiError> {
    // Prefer true streaming when we can; fall back to collect+chunk frames.
    match chat_upstream::chat_completions_raw_stream(gw, body.clone()).await {
        Ok(byte_stream) => {
            let mut buf = String::new();
            tokio::pin!(byte_stream);
            while let Some(item) = byte_stream.next().await {
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
                    if data.is_empty() {
                        continue;
                    }
                    if socket.send(Message::Text(data.to_string().into())).await.is_err() {
                        return Ok(());
                    }
                }
            }
            for data in parse_sse_data(&buf) {
                if socket.send(Message::Text(data.into())).await.is_err() {
                    return Ok(());
                }
            }
            Ok(())
        }
        Err(_) => {
            let (text, chunks) = collect_openai_stream(gw, body).await?;
            if chunks.is_empty() {
                let fake = json!({
                    "id": "chatcmpl-local",
                    "object": "chat.completion.chunk",
                    "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
                });
                let _ = socket.send(Message::Text(fake.to_string().into())).await;
            } else {
                for c in chunks {
                    if socket.send(Message::Text(c.to_string().into())).await.is_err() {
                        return Ok(());
                    }
                }
            }
            let _ = socket.send(Message::Text("[DONE]".into())).await;
            Ok(())
        }
    }
}

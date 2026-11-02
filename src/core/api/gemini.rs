//! Gemini-shaped generateContent under `/v1/models/{model}:…`.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use super::chat_upstream::{self, collect_openai_stream};
use crate::core::gateway::{ApiError, Shared};

pub fn openai_from_gemini(body: &Value) -> anyhow::Result<Value> {
    let mut messages = Vec::new();
    if let Some(sys) = body.get("systemInstruction").or_else(|| body.get("system_instruction")) {
        let text = parts_text(sys.get("parts").unwrap_or(sys))?;
        if !text.is_empty() {
            messages.push(json!({"role":"system","content": text}));
        }
    }
    let contents = body
        .get("contents")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow::anyhow!("contents must be an array"))?;
    for c in contents {
        let role = c.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let text = parts_text(c.get("parts").unwrap_or(&Value::Null))?;
        let role = match role {
            "model" | "assistant" => "assistant",
            _ => "user",
        };
        messages.push(json!({"role": role, "content": text}));
    }
    if messages.is_empty() {
        anyhow::bail!("contents must not be empty");
    }
    let mut out = json!({"messages": messages, "stream": false});
    if let Some(cfg) = body.get("generationConfig").or_else(|| body.get("generation_config")) {
        if let Some(t) = cfg.get("temperature") {
            out["temperature"] = t.clone();
        }
        if let Some(t) = cfg.get("topP").or_else(|| cfg.get("top_p")) {
            out["top_p"] = t.clone();
        }
        if let Some(t) = cfg.get("maxOutputTokens").or_else(|| cfg.get("max_output_tokens")) {
            out["max_tokens"] = t.clone();
        }
    }
    Ok(out)
}

fn parts_text(parts: &Value) -> anyhow::Result<String> {
    match parts {
        Value::Array(arr) => {
            let mut out = String::new();
            for p in arr {
                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                } else if p.as_object().is_some_and(|o| o.keys().any(|k| k != "text")) {
                    anyhow::bail!("only text parts are supported in this adapter");
                }
            }
            Ok(out)
        }
        Value::String(s) => Ok(s.clone()),
        Value::Null => Ok(String::new()),
        _ => anyhow::bail!("parts must be an array"),
    }
}

pub fn gemini_from_openai_completion(openai: &Value) -> Value {
    let text = openai
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP",
            "index": 0
        }]
    })
}

fn gemini_sse(text: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": text}]},
                "finishReason": "STOP",
                "index": 0
            }]
        })
    )
}

/// `path` is everything after `/v1/models/`, e.g. `local:generateContent`.
pub async fn generate(State(gw): State<Shared>, Path(path): Path<String>, Json(body): Json<Value>) -> Response {
    if !gw.cfg.server.has_api_style("gemini") {
        return ApiError::new(StatusCode::NOT_FOUND, "gemini API style is not enabled").into_response();
    }
    let stream = if path.ends_with(":streamGenerateContent") {
        true
    } else if path.ends_with(":generateContent") {
        false
    } else {
        return ApiError::new(
            StatusCode::NOT_FOUND,
            "use /v1/models/{model}:generateContent or :streamGenerateContent",
        )
        .into_response();
    };
    let mut openai = match openai_from_gemini(&body) {
        Ok(v) => v,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    if stream {
        match collect_openai_stream(&gw, openai).await {
            Ok((text, _)) => {
                let body = gemini_sse(&text);
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
        openai["stream"] = json!(false);
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
                Json(gemini_from_openai_completion(&openai_json)).into_response()
            }
            Err(e) => e.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_contents_and_system() {
        let body = json!({
            "systemInstruction": {"parts":[{"text":"sys"}]},
            "contents": [{"role":"user","parts":[{"text":"hi"}]}],
            "generationConfig": {"temperature": 0.2, "maxOutputTokens": 32}
        });
        let o = openai_from_gemini(&body).unwrap();
        assert_eq!(o["messages"][0]["content"], "sys");
        assert_eq!(o["messages"][1]["content"], "hi");
        assert_eq!(o["temperature"], 0.2);
        assert_eq!(o["max_tokens"], 32);
    }
}

//! Run project custom route handlers (JSON stdin/stdout, or streamed stdout).

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::stream::unfold;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};
use tokio::time::Instant;

use crate::core::gateway::ApiError;
use crate::core::paths;

#[derive(Debug, Serialize)]
struct HandlerRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    query: String,
    body_b64: String,
}

#[derive(Debug, Deserialize)]
struct HandlerResponse {
    status: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body_b64: String,
}

/// Options for a custom handler route from `api/routes.toml`.
#[derive(Debug, Clone)]
pub struct InvokeOpts {
    pub timeout_ms: u64,
    pub stream: bool,
    pub content_type: String,
}

impl Default for InvokeOpts {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            stream: false,
            content_type: "text/event-stream".into(),
        }
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "exe" | "bat" | "cmd" | "ps1"))
    }
}

fn is_js(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "js" | "mjs" | "cjs"))
}

/// Resolve how to spawn the handler script.
pub fn command_for(script: &Path) -> anyhow::Result<(String, Vec<String>)> {
    if !script.is_file() {
        anyhow::bail!("handler not found: {}", script.display());
    }
    if is_executable(script) {
        return Ok((script.display().to_string(), vec![]));
    }
    if is_js(script) {
        return Ok(("node".into(), vec![script.display().to_string()]));
    }
    anyhow::bail!(
        "handler {} is not executable and is not a .js/.mjs file (make it executable or use Node)",
        script.display()
    );
}

async fn run_script(script: &Path, timeout_ms: u64, req: &HandlerRequest) -> anyhow::Result<HandlerResponse> {
    let (program, args) = command_for(script)?;
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| spawn_err(&program, e))?;

    let input = serde_json::to_vec(req)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&input).await?;
        stdin.shutdown().await?;
    }

    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("handler timed out after {timeout_ms}ms"))?
        .map_err(|e| anyhow::anyhow!("handler failed: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("handler exited {}: {}", output.status.code().unwrap_or(-1), err.trim());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("handler produced no stdout"))?;
    serde_json::from_str(line).map_err(|e| anyhow::anyhow!("handler stdout is not valid JSON: {e}"))
}

fn spawn_err(program: &str, e: std::io::Error) -> anyhow::Error {
    if program == "node" && e.kind() == std::io::ErrorKind::NotFound {
        anyhow::anyhow!("node is not on PATH; install Node.js or use an executable handler script")
    } else {
        anyhow::anyhow!("failed to start handler {program}: {e}")
    }
}

fn headers_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(s) = v.to_str() {
            out.insert(k.as_str().to_string(), s.to_string());
        }
    }
    out
}

async fn request_from_http(req: Request) -> Result<HandlerRequest, Response> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let headers = headers_map(req.headers());
    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, format!("read body: {e}")).into_response());
        }
    };
    Ok(HandlerRequest {
        method: method.to_string(),
        path,
        headers,
        query,
        body_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// Invoke a project handler for an HTTP request (buffered or streamed).
pub async fn invoke(root: &Path, handler_rel: &str, opts: InvokeOpts, req: Request) -> Response {
    let script = match paths::inside(root, handler_rel) {
        Ok(p) => p,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let hr = match request_from_http(req).await {
        Ok(h) => h,
        Err(r) => return r,
    };
    if opts.stream {
        invoke_stream(&script, &opts, &hr).await
    } else {
        match run_script(&script, opts.timeout_ms, &hr).await {
            Ok(resp) => response_from(resp),
            Err(e) => ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
        }
    }
}

/// Stream raw stdout as the HTTP body (`stream = true` routes).
async fn invoke_stream(script: &Path, opts: &InvokeOpts, req: &HandlerRequest) -> Response {
    let (program, args) = match command_for(script) {
        Ok(c) => c,
        Err(e) => return ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };
    let mut child = match Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ApiError::new(StatusCode::BAD_GATEWAY, spawn_err(&program, e).to_string()).into_response(),
    };

    let input = match serde_json::to_vec(req) {
        Ok(b) => b,
        Err(e) => return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(&input).await {
            return ApiError::new(StatusCode::BAD_GATEWAY, format!("handler stdin: {e}")).into_response();
        }
        let _ = stdin.shutdown().await;
    }

    let mut stdout = match child.stdout.take() {
        Some(s) => s,
        None => return ApiError::new(StatusCode::BAD_GATEWAY, "handler has no stdout").into_response(),
    };
    let stderr = child.stderr.take();

    // Peek first byte so empty+failed scripts become 502 instead of an empty 200 stream.
    let mut first = [0u8; 1];
    let first_read = tokio::time::timeout(Duration::from_millis(opts.timeout_ms), stdout.read(&mut first)).await;
    let first_n = match first_read {
        Ok(Ok(0)) => {
            // No stdout — wait for exit and surface stderr.
            let status = child.wait().await;
            let err = if let Some(mut err) = stderr {
                let mut buf = Vec::new();
                let _ = err.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).trim().to_string()
            } else {
                String::new()
            };
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            let msg = if err.is_empty() {
                format!("handler exited {code} with no stdout")
            } else {
                format!("handler exited {code}: {err}")
            };
            return ApiError::new(StatusCode::BAD_GATEWAY, msg).into_response();
        }
        Ok(Ok(_)) => 1usize,
        Ok(Err(e)) => {
            return ApiError::new(StatusCode::BAD_GATEWAY, format!("handler stdout: {e}")).into_response();
        }
        Err(_) => {
            let _ = child.kill().await;
            return ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("handler timed out after {}ms before producing output", opts.timeout_ms),
            )
            .into_response();
        }
    };

    let content_type = opts.content_type.clone();
    let deadline = Instant::now() + Duration::from_millis(opts.timeout_ms);
    struct StreamState {
        first: Option<Bytes>,
        stdout: ChildStdout,
        child: Child,
        deadline: Instant,
    }
    let state = StreamState {
        first: Some(Bytes::copy_from_slice(&first[..first_n])),
        stdout,
        child,
        deadline,
    };
    let stream = unfold(Some(state), |state| async move {
        let mut state = state?;
        if let Some(chunk) = state.first.take() {
            return Some((Ok::<_, std::io::Error>(chunk), Some(state)));
        }
        let left = state.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let _ = state.child.kill().await;
            let _ = state.child.wait().await;
            return None;
        }
        let mut buf = vec![0u8; 8192];
        match tokio::time::timeout(left, state.stdout.read(&mut buf)).await {
            Ok(Ok(0)) => {
                let _ = state.child.wait().await;
                None
            }
            Ok(Ok(n)) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), Some(state)))
            }
            Ok(Err(e)) => {
                let _ = state.child.kill().await;
                let _ = state.child.wait().await;
                Some((Err(e), None))
            }
            Err(_) => {
                let _ = state.child.kill().await;
                let _ = state.child.wait().await;
                None
            }
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
}

fn response_from(resp: HandlerResponse) -> Response {
    let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = match base64::engine::general_purpose::STANDARD.decode(resp.body_b64.as_bytes()) {
        Ok(b) => b,
        Err(e) => {
            return ApiError::new(StatusCode::BAD_GATEWAY, format!("handler body_b64 is invalid: {e}")).into_response();
        }
    };
    let mut builder = Response::builder().status(status);
    for (k, v) in resp.headers {
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(&v)) {
            if name != header::CONTENT_LENGTH {
                builder = builder.header(name, val);
            }
        }
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
}

/// Build a small Node echo fixture for tests (works on Windows without chmod).
#[cfg(test)]
pub fn write_echo_js_fixture(path: &Path) {
    let body = r#"const chunks = [];
process.stdin.on("data", (c) => chunks.push(c));
process.stdin.on("end", () => {
  const req = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  const raw = Buffer.from(req.body_b64 || "", "base64").toString("utf8");
  const out = {
    status: 200,
    headers: { "content-type": "application/json" },
    body_b64: Buffer.from(JSON.stringify({ echo: raw, path: req.path })).toString("base64"),
  };
  process.stdout.write(JSON.stringify(out) + "\n");
});
"#;
    std::fs::write(path, body).unwrap();
}

/// Streaming SSE echo fixture for tests.
#[cfg(test)]
pub fn write_stream_js_fixture(path: &Path) {
    let body = r#"const chunks = [];
process.stdin.on("data", (c) => chunks.push(c));
process.stdin.on("end", () => {
  const req = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  const raw = Buffer.from(req.body_b64 || "", "base64").toString("utf8");
  process.stdout.write("data: one\n\n");
  process.stdout.write("data: " + raw + "\n\n");
  process.stdout.write("data: [DONE]\n\n");
});
"#;
    std::fs::write(path, body).unwrap();
}

/// Build a chmod + shebang echo fixture (Unix only).
#[cfg(all(test, unix))]
pub fn write_echo_executable_fixture(path: &Path) {
    let body = r#"#!/usr/bin/env python3
import json, sys, base64
req = json.load(sys.stdin)
raw = base64.b64decode(req.get("body_b64") or "")
out = {
  "status": 200,
  "headers": {"content-type": "application/json"},
  "body_b64": base64.b64encode(json.dumps({"echo": raw.decode("utf-8", "replace"), "path": req["path"]}).encode()).decode(),
}
print(json.dumps(out))
"#;
    std::fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req() -> HandlerRequest {
        HandlerRequest {
            method: "POST".into(),
            path: "/v2/echo".into(),
            headers: HashMap::new(),
            query: String::new(),
            body_b64: base64::engine::general_purpose::STANDARD.encode(b"hi"),
        }
    }

    fn assert_echo(resp: HandlerResponse) {
        assert_eq!(resp.status, 200);
        let body = base64::engine::general_purpose::STANDARD
            .decode(resp.body_b64.as_bytes())
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["echo"], "hi");
        assert_eq!(v["path"], "/v2/echo");
    }

    #[tokio::test]
    async fn js_handler_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("echo.js");
        write_echo_js_fixture(&script);
        let resp = run_script(&script, 10_000, &sample_req()).await.unwrap();
        assert_echo(resp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn executable_handler_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("echo.py");
        write_echo_executable_fixture(&script);
        let resp = run_script(&script, 10_000, &sample_req()).await.unwrap();
        assert_echo(resp);
    }

    #[tokio::test]
    async fn stream_handler_yields_sse_chunks() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("stream.js");
        write_stream_js_fixture(&script);
        let opts = InvokeOpts {
            timeout_ms: 10_000,
            stream: true,
            content_type: "text/event-stream".into(),
        };
        let resp = invoke_stream(&script, &opts, &sample_req()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("data: one\n\n"));
        assert!(text.contains("data: hi\n\n"));
        assert!(text.contains("data: [DONE]\n\n"));
    }

    #[test]
    fn command_for_js_uses_node() {
        let tmp = tempfile::tempdir().unwrap();
        let js = tmp.path().join("h.js");
        std::fs::write(&js, "x").unwrap();
        let (prog, args) = command_for(&js).unwrap();
        assert_eq!(prog, "node");
        assert_eq!(args, vec![js.display().to_string()]);
    }
}

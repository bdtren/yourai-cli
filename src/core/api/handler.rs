//! Run project custom route handlers (JSON stdin/stdout).

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

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
        .map_err(|e| {
            if program == "node" && e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("node is not on PATH; install Node.js or use an executable handler script")
            } else {
                anyhow::anyhow!("failed to start handler {program}: {e}")
            }
        })?;

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

fn headers_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(s) = v.to_str() {
            out.insert(k.as_str().to_string(), s.to_string());
        }
    }
    out
}

/// Invoke a project handler for an HTTP request.
pub async fn invoke(root: &Path, handler_rel: &str, timeout_ms: u64, req: Request) -> Response {
    let script = match paths::inside(root, handler_rel) {
        Ok(p) => p,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let headers = headers_map(req.headers());
    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return ApiError::new(StatusCode::BAD_REQUEST, format!("read body: {e}")).into_response();
        }
    };
    let hr = HandlerRequest {
        method: method.to_string(),
        path,
        headers,
        query,
        body_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    };
    match run_script(&script, timeout_ms, &hr).await {
        Ok(resp) => response_from(resp),
        Err(e) => ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
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
    let body = r#"const fs = require("fs");
const chunks = [];
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

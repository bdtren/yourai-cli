//! The project's single address. Runs as `yourai __gateway`, started by `yourai start`.
//!
//! The chat engine (llama-server) always runs. Image, video, and speech engines start on
//! first use and stop after `server.idle_unload_minutes` without requests. Every engine
//! listens on 127.0.0.1; only the gateway is reachable from outside, and it checks the
//! API key itself.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::config::Config;
use super::engine::{self, Bind};
use super::lock::{Lock, ModelLock};
use super::ollama;
use super::project::Project;
use super::store;
use super::{paths, process, secrets};

/// One engine process the gateway manages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Chat,
    Image,
    Video,
    SpeechToText,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::Chat => "chat",
            Slot::Image => "image",
            Slot::Video => "video",
            Slot::SpeechToText => "speech_to_text",
        }
    }

    /// Path that answers 200 once the engine has loaded its model.
    fn ready_path(self) -> &'static str {
        match self {
            Slot::Image | Slot::Video => "/v1/models",
            _ => "/health",
        }
    }
}

struct Proc {
    pid: u32,
    started: Option<u64>,
    port: u16,
    last_used: Instant,
}

/// Engine pids written to `.yourai/engines.json`, so `yourai stop` can clean up even
/// if the gateway itself was killed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRecord {
    pub pid: u32,
    pub started: Option<u64>,
    pub port: u16,
}

pub(crate) struct Gw {
    pub(crate) root: PathBuf,
    pub(crate) state_dir: PathBuf,
    pub(crate) outputs: PathBuf,
    /// When set, GET `/` and static assets are served from this project folder.
    pub(crate) ui_dir: Option<PathBuf>,
    /// When set, public chat mounts come from `api/routes.toml` instead of api_styles defaults.
    pub(crate) route_table: Option<super::api::routes::RoutesFile>,
    pub(crate) cfg: Config,
    pub(crate) lock: Lock,
    pub(crate) api_key: Option<String>,
    pub(crate) key_file: Option<PathBuf>,
    pub(crate) http: reqwest::Client,
    procs: Mutex<BTreeMap<Slot, Proc>>,
    /// sd-server handles one job at a time, and image and video share it.
    sd: tokio::sync::Mutex<()>,
    stt: tokio::sync::Mutex<()>,
    tts: tokio::sync::Mutex<()>,
}

pub(crate) type Shared = Arc<Gw>;

/// An error response in the OpenAI shape: `{"error": {"message", "type"}}`.
#[derive(Debug)]
pub struct ApiError(pub(crate) StatusCode, pub(crate) String);

impl ApiError {
    pub(crate) fn new(code: StatusCode, msg: impl Into<String>) -> Self {
        ApiError(code, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let kind = if self.0.is_client_error() {
            "invalid_request_error"
        } else {
            "server_error"
        };
        (self.0, Json(json!({"error": {"message": self.1, "type": kind}}))).into_response()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    eprintln!("[yourai gateway] {}", msg.as_ref());
}

/// Which requests need the API key: the APIs, generated files, and anything that is
/// not a plain GET (the browser chat page itself stays reachable).
pub fn needs_key(method: &Method, path: &str) -> bool {
    if path == "/health" {
        return false;
    }
    path.starts_with("/v1/") || path.starts_with("/files/") || path.starts_with("/yourai/") || method != Method::GET
}

fn const_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn key_matches(headers: &HeaderMap, key: &str) -> bool {
    if let Some(given) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        if const_eq(given.trim().as_bytes(), key.as_bytes()) {
            return true;
        }
    }
    if let Some(given) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return const_eq(given.trim().as_bytes(), key.as_bytes());
    }
    false
}

async fn auth(State(gw): State<Shared>, req: Request, next: axum::middleware::Next) -> Response {
    if let Some(key) = &gw.api_key {
        if needs_key(req.method(), req.uri().path()) && !key_matches(req.headers(), key) {
            return ApiError::new(
                StatusCode::UNAUTHORIZED,
                "missing or wrong API key: send `Authorization: Bearer <key>` or `x-api-key`",
            )
            .into_response();
        }
    }
    next.run(req).await
}

impl Gw {
    fn resolve(&self, stored: &str) -> Result<PathBuf, ApiError> {
        store::resolve_lock_path(&self.root, stored)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    fn resolved_files(&self, m: &ModelLock) -> Result<std::collections::BTreeMap<String, String>, ApiError> {
        m.files
            .iter()
            .map(|(k, v)| Ok((k.clone(), self.resolve(v)?.display().to_string())))
            .collect()
    }

    fn model(&self, slot: Slot) -> Result<&ModelLock, ApiError> {
        let key = match slot {
            Slot::Image => "image",
            Slot::Video => "video",
            Slot::SpeechToText => "speech_to_text",
            Slot::Chat => unreachable!(),
        };
        self.lock.models.get(key).ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                format!("{key} is not enabled in this project: run `yourai config set {key}.enabled true` and `yourai rebuild`"),
            )
        })
    }

    fn engine_exe(&self, name: &str) -> Result<PathBuf, ApiError> {
        let Some(e) = self.lock.engines.get(name) else {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("{name} is not installed: run `yourai rebuild`"),
            ));
        };
        let p = self.resolve(&e.exe)?;
        if p.is_file() {
            Ok(p)
        } else {
            Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("{name} is not installed: run `yourai rebuild`"),
            ))
        }
    }

    fn chat_uses_ollama(&self) -> bool {
        self.cfg.runtime.engine == "ollama"
    }

    fn ollama_models_path(&self) -> PathBuf {
        if self.cfg.store.project_modules {
            ollama::ollama_models_dir(&self.root.join(store::MODULE_DIR))
        } else {
            ollama::ollama_models_dir(&paths::home())
        }
    }

    fn command(&self, slot: Slot, port: u16) -> Result<(PathBuf, Vec<String>), ApiError> {
        let threads = self.cfg.runtime.threads;
        Ok(match slot {
            Slot::Chat if self.chat_uses_ollama() => {
                let exe = PathBuf::from(&self.lock.engine_exe);
                let exe = if exe.is_file() {
                    exe
                } else {
                    ollama::find_binary().map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?
                };
                (exe, ollama::serve_args())
            }
            Slot::Chat => {
                let mmproj = if self.lock.mmproj_path.is_empty() {
                    None
                } else {
                    Some(self.resolve(&self.lock.mmproj_path)?)
                };
                let model = self.resolve(&self.lock.model_path)?;
                (
                    self.resolve(&self.lock.engine_exe)?,
                    engine::llama_args(
                        &self.cfg,
                        &model,
                        mmproj.as_deref(),
                        Bind {
                            host: "127.0.0.1",
                            port,
                        },
                        self.key_file.as_deref(),
                        self.ui_dir.is_some(),
                    ),
                )
            }
            Slot::Image | Slot::Video => {
                let m = self.model(slot)?;
                let files = self.resolved_files(m)?;
                let extra = engine::sd_server_args(&m.defaults);
                (
                    self.engine_exe(super::catalog::SD)?,
                    engine::sd_args(&files, &m.main, port, threads, &extra),
                )
            }
            Slot::SpeechToText => {
                let m = self.model(slot)?;
                let path = m.main_path().unwrap_or_default();
                let path = self.resolve(path)?;
                (
                    self.engine_exe(super::catalog::WHISPER)?,
                    engine::whisper_args(&path, port, threads, &self.cfg.speech_to_text.language),
                )
            }
        })
    }

    fn log_file(&self, slot: Slot) -> PathBuf {
        match slot {
            Slot::Chat => self.state_dir.join("server.log"),
            other => self.state_dir.join(format!("{}.log", other.name())),
        }
    }

    fn save_records(&self) {
        let recs: BTreeMap<&str, EngineRecord> = self
            .procs
            .lock()
            .unwrap()
            .iter()
            .map(|(s, p)| {
                (
                    s.name(),
                    EngineRecord {
                        pid: p.pid,
                        started: p.started,
                        port: p.port,
                    },
                )
            })
            .collect();
        let _ = std::fs::write(
            self.state_dir.join("engines.json"),
            serde_json::to_string_pretty(&recs).unwrap_or_default(),
        );
    }

    pub(crate) fn running_port(&self, slot: Slot) -> Option<u16> {
        let mut procs = self.procs.lock().unwrap();
        let alive = procs.get(&slot).is_some_and(|p| process::is_ours(p.pid, p.started));
        if !alive {
            procs.remove(&slot);
            return None;
        }
        let p = procs.get_mut(&slot).unwrap();
        p.last_used = Instant::now();
        Some(p.port)
    }

    async fn stop(&self, slot: Slot) {
        let p = self.procs.lock().unwrap().remove(&slot);
        if let Some(p) = p {
            log(format!("stopping {} engine (pid {})", slot.name(), p.pid));
            let _ = tokio::task::spawn_blocking(move || process::stop(p.pid)).await;
            self.save_records();
        }
    }

    /// Starts the engine for `slot` if needed and waits until it has loaded its model.
    async fn ensure(&self, slot: Slot) -> Result<u16, ApiError> {
        if let Some(port) = self.running_port(slot) {
            return Ok(port);
        }
        let other = match slot {
            Slot::Image => Some(Slot::Video),
            Slot::Video => Some(Slot::Image),
            _ => None,
        };
        if let Some(o) = other {
            self.stop(o).await;
        }
        let port = process::next_free_port("127.0.0.1", 39100)
            .ok_or_else(|| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "no free local port for the engine"))?;
        let (exe, args) = self.command(slot, port)?;
        log(format!("starting {} engine on 127.0.0.1:{port}", slot.name()));
        let env = if slot == Slot::Chat && self.chat_uses_ollama() {
            ollama::serve_env("127.0.0.1", port, &self.ollama_models_path())
        } else {
            Vec::new()
        };
        let pid = process::spawn_detached_env(&exe, &args, &self.log_file(slot), &self.root, &env).map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not start the engine: {e}"),
            )
        })?;
        let started = process::start_time(pid);
        self.procs.lock().unwrap().insert(
            slot,
            Proc {
                pid,
                started,
                port,
                last_used: Instant::now(),
            },
        );
        self.save_records();

        if slot == Slot::Chat && self.chat_uses_ollama() {
            let bin = exe.clone();
            let models = self.ollama_models_path();
            let import = tokio::task::spawn_blocking(move || {
                ollama::ensure_model_imported(&bin, &models, "127.0.0.1", port)
            })
            .await
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e.to_string()));
            if let Err(e) = import {
                self.stop(slot).await;
                return Err(e);
            }
        } else {
            let url = format!("http://127.0.0.1:{port}{}", slot.ready_path());
            let deadline = Instant::now() + Duration::from_secs(900);
            loop {
                if !process::is_ours(pid, started) {
                    self.procs.lock().unwrap().remove(&slot);
                    self.save_records();
                    return Err(ApiError::new(
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "the {} engine stopped while loading its model; see {}",
                            slot.name(),
                            self.log_file(slot).display()
                        ),
                    ));
                }
                if let Ok(r) = self.http.get(&url).timeout(Duration::from_secs(3)).send().await {
                    if r.status().is_success() {
                        break;
                    }
                }
                if Instant::now() > deadline {
                    self.stop(slot).await;
                    return Err(ApiError::new(
                        StatusCode::GATEWAY_TIMEOUT,
                        format!("the {} engine did not load its model in 15 minutes", slot.name()),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        log(format!("{} engine ready", slot.name()));
        if let Some(p) = self.procs.lock().unwrap().get_mut(&slot) {
            p.last_used = Instant::now();
        }
        Ok(port)
    }

    fn touch(&self, slot: Slot) {
        if let Some(p) = self.procs.lock().unwrap().get_mut(&slot) {
            p.last_used = Instant::now();
        }
    }

    /// Saves generated bytes under outputs/<dir>/ and returns the project-relative path.
    fn save_output(&self, dir: &str, stem: &str, ext: &str, bytes: &[u8]) -> Result<String, ApiError> {
        let folder = self.outputs.join(dir);
        std::fs::create_dir_all(&folder).map_err(internal)?;
        let mut name = format!("{stem}-{}.{ext}", now_secs());
        let mut n = 1;
        while folder.join(&name).exists() {
            n += 1;
            name = format!("{stem}-{}-{n}.{ext}", now_secs());
        }
        std::fs::write(folder.join(&name), bytes).map_err(internal)?;
        Ok(format!("{dir}/{name}"))
    }

    /// Submits an sd-server job and waits for its result.
    async fn sd_job(&self, slot: Slot, port: u16, path: &str, body: Value) -> Result<Value, ApiError> {
        let base = format!("http://127.0.0.1:{port}");
        let r = self
            .http
            .post(format!("{base}{path}"))
            .json(&body)
            .send()
            .await
            .map_err(upstream)?;
        let status = r.status();
        let job: Value = r.json().await.map_err(upstream)?;
        if !status.is_success() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("the engine rejected the request: {}", error_text(&job)),
            ));
        }
        let poll = job
            .get("poll_url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                job.get("id")
                    .and_then(Value::as_str)
                    .map(|id| format!("/sdcpp/v1/jobs/{id}"))
            })
            .ok_or_else(|| ApiError::new(StatusCode::BAD_GATEWAY, "the engine returned no job id"))?;
        loop {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            self.touch(slot);
            let j: Value = self
                .http
                .get(format!("{base}{poll}"))
                .send()
                .await
                .map_err(upstream)?
                .json()
                .await
                .map_err(upstream)?;
            match j.get("status").and_then(Value::as_str).unwrap_or("") {
                "completed" => return Ok(j.get("result").cloned().unwrap_or(Value::Null)),
                "failed" | "cancelled" => {
                    return Err(ApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("generation failed: {}", error_text(&j)),
                    ))
                }
                _ => {}
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn upstream(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, format!("engine error: {e}"))
}

fn error_text(v: &Value) -> String {
    v.pointer("/error/message")
        .or_else(|| v.get("error"))
        .or_else(|| v.get("message"))
        .map(|e| e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string()))
        .unwrap_or_else(|| v.to_string())
}

fn slug(prompt: &str) -> String {
    let s: String = prompt
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let parts: Vec<&str> = s.split('-').filter(|p| !p.is_empty()).take(6).collect();
    if parts.is_empty() {
        "output".into()
    } else {
        parts.join("-")
    }
}

/// `"768x512"` -> (768, 512).
pub fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    let ok = |v: u32| (64..=2048).contains(&v) && v % 16 == 0;
    (ok(w) && ok(h)).then_some((w, h))
}

/// First non-zero value: the request, then yourai.toml, then the catalog default.
fn pick(req: Option<u64>, cfg: u32, defaults: &Value, key: &str, fallback: u64) -> u64 {
    req.filter(|v| *v > 0)
        .or((cfg > 0).then_some(cfg as u64))
        .or_else(|| defaults.get(key).and_then(Value::as_u64))
        .unwrap_or(fallback)
}

const MAX_INPUT_IMAGE: usize = 20 * 1024 * 1024;

/// Denoising strength for img2img / Wan i2v (default 0.75).
pub fn clamp_strength(v: Option<f64>) -> f64 {
    v.unwrap_or(0.75).clamp(0.05, 1.0)
}

fn image_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"))
}

fn mime_for_image_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

/// Turn a data URL, raw base64, or file path into a `data:image/...;base64,...` string for sd.cpp.
pub fn encode_image_input(root: &Path, image: &str) -> Result<String, ApiError> {
    let s = image.trim();
    if s.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "image must not be empty"));
    }
    let (mime, bytes) = if let Some(rest) = s.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',').ok_or_else(|| {
            ApiError::new(StatusCode::BAD_REQUEST, "image data URL is missing base64 data")
        })?;
        if !meta.contains("base64") {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "image data URL must be base64-encoded",
            ));
        }
        let mime = meta.split(';').next().unwrap_or("image/png").to_string();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("invalid image base64: {e}")))?;
        (mime, bytes)
    } else {
        let path = Path::new(s);
        let as_file = path.is_absolute()
            || s.contains('/')
            || s.contains('\\')
            || image_ext(path)
            || root.join(path).is_file();
        if as_file {
            let file = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            if !file.is_file() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("image file not found: {}", file.display()),
                ));
            }
            let bytes = std::fs::read(&file).map_err(|e| {
                ApiError::new(StatusCode::BAD_REQUEST, format!("could not read image: {e}"))
            })?;
            (mime_for_image_path(&file).to_string(), bytes)
        } else {
            let bytes = base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("image must be a file path, data URL, or base64 ({e})"),
                )
            })?;
            ("image/png".into(), bytes)
        }
    };
    if bytes.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "image is empty"));
    }
    if bytes.len() > MAX_INPUT_IMAGE {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "image is larger than 20 MB",
        ));
    }
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// JSON body for `/sdcpp/v1/img_gen` (text-to-image or img2img when `init_image` is set).
pub fn image_job_body(
    prompt: &str,
    negative_prompt: &str,
    width: u64,
    height: u64,
    seed: i64,
    n: u32,
    steps: u64,
    cfg_scale: f64,
    init_image: Option<&str>,
    strength: Option<f64>,
) -> Value {
    let mut body = json!({
        "prompt": prompt,
        "negative_prompt": negative_prompt,
        "width": width,
        "height": height,
        "seed": seed,
        "batch_count": n,
        "sample_params": {
            "sample_steps": steps,
            "guidance": {"txt_cfg": cfg_scale, "img_cfg": cfg_scale, "distilled_guidance": 3.5}
        },
        "output_format": "png"
    });
    if let Some(img) = init_image {
        body["init_image"] = json!(img);
        body["strength"] = json!(clamp_strength(strength));
    }
    body
}

#[derive(Deserialize)]
struct ImageReq {
    prompt: String,
    #[serde(default)]
    negative_prompt: String,
    n: Option<u32>,
    size: Option<String>,
    steps: Option<u64>,
    seed: Option<i64>,
    response_format: Option<String>,
}

#[derive(Deserialize)]
struct ImageEditReq {
    prompt: String,
    /// Data URL, raw base64, or path under the project (or absolute).
    image: String,
    #[serde(default)]
    negative_prompt: String,
    n: Option<u32>,
    size: Option<String>,
    steps: Option<u64>,
    seed: Option<i64>,
    strength: Option<f64>,
    response_format: Option<String>,
}

fn parse_image_size(size: Option<&str>) -> Result<Option<(u32, u32)>, ApiError> {
    match size {
        Some(s) => parse_size(s)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "size must look like 512x512 (multiples of 16, 64-2048)",
                )
            })
            .map(Some),
        None => Ok(None),
    }
}

async fn run_image_job(
    gw: &Gw,
    prompt: &str,
    negative_prompt: &str,
    size: Option<(u32, u32)>,
    steps: Option<u64>,
    seed: Option<i64>,
    n: Option<u32>,
    response_format: Option<&str>,
    init_image: Option<&str>,
    strength: Option<f64>,
) -> Result<Json<Value>, ApiError> {
    let m = gw.model(Slot::Image)?.clone();
    let d = &m.defaults;
    let c = &gw.cfg.image;
    let width = size
        .map(|s| s.0 as u64)
        .unwrap_or_else(|| pick(None, c.width, d, "width", 512));
    let height = size
        .map(|s| s.1 as u64)
        .unwrap_or_else(|| pick(None, c.height, d, "height", 512));
    let steps = pick(steps, c.steps, d, "steps", 20);
    let cfg_scale = d.get("cfg_scale").and_then(Value::as_f64).unwrap_or(7.0);
    let n = n.unwrap_or(1).clamp(1, 4);
    let body = image_job_body(
        prompt,
        negative_prompt,
        width,
        height,
        seed.unwrap_or(-1),
        n,
        steps,
        cfg_scale,
        init_image,
        strength,
    );

    let _sd = gw.sd.lock().await;
    let port = gw.ensure(Slot::Image).await?;
    let result = gw.sd_job(Slot::Image, port, "/sdcpp/v1/img_gen", body).await?;
    let mut data = Vec::new();
    for img in result.get("images").and_then(Value::as_array).into_iter().flatten() {
        let b64 = img.get("b64_json").and_then(Value::as_str).unwrap_or("");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| upstream(format!("bad image data: {e}")))?;
        let rel = gw.save_output("images", &slug(prompt), "png", &bytes)?;
        let mut item = json!({"url": format!("/files/{rel}"), "path": format!("outputs/{rel}")});
        if response_format != Some("url") {
            item["b64_json"] = Value::String(b64.to_string());
        }
        data.push(item);
    }
    if data.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "the engine returned no images"));
    }
    Ok(Json(json!({"created": now_secs(), "data": data})))
}

async fn images(State(gw): State<Shared>, Json(req): Json<ImageReq>) -> Result<Json<Value>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "prompt must not be empty"));
    }
    let size = parse_image_size(req.size.as_deref())?;
    run_image_job(
        &gw,
        &req.prompt,
        &req.negative_prompt,
        size,
        req.steps,
        req.seed,
        req.n,
        req.response_format.as_deref(),
        None,
        None,
    )
    .await
}

async fn images_edits(State(gw): State<Shared>, Json(req): Json<ImageEditReq>) -> Result<Json<Value>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "prompt must not be empty"));
    }
    if req.image.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "image is required for edits"));
    }
    let init = encode_image_input(&gw.root, &req.image)?;
    let size = parse_image_size(req.size.as_deref())?;
    run_image_job(
        &gw,
        &req.prompt,
        &req.negative_prompt,
        size,
        req.steps,
        req.seed,
        req.n,
        req.response_format.as_deref(),
        Some(&init),
        req.strength,
    )
    .await
}

#[derive(Deserialize)]
struct VideoReq {
    prompt: String,
    #[serde(default)]
    negative_prompt: String,
    size: Option<String>,
    frames: Option<u64>,
    fps: Option<u64>,
    steps: Option<u64>,
    seed: Option<i64>,
    /// Optional conditioning image (data URL, base64, or path) for image-to-video.
    image: Option<String>,
    strength: Option<f64>,
    response_format: Option<String>,
}

/// Wan-style video models want 4n+1 frames.
pub fn align_frames(n: u64) -> u64 {
    let n = n.clamp(5, 241);
    (n - 1) / 4 * 4 + 1
}

/// MiniMax-H3 wants frames on the 17k+5 grid (minimum 5).
pub fn align_frames_minimax_h3(n: u64) -> u64 {
    let n = n.clamp(5, 241);
    if n <= 5 {
        return 5;
    }
    let k = (n - 5).div_ceil(17);
    17 * k + 5
}

fn align_video_frames(n: u64, defaults: &Value) -> u64 {
    match defaults.get("frame_align").and_then(Value::as_str).unwrap_or("wan") {
        "minimax_h3" => align_frames_minimax_h3(n),
        _ => align_frames(n),
    }
}

fn align_dim(n: u64, multiple: u64) -> u64 {
    if multiple <= 1 {
        return n;
    }
    let n = n.max(multiple);
    n.div_ceil(multiple) * multiple
}

/// Build the `/sdcpp/v1/vid_gen` JSON body from request + config + catalog defaults.
/// When `init_image` is set: MiniMax uses `ref_images`; other models use `init_image` + `strength`.
#[allow(clippy::too_many_arguments)] // request + config + catalog knobs stay explicit
pub fn video_job_body(
    prompt: &str,
    negative_prompt: &str,
    seed: i64,
    req_size: Option<(u32, u32)>,
    req_frames: Option<u64>,
    req_fps: Option<u64>,
    req_steps: Option<u64>,
    cfg_width: u32,
    cfg_height: u32,
    cfg_frames: u32,
    cfg_fps: u32,
    cfg_steps: u32,
    defaults: &Value,
    init_image: Option<&str>,
    strength: Option<f64>,
) -> Value {
    let multiple = defaults
        .get("size_multiple")
        .and_then(Value::as_u64)
        .unwrap_or(16)
        .max(1);
    let width = align_dim(
        req_size
            .map(|s| s.0 as u64)
            .unwrap_or_else(|| pick(None, cfg_width, defaults, "width", 480)),
        multiple,
    );
    let height = align_dim(
        req_size
            .map(|s| s.1 as u64)
            .unwrap_or_else(|| pick(None, cfg_height, defaults, "height", 272)),
        multiple,
    );
    let frames = align_video_frames(pick(req_frames, cfg_frames, defaults, "frames", 33), defaults);
    let fps = pick(req_fps, cfg_fps, defaults, "fps", 16);
    let steps = pick(req_steps, cfg_steps, defaults, "steps", 20);
    let cfg_scale = defaults.get("cfg_scale").and_then(Value::as_f64).unwrap_or(6.0);
    let flow_shift = defaults.get("flow_shift").and_then(Value::as_f64).unwrap_or(3.0);
    let sample_method = defaults.get("sample_method").and_then(Value::as_str).unwrap_or("euler");
    let distilled = defaults
        .get("distilled_guidance")
        .and_then(Value::as_f64)
        .unwrap_or(3.5);
    let mut body = json!({
        "prompt": prompt,
        "negative_prompt": negative_prompt,
        "width": width,
        "height": height,
        "seed": seed,
        "video_frames": frames,
        "fps": fps,
        "sample_params": {
            "sample_method": sample_method,
            "sample_steps": steps,
            "flow_shift": flow_shift,
            "guidance": {
                "txt_cfg": cfg_scale,
                "img_cfg": cfg_scale,
                "distilled_guidance": distilled
            }
        },
        "output_format": "webm"
    });
    if let Some(img) = init_image {
        let minimax = defaults.get("frame_align").and_then(Value::as_str) == Some("minimax_h3");
        if minimax {
            body["ref_images"] = json!([img]);
        } else {
            body["init_image"] = json!(img);
            body["strength"] = json!(clamp_strength(strength));
        }
    }
    body
}

async fn videos(State(gw): State<Shared>, Json(req): Json<VideoReq>) -> Result<Json<Value>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "prompt must not be empty"));
    }
    let m = gw.model(Slot::Video)?.clone();
    let d = &m.defaults;
    let c = &gw.cfg.video;
    let size = match req.size.as_deref() {
        Some(s) => Some(parse_size(s).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "size must look like 480x272 (multiples of 16, 64-2048)",
            )
        })?),
        None => None,
    };
    let init = match req.image.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(img) => Some(encode_image_input(&gw.root, img)?),
        None => None,
    };
    let body = video_job_body(
        &req.prompt,
        &req.negative_prompt,
        req.seed.unwrap_or(-1),
        size,
        req.frames,
        req.fps,
        req.steps,
        c.width,
        c.height,
        c.frames,
        c.fps,
        c.steps,
        d,
        init.as_deref(),
        req.strength,
    );
    let frames = body.get("video_frames").and_then(Value::as_u64).unwrap_or(33);
    let fps = body.get("fps").and_then(Value::as_u64).unwrap_or(16);

    let _sd = gw.sd.lock().await;
    let port = gw.ensure(Slot::Video).await?;
    let result = gw.sd_job(Slot::Video, port, "/sdcpp/v1/vid_gen", body).await?;
    let b64 = result.get("b64_json").and_then(Value::as_str).unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| upstream(format!("bad video data: {e}")))?;
    if bytes.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "the engine returned no video"));
    }
    let ext = result.get("output_format").and_then(Value::as_str).unwrap_or("webm");
    let rel = gw.save_output("videos", &slug(&req.prompt), ext, &bytes)?;
    let mut item = json!({
        "url": format!("/files/{rel}"),
        "path": format!("outputs/{rel}"),
        "mime_type": result.get("mime_type").cloned().unwrap_or(json!("video/webm")),
        "frames": result.get("frame_count").cloned().unwrap_or(json!(frames)),
        "fps": result.get("fps").cloned().unwrap_or(json!(fps)),
    });
    if req.response_format.as_deref() == Some("b64_json") {
        item["b64_json"] = Value::String(b64.to_string());
    }
    Ok(Json(json!({"created": now_secs(), "data": [item]})))
}

async fn transcriptions(
    State(gw): State<Shared>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let ctype = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .filter(|v| v.to_str().is_ok_and(|s| s.starts_with("multipart/form-data")))
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "send the audio as multipart/form-data with a `file` field",
            )
        })?;
    let _stt = gw.stt.lock().await;
    let port = gw.ensure(Slot::SpeechToText).await?;
    let r = gw
        .http
        .post(format!("http://127.0.0.1:{port}/inference"))
        .header(header::CONTENT_TYPE, ctype)
        .body(body)
        .send()
        .await
        .map_err(upstream)?;
    gw.touch(Slot::SpeechToText);
    let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ct = r.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = r.bytes().await.map_err(upstream)?;
    let mut resp = (status, bytes).into_response();
    if let Some(ct) = ct {
        resp.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Ok(resp)
}

#[derive(Deserialize)]
struct SpeechReq {
    input: String,
    language: Option<String>,
}

async fn speech(State(gw): State<Shared>, Json(req): Json<SpeechReq>) -> Result<Response, ApiError> {
    let text = req.input.trim();
    if text.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "input must not be empty"));
    }
    if text.chars().count() > 4000 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "input is longer than 4000 characters",
        ));
    }
    let m = gw.lock.models.get("text_to_speech").ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "text_to_speech is not enabled in this project: run `yourai config set text_to_speech.enabled true` and `yourai rebuild`",
        )
    })?;
    let exe = gw.engine_exe("llama-tts")?;
    let model = gw.resolve(m.main_path().unwrap_or_default())?;
    let mmproj = match m.files.get("mmproj") {
        Some(p) if !p.is_empty() => gw.resolve(p)?,
        _ => {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "text_to_speech model is missing its projector: run `yourai rebuild`",
            ))
        }
    };
    let tc = &gw.cfg.text_to_speech;
    let language = req.language.as_deref().unwrap_or(&tc.language).to_string();
    let speaker = (!tc.speaker_file.is_empty())
        .then(|| paths::inside(&gw.root, &tc.speaker_file).ok())
        .flatten();

    let _tts = gw.tts.lock().await;
    let tmp = gw.state_dir.join(format!("tts-{}.wav", std::process::id()));
    let args = engine::tts_args(&model, &mmproj, text, &language, speaker.as_deref(), &tmp);
    let log_file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(gw.state_dir.join("text_to_speech.log"))
        .map_err(internal)?;
    let status = tokio::process::Command::new(&exe)
        .args(&args)
        .env(engine::lib_env_var(), exe.parent().unwrap_or(Path::new(".")))
        .stdin(std::process::Stdio::null())
        .stdout(log_file.try_clone().map_err(internal)?)
        .stderr(log_file)
        .kill_on_drop(true)
        .status()
        .await
        .map_err(internal)?;
    let bytes = std::fs::read(&tmp).ok().filter(|_| status.success());
    let _ = std::fs::remove_file(&tmp);
    let bytes = bytes.ok_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "text to speech failed; see {}",
                gw.state_dir.join("text_to_speech.log").display()
            ),
        )
    })?;
    let rel = gw.save_output("audio", &slug(text), "wav", &bytes)?;
    let mut resp = (StatusCode::OK, bytes).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    if let Ok(v) = HeaderValue::from_str(&format!("outputs/{rel}")) {
        resp.headers_mut().insert("x-yourai-file", v);
    }
    Ok(resp)
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "txt" | "md" => "text/plain; charset=utf-8",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

async fn files(
    State(gw): State<Shared>,
    axum::extract::Path(rel): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    let path = paths::inside(&gw.outputs, &rel).map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "not found"))?;
    let bytes = std::fs::read(&path).map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "not found"))?;
    let mut resp = (StatusCode::OK, bytes).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime_for(&path)));
    Ok(resp)
}

/// Engine states for `yourai status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStatus {
    pub name: String,
    pub enabled: bool,
    pub running: bool,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub model: String,
    pub idle_seconds: Option<u64>,
}

async fn status(State(gw): State<Shared>) -> Json<Value> {
    let procs = gw.procs.lock().unwrap();
    let mut out = Vec::new();
    let slots = [
        (Slot::Chat, true, gw.lock.model_id.clone()),
        (
            Slot::Image,
            gw.lock.models.contains_key("image"),
            model_id(&gw.lock, "image"),
        ),
        (
            Slot::Video,
            gw.lock.models.contains_key("video"),
            model_id(&gw.lock, "video"),
        ),
        (
            Slot::SpeechToText,
            gw.lock.models.contains_key("speech_to_text"),
            model_id(&gw.lock, "speech_to_text"),
        ),
    ];
    for (slot, enabled, model) in slots {
        let p = procs.get(&slot);
        out.push(EngineStatus {
            name: slot.name().into(),
            enabled,
            running: p.is_some(),
            port: p.map(|p| p.port),
            pid: p.map(|p| p.pid),
            memory_bytes: p.and_then(|p| process::memory(p.pid)),
            model,
            idle_seconds: p.map(|p| p.last_used.elapsed().as_secs()),
        });
    }
    if let Some(m) = gw.lock.models.get("text_to_speech") {
        out.push(EngineStatus {
            name: "text_to_speech".into(),
            enabled: true,
            running: false,
            port: None,
            pid: None,
            memory_bytes: None,
            model: m.id.clone(),
            idle_seconds: None,
        });
    }
    Json(json!({"engines": out, "idle_unload_minutes": gw.cfg.server.idle_unload_minutes}))
}

fn model_id(lock: &Lock, key: &str) -> String {
    lock.models.get(key).map(|m| m.id.clone()).unwrap_or_default()
}

const HOP_BY_HOP: [&str; 6] = [
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "upgrade",
    "proxy-connection",
];

fn static_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "map" => "application/json",
        "md" | "txt" | "http" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Serve a file from the project UI directory. Path traversal is rejected.
async fn serve_project_ui(dir: &Path, req_path: &str) -> Response {
    let rel = if req_path == "/" || req_path.is_empty() {
        "index.html".to_string()
    } else {
        req_path.trim_start_matches('/').to_string()
    };
    let Ok(file) = paths::inside(dir, &rel) else {
        return ApiError::new(StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    let file = if file.is_dir() { file.join("index.html") } else { file };
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let mut resp = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, static_mime(&file));
            if let Ok(len) = HeaderValue::from_str(&bytes.len().to_string()) {
                resp = resp.header(header::CONTENT_LENGTH, len);
            }
            resp.body(Body::from(bytes))
                .unwrap_or_else(|e| internal(e).into_response())
        }
        Err(_) => ApiError::new(StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn should_proxy_to_llama(gw: &Gw, path: &str) -> bool {
    if path == "/health" || path.starts_with("/props") || path.starts_with("/slots") || path.starts_with("/metrics") {
        return true;
    }
    if !path.starts_with("/v1/") {
        return false;
    }
    // Foreign style routes are handled explicitly; do not send them to llama.
    if path == "/v1/messages" || path.contains(":generateContent") || path.contains(":streamGenerateContent") {
        return false;
    }
    if path == "/v1/chat/ws" {
        return false;
    }
    // When api/routes.toml owns mounts, chat completions are only served if registered there.
    if gw.route_table.is_some() && path.starts_with("/v1/chat/completions") {
        return false;
    }
    // Otherwise always proxy OpenAI Chat Completions HTTP (used by `yourai chat` and adapters).
    true
}

/// Proxy to llama-server at a fixed upstream path (for remapped `openai_chat` routes).
async fn proxy_upstream_path(State(gw): State<Shared>, req: Request, upstream_path: &str) -> Response {
    let Some(port) = gw.running_port(Slot::Chat) else {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "the chat model is still loading").into_response();
    };
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let target = format!("http://127.0.0.1:{port}{upstream_path}{query}");
    let mut rb = gw.http.request(req.method().clone(), target);
    for (k, v) in req.headers() {
        if k != header::HOST && k != header::CONTENT_LENGTH && !HOP_BY_HOP.contains(&k.as_str()) {
            rb = rb.header(k, v);
        }
    }
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());
    let r = match rb.body(body).send().await {
        Ok(r) => r,
        Err(e) => return upstream(e).into_response(),
    };
    let mut resp = Response::builder().status(r.status());
    for (k, v) in r.headers() {
        if !HOP_BY_HOP.contains(&k.as_str()) {
            resp = resp.header(k, v);
        }
    }
    resp.body(Body::from_stream(r.bytes_stream()))
        .unwrap_or_else(|e| internal(e).into_response())
}

async fn openai_chat_upstream(State(gw): State<Shared>, req: Request) -> Response {
    if gw.chat_uses_ollama() && *req.method() == Method::POST {
        return openai_chat_ollama(State(gw), req).await;
    }
    proxy_upstream_path(State(gw), req, "/v1/chat/completions").await
}

/// Buffer the JSON body so we can force Ollama's imported model name (`yourai-chat`).
async fn openai_chat_ollama(State(gw): State<Shared>, req: Request) -> Response {
    let Some(port) = gw.running_port(Slot::Chat) else {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "the chat model is still loading").into_response();
    };
    let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let mut body: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return ApiError::new(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    body["model"] = json!(ollama::chat_model_id());
    let r = match gw
        .http
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return upstream(e).into_response(),
    };
    let mut resp = Response::builder().status(r.status());
    for (k, v) in r.headers() {
        if !HOP_BY_HOP.contains(&k.as_str()) {
            resp = resp.header(k, v);
        }
    }
    resp.body(Body::from_stream(r.bytes_stream()))
        .unwrap_or_else(|e| internal(e).into_response())
}

/// When a project UI exists, serve it for browser GETs; otherwise (and for API paths)
/// forward to llama-server. Media mounts come from `api/routes.toml` or defaults.
async fn fallback(State(gw): State<Shared>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    if gw.chat_uses_ollama() && method == Method::POST && path.starts_with("/v1/chat/completions") {
        return openai_chat_ollama(State(gw), req).await;
    }
    if let Some(dir) = gw.ui_dir.as_ref() {
        if !should_proxy_to_llama(&gw, &path) && (method == Method::GET || method == Method::HEAD) {
            return serve_project_ui(dir, &path).await;
        }
    }
    if path.starts_with("/v1/") && !should_proxy_to_llama(&gw, &path) {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    }
    proxy(State(gw), req).await
}

/// Everything else (chat completions, the browser chat page, /health) goes to llama-server.
/// Responses are streamed (`Body::from_stream`) so SSE / WebUI resume paths are not buffered.
async fn proxy(State(gw): State<Shared>, req: Request) -> Response {
    let Some(port) = gw.running_port(Slot::Chat) else {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "the chat model is still loading").into_response();
    };
    let pq = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let stream_resume = pq.starts_with("/v1/stream");
    let mut rb = gw
        .http
        .request(req.method().clone(), format!("http://127.0.0.1:{port}{pq}"));
    for (k, v) in req.headers() {
        if k != header::HOST && k != header::CONTENT_LENGTH && !HOP_BY_HOP.contains(&k.as_str()) {
            rb = rb.header(k, v);
        }
    }
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());
    let r = match rb.body(body).send().await {
        Ok(r) => r,
        Err(e) => {
            if stream_resume {
                log(format!("proxy {pq} failed: {e}"));
            }
            return upstream(e).into_response();
        }
    };
    if stream_resume && !r.status().is_success() {
        log(format!("proxy {pq} upstream status {}", r.status()));
    }
    let mut resp = Response::builder().status(r.status().as_u16());
    for (k, v) in r.headers() {
        // Forward SSE / cache headers so the browser WebUI can resume streams.
        if !HOP_BY_HOP.contains(&k.as_str()) {
            resp = resp.header(k, v);
        }
    }
    resp.body(Body::from_stream(r.bytes_stream()))
        .unwrap_or_else(|e| internal(e).into_response())
}

fn mount_default_media(r: Router<Shared>) -> Router<Shared> {
    r.route("/v1/images/generations", post(images))
        .route("/v1/images/edits", post(images_edits))
        .route("/v1/videos/generations", post(videos))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .route("/v1/audio/speech", post(speech))
}

fn router(gw: Shared) -> Router {
    let mut r = Router::new()
        .route("/files/{*path}", get(files))
        .route("/yourai/status", get(status));

    if let Some(table) = gw.route_table.clone() {
        let mut mounted_media = false;
        for (i, entry) in table.route.iter().enumerate() {
            let method = entry.method_norm();
            let path = entry.path.clone();
            if let Some(upstream) = entry.upstream.as_deref() {
                r = match (method.as_str(), upstream) {
                    ("POST", "openai_chat") => r.route(&path, post(openai_chat_upstream)),
                    ("GET", "openai_chat_ws") => r.route(&path, get(super::api::chat_ws)),
                    ("POST", "claude_messages") => r.route(&path, post(super::api::claude_messages)),
                    ("POST", "gemini_generate") => r.route(&path, post(super::api::gemini_generate)),
                    ("POST", "images_generations") => {
                        mounted_media = true;
                        r.route(&path, post(images))
                    }
                    ("POST", "images_edits") => {
                        mounted_media = true;
                        r.route(&path, post(images_edits))
                    }
                    ("POST", "videos_generations") => {
                        mounted_media = true;
                        r.route(&path, post(videos))
                    }
                    ("POST", "audio_transcriptions") => {
                        mounted_media = true;
                        r.route(&path, post(transcriptions))
                    }
                    ("POST", "audio_speech") => {
                        mounted_media = true;
                        r.route(&path, post(speech))
                    }
                    _ => {
                        log(format!(
                            "warning: skipping route {method} {path} (unsupported method/upstream)"
                        ));
                        r
                    }
                };
            } else if entry.handler.is_some() {
                // Capture route index; Shared state is injected by axum.
                let idx = i;
                let handler = move |State(gw): State<Shared>, req: Request| {
                    let idx = idx;
                    async move { custom_handler_at(State(gw), idx, req).await }
                };
                r = match method.as_str() {
                    "GET" => r.route(&path, get(handler)),
                    "POST" => r.route(&path, post(handler)),
                    "PUT" => r.route(&path, axum::routing::put(handler)),
                    "PATCH" => r.route(&path, axum::routing::patch(handler)),
                    "DELETE" => r.route(&path, axum::routing::delete(handler)),
                    other => {
                        log(format!(
                            "warning: skipping handler route {other} {path} (unsupported method)"
                        ));
                        r
                    }
                };
            }
        }
        // Old routes.toml files (pre-media) keep working.
        if !mounted_media && !table.has_media_upstream() {
            log("api/routes.toml has no media upstreams; mounting default /v1/images|videos|audio routes");
            r = mount_default_media(r);
        }
    } else {
        r = mount_default_media(r);
        if gw.cfg.server.has_api_style("openai") {
            r = r.route("/v1/chat/ws", get(super::api::chat_ws));
        }
        if gw.cfg.server.has_api_style("claude") {
            r = r.route("/v1/messages", post(super::api::claude_messages));
        }
        if gw.cfg.server.has_api_style("gemini") {
            r = r.route("/v1/models/{*path}", post(super::api::gemini_generate));
        }
    }
    r.fallback(fallback)
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .layer(axum::middleware::from_fn_with_state(gw.clone(), auth))
        .with_state(gw)
}

async fn custom_handler_at(State(gw): State<Shared>, idx: usize, req: Request) -> Response {
    let Some(table) = gw.route_table.as_ref() else {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    };
    let Some(entry) = table.route.get(idx) else {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    };
    let Some(handler) = entry.handler.as_ref() else {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    };
    let opts = super::api::handler::InvokeOpts {
        timeout_ms: entry.default_timeout_ms(),
        stream: entry.stream,
        content_type: entry.stream_content_type().to_string(),
    };
    super::api::handler::invoke(&gw.root, handler, opts, req).await
}

async fn stop_all(gw: &Gw) {
    for slot in [Slot::Image, Slot::Video, Slot::SpeechToText, Slot::Chat] {
        gw.stop(slot).await;
    }
    let _ = std::fs::remove_file(gw.state_dir.join("engines.json"));
}

async fn idle_loop(gw: Shared) {
    let minutes = gw.cfg.server.idle_unload_minutes;
    if minutes == 0 {
        return;
    }
    let limit = Duration::from_secs(minutes as u64 * 60);
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let idle: Vec<Slot> = gw
            .procs
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, p)| **s != Slot::Chat && p.last_used.elapsed() > limit)
            .map(|(s, _)| *s)
            .collect();
        for slot in idle {
            let guard = match slot {
                Slot::Image | Slot::Video => gw.sd.try_lock().ok(),
                _ => gw.stt.try_lock().ok(),
            };
            if guard.is_some() {
                log(format!("{} engine idle for {minutes} min, unloading", slot.name()));
                gw.stop(slot).await;
            }
        }
    }
}

async fn chat_watch(gw: Shared) {
    if let Err(e) = gw.ensure(Slot::Chat).await {
        log(format!("error: {}", e.1));
        stop_all(&gw).await;
        std::process::exit(1);
    }
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let (pid, started) = match gw.procs.lock().unwrap().get(&Slot::Chat) {
            Some(p) => (p.pid, p.started),
            None => continue,
        };
        if !process::is_ours(pid, started) {
            log("error: the chat engine stopped unexpectedly; see the lines above");
            stop_all(&gw).await;
            std::process::exit(1);
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Kills engine processes left behind by a gateway that did not shut down cleanly.
pub fn kill_leftovers(p: &Project) {
    let path = p.engines_path();
    let Some(recs) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, EngineRecord>>(&t).ok())
    else {
        return;
    };
    for r in recs.values() {
        if process::is_ours(r.pid, r.started) {
            process::stop(r.pid);
        }
    }
    let _ = std::fs::remove_file(path);
}

/// Entry point of `yourai __gateway`.
pub fn run(p: &Project) -> anyhow::Result<()> {
    let cfg = p.config()?;
    let lock = p
        .lock()
        .ok_or_else(|| anyhow::anyhow!("this project has not been built yet; run `yourai build`"))?;
    let api_key = if cfg.server.require_api_key {
        Some(
            secrets::api_key(&p.root)
                .ok_or_else(|| anyhow::anyhow!("server.require_api_key is true but no API key is set"))?,
        )
    } else {
        None
    };
    let key_file = p.state_dir().join("api-keys.txt");
    let ui_dir = cfg.project_ui_dir(&p.root);
    if let Some(dir) = &ui_dir {
        log(format!("serving project UI from {}", dir.display()));
    }
    let route_table = super::api::routes::load(&p.root)?;
    if let Some(t) = &route_table {
        log(format!("loaded {} route(s) from api/routes.toml", t.route.len()));
    }
    let gw = Arc::new(Gw {
        root: p.root.clone(),
        state_dir: p.state_dir(),
        outputs: p.outputs_dir(),
        ui_dir,
        route_table,
        key_file: (api_key.is_some() && key_file.is_file()).then_some(key_file),
        cfg,
        lock,
        api_key,
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()?,
        procs: Mutex::new(BTreeMap::new()),
        sd: tokio::sync::Mutex::new(()),
        stt: tokio::sync::Mutex::new(()),
        tts: tokio::sync::Mutex::new(()),
    });
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        let host = if gw.cfg.server.host == "localhost" {
            "127.0.0.1"
        } else {
            gw.cfg.server.host.as_str()
        };
        let addr: SocketAddr = format!("{host}:{}", gw.cfg.server.port)
            .parse()
            .or_else(|_| format!("[{host}]:{}", gw.cfg.server.port).parse())?;
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("couldn't bind {addr}: {e}"))?;
        log(format!("listening on {addr}"));
        tokio::spawn(chat_watch(gw.clone()));
        tokio::spawn(idle_loop(gw.clone()));
        axum::serve(listener, router(gw.clone()))
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        log("shutting down");
        stop_all(&gw).await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rules() {
        assert!(!needs_key(&Method::GET, "/health"));
        assert!(!needs_key(&Method::GET, "/"));
        assert!(!needs_key(&Method::GET, "/assets/index.js"));
        assert!(needs_key(&Method::GET, "/v1/models"));
        assert!(needs_key(&Method::POST, "/completion"));
        assert!(needs_key(&Method::GET, "/files/images/a.png"));
        assert!(needs_key(&Method::GET, "/yourai/status"));

        let mut h = HeaderMap::new();
        assert!(!key_matches(&h, "secret"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert!(key_matches(&h, "secret"));
        assert!(!key_matches(&h, "secreT"));
        assert!(!key_matches(&h, "secret2"));
    }

    #[test]
    fn sizes_frames_and_names() {
        assert_eq!(parse_size("512x768"), Some((512, 768)));
        assert_eq!(parse_size("500x500"), None);
        assert_eq!(parse_size("big"), None);
        assert_eq!(align_frames(33), 33);
        assert_eq!(align_frames(35), 33);
        assert_eq!(align_frames(1), 5);
        assert_eq!(align_frames_minimax_h3(5), 5);
        assert_eq!(align_frames_minimax_h3(56), 56);
        assert_eq!(align_frames_minimax_h3(57), 73);
        assert_eq!(align_frames_minimax_h3(1), 5);
        assert_eq!(
            slug("A cat, on a chair! Very cute and fluffy indeed"),
            "a-cat-on-a-chair-very"
        );
        assert_eq!(slug("!!!"), "output");
        assert_eq!(pick(Some(4), 0, &json!({"steps": 1}), "steps", 20), 4);
        assert_eq!(pick(None, 0, &json!({"steps": 1}), "steps", 20), 1);
        assert_eq!(pick(None, 8, &json!({"steps": 1}), "steps", 20), 8);

        let wan = video_job_body(
            "waves",
            "",
            -1,
            None,
            None,
            None,
            None,
            0,
            0,
            0,
            0,
            0,
            &json!({
                "frame_align": "wan",
                "width": 480,
                "height": 272,
                "steps": 20,
                "cfg_scale": 6.0,
                "flow_shift": 3.0,
                "frames": 33,
                "fps": 16,
                "sample_method": "euler",
                "distilled_guidance": 3.5
            }),
            None,
            None,
        );
        assert_eq!(wan["width"], 480);
        assert_eq!(wan["height"], 272);
        assert_eq!(wan["video_frames"], 33);
        assert_eq!(wan["fps"], 16);
        assert_eq!(wan["sample_params"]["flow_shift"], 3.0);
        assert_eq!(wan["sample_params"]["guidance"]["txt_cfg"], 6.0);
        assert_eq!(wan["sample_params"]["sample_method"], "euler");
        assert!(wan.get("init_image").is_none());

        let wan_i2v = video_job_body(
            "waves",
            "",
            -1,
            None,
            None,
            None,
            None,
            0,
            0,
            0,
            0,
            0,
            &json!({"frame_align": "wan", "width": 480, "height": 272, "steps": 20, "cfg_scale": 6.0, "flow_shift": 3.0, "frames": 33, "fps": 16, "sample_method": "euler"}),
            Some("data:image/png;base64,aaa"),
            Some(0.5),
        );
        assert_eq!(wan_i2v["init_image"], "data:image/png;base64,aaa");
        assert_eq!(wan_i2v["strength"], 0.5);
        assert!(wan_i2v.get("ref_images").is_none());

        let h3 = video_job_body(
            "fox",
            "",
            7,
            None,
            Some(60),
            None,
            None,
            0,
            0,
            0,
            0,
            0,
            &json!({
                "frame_align": "minimax_h3",
                "width": 864,
                "height": 480,
                "steps": 8,
                "cfg_scale": 1.0,
                "flow_shift": 12.0,
                "frames": 56,
                "fps": 24,
                "sample_method": "euler",
                "distilled_guidance": 3.5,
                "size_multiple": 32
            }),
            Some("data:image/png;base64,bbb"),
            Some(0.9),
        );
        assert_eq!(h3["width"], 864);
        assert_eq!(h3["height"], 480);
        assert_eq!(h3["video_frames"], 73); // 60 → 17k+5
        assert_eq!(h3["fps"], 24);
        assert_eq!(h3["sample_params"]["flow_shift"], 12.0);
        assert_eq!(h3["sample_params"]["guidance"]["txt_cfg"], 1.0);
        assert_eq!(h3["seed"], 7);
        assert_eq!(h3["ref_images"], json!(["data:image/png;base64,bbb"]));
        assert!(h3.get("init_image").is_none());
        assert!(h3.get("strength").is_none());

        let edit = image_job_body("cat", "", 512, 512, -1, 1, 20, 7.0, Some("data:image/png;base64,x"), Some(2.0));
        assert_eq!(edit["init_image"], "data:image/png;base64,x");
        assert_eq!(edit["strength"], 1.0);
        assert_eq!(clamp_strength(None), 0.75);
        assert_eq!(clamp_strength(Some(0.01)), 0.05);

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("in.png"), b"fakepng").unwrap();
        let enc = encode_image_input(tmp.path(), "in.png").unwrap();
        assert!(enc.starts_with("data:image/png;base64,"));
        assert!(encode_image_input(tmp.path(), "").is_err());
    }

    #[tokio::test]
    async fn api_key_is_enforced_and_unknown_capabilities_404() {
        let tmp = tempfile::tempdir().unwrap();
        let gw = Arc::new(Gw {
            root: tmp.path().into(),
            state_dir: tmp.path().join(".yourai"),
            outputs: tmp.path().join("outputs"),
            ui_dir: None,
            route_table: None,
            cfg: super::super::config::sample(),
            lock: Lock::default(),
            api_key: Some("k".into()),
            key_file: None,
            http: reqwest::Client::new(),
            procs: Mutex::new(BTreeMap::new()),
            sd: tokio::sync::Mutex::new(()),
            stt: tokio::sync::Mutex::new(()),
            tts: tokio::sync::Mutex::new(()),
        });
        std::fs::create_dir_all(tmp.path().join("outputs/images")).unwrap();
        std::fs::write(tmp.path().join("outputs/images/a.png"), b"png").unwrap();
        std::fs::write(tmp.path().join("secret.txt"), b"no").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router(gw)).await });
        let c = reqwest::Client::new();
        let base = format!("http://{addr}");

        let r = c.get(format!("{base}/files/images/a.png")).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = c
            .get(format!("{base}/files/images/a.png"))
            .bearer_auth("k")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "image/png");
        let r = c
            .get(format!("{base}/files/..%2Fsecret.txt"))
            .bearer_auth("k")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
        let r = c
            .post(format!("{base}/v1/images/generations"))
            .bearer_auth("k")
            .json(&json!({"prompt": "a cat"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404);
        let body: Value = r.json().await.unwrap();
        assert!(body["error"]["message"].as_str().unwrap().contains("image.enabled"));
        let r = c.get(format!("{base}/health")).send().await.unwrap();
        assert_eq!(r.status(), 503, "chat engine not running yet");
    }
}

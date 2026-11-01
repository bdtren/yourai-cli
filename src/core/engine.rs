use std::path::{Path, PathBuf};
use std::process::Command;

use super::catalog::{self, Catalog, EngineEntry, Model};
use super::config::Config;
use super::downloader;
use super::hardware::{GpuVendor, Hardware};
use super::store::{self, Store};
use crate::error::{CliError, Exit};
use crate::ui;

/// How to install and check one engine from the catalog.
pub struct EngineSpec {
    /// Catalog engine name.
    pub name: &'static str,
    /// Shown to the user.
    pub label: &'static str,
    /// Executable inside the archive, without `.exe`.
    pub exe: &'static str,
    /// Arguments that must exit 0 on a working build.
    pub smoke: &'static [&'static str],
}

pub const LLAMA: EngineSpec = EngineSpec {
    name: catalog::LLAMA,
    label: "llama.cpp",
    exe: "llama-server",
    smoke: &["--version"],
};

pub const SD: EngineSpec = EngineSpec {
    name: catalog::SD,
    label: "stable-diffusion.cpp",
    exe: "sd-server",
    smoke: &["--version"],
};

pub const WHISPER: EngineSpec = EngineSpec {
    name: catalog::WHISPER,
    label: "whisper.cpp",
    exe: "whisper-server",
    smoke: &["--help"],
};

pub fn spec(name: &str) -> Option<&'static EngineSpec> {
    [&LLAMA, &SD, &WHISPER].into_iter().find(|s| s.name == name)
}

pub fn exe_file(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn os_arch() -> (String, String) {
    (std::env::consts::OS.to_string(), std::env::consts::ARCH.to_string())
}

/// Preferred asset key for this machine, for example `linux-x86_64-cuda`.
pub fn preferred_key(hw: &Hardware) -> String {
    let (os, arch) = os_arch();
    let backend = match (os.as_str(), hw.gpu_vendor) {
        ("macos", _) if arch == "aarch64" => "metal",
        ("macos", _) => "cpu",
        (_, GpuVendor::Nvidia) => "cuda",
        (_, GpuVendor::Amd | GpuVendor::Intel) => "vulkan",
        _ => "cpu",
    };
    format!("{os}-{arch}-{backend}")
}

pub fn cpu_key() -> String {
    let (os, arch) = os_arch();
    format!("{os}-{arch}-cpu")
}

/// Asset keys to try, best first: the GPU build, Vulkan for NVIDIA without CUDA, then CPU.
fn candidate_keys(hw: &Hardware) -> Vec<String> {
    let want = preferred_key(hw);
    let mut keys = vec![want.clone()];
    if want.ends_with("-cuda") {
        keys.push(want.replace("-cuda", "-vulkan"));
    }
    if !keys.contains(&cpu_key()) {
        keys.push(cpu_key());
    }
    keys
}

/// Picks the best asset key the catalog actually has, falling back to the CPU build.
/// Returns (key, reason-if-fallback).
pub fn select_key(hw: &Hardware, entry: &EngineEntry) -> anyhow::Result<(String, Option<String>)> {
    select_key_for(hw, entry, "llama.cpp")
}

fn select_key_for(hw: &Hardware, entry: &EngineEntry, label: &str) -> anyhow::Result<(String, Option<String>)> {
    let want = preferred_key(hw);
    for key in candidate_keys(hw) {
        if entry.assets.contains_key(&key) {
            let note = (key != want).then(|| {
                if key == cpu_key() {
                    format!("no {label} GPU build for `{want}` in the catalog, using the CPU build (slower)")
                } else {
                    format!("no {label} build for `{want}` in the catalog, using `{key}`")
                }
            });
            return Ok((key, note));
        }
    }
    Err(
        CliError::new(Exit::Unsupported, format!("no {label} build is available for {want}"))
            .hint("run `yourai update` for a newer catalog, or turn this capability off in yourai.toml")
            .into(),
    )
}

/// Whether the catalog has any build of this engine for this machine.
pub fn available(hw: &Hardware, catalog: &Catalog, spec: &EngineSpec) -> bool {
    catalog
        .engine(spec.name)
        .is_some_and(|e| candidate_keys(hw).iter().any(|k| e.assets.contains_key(k)))
}

/// Relative install folder name under `engines/` (shared by module and cache).
pub fn engine_folder(spec: &EngineSpec, version: &str, key: &str) -> String {
    if spec.name == LLAMA.name {
        format!("{version}-{key}")
    } else {
        format!("{}-{version}-{key}", spec.exe)
    }
}

fn module_spec_dir(store: &Store, spec: &EngineSpec, version: &str, key: &str) -> PathBuf {
    store.engines_dir().join(engine_folder(spec, version, key))
}

fn cache_spec_dir(spec: &EngineSpec, version: &str, key: &str) -> PathBuf {
    Store::cache_engines().join(engine_folder(spec, version, key))
}

/// Download size of the build `ensure_spec` would install, or 0 if it is already in the
/// shared cache (a later build can fast-copy it).
pub fn download_mb(hw: &Hardware, catalog: &Catalog, spec: &EngineSpec) -> f64 {
    let Some(entry) = catalog.engine(spec.name) else {
        return 0.0;
    };
    select_key_for(hw, entry, spec.label)
        .ok()
        .filter(|(k, _)| !cache_spec_dir(spec, &entry.version, k).join(".installed").is_file())
        .and_then(|(k, _)| entry.assets.get(&k).map(|v| v.iter().map(|a| a.size_mb).sum()))
        .unwrap_or(0.0)
}

fn find_exe(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(exe_file(name));
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_exe(&p, name) {
                return Some(found);
            }
        }
    }
    None
}

/// Another program shipped in the same engine folder, such as `llama-tts`.
pub fn sibling(exe: &Path, name: &str) -> Option<PathBuf> {
    let p = exe.parent()?.join(exe_file(name));
    p.is_file().then_some(p)
}

fn unpack(archive: &Path, into: &Path) -> anyhow::Result<()> {
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        z.extract(into)?;
    } else {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut t = tar::Archive::new(gz);
        t.set_preserve_permissions(true);
        t.unpack(into)?;
    }
    Ok(())
}

/// The engine's smoke-test command (for example `llama-server --version`) must exit 0.
fn smoke(exe: &Path, spec: &EngineSpec) -> anyhow::Result<String> {
    let out = Command::new(exe)
        .args(spec.smoke)
        .env(lib_env_var(), exe.parent().unwrap())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        let tail: Vec<&str> = text.trim().lines().rev().take(5).collect();
        anyhow::bail!(
            "{} {} failed: {}",
            spec.exe,
            spec.smoke.join(" "),
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
    Ok(text
        .lines()
        .find(|l| l.contains("version"))
        .unwrap_or("")
        .trim()
        .to_string())
}

pub fn lib_env_var() -> &'static str {
    if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else if cfg!(windows) {
        "YOURAI_ENGINE_DIR"
    } else {
        "LD_LIBRARY_PATH"
    }
}

fn engine_ready(dir: &Path, spec: &EngineSpec) -> Option<PathBuf> {
    let exe = find_exe(dir, spec.exe)?;
    (dir.join(".installed").is_file() && smoke(&exe, spec).is_ok()).then_some(exe)
}

/// Downloads, verifies, unpacks, and smoke-tests one engine build into the project module.
fn install_spec(store: &Store, spec: &EngineSpec, entry: &EngineEntry, key: &str) -> anyhow::Result<PathBuf> {
    let label = spec.label;
    let dir = module_spec_dir(store, spec, &entry.version, key);
    let cache = cache_spec_dir(spec, &entry.version, key);

    if !store.opts.force {
        if let Some(exe) = engine_ready(&dir, spec) {
            ui::ok(format!("engine {label} {} ({key}) is installed", entry.version));
            return Ok(exe);
        }
        if store::try_copy_engine_from_cache(store, &dir, &cache)? {
            if let Some(exe) = engine_ready(&dir, spec) {
                ui::ok(format!("engine {label} {} ({key}) is installed", entry.version));
                return Ok(exe);
            }
        }
    }

    let assets = entry
        .assets
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("catalog has no {label} asset `{key}`"))?;
    let staging = dir.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    std::fs::create_dir_all(store.downloads_dir())?;
    for a in assets {
        let dest = store.downloads_dir().join(&a.file);
        let url = entry.asset_url(a);
        downloader::download(&downloader::Request {
            url: &url,
            dest: &dest,
            sha256: Some(&a.sha256),
            size_bytes: (a.size_mb * 1e6) as u64,
            label: &format!("engine {}", a.file),
        })?;
        unpack(&dest, &staging)?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir)?;
    let exe =
        find_exe(&dir, spec.exe).ok_or_else(|| anyhow::anyhow!("{} not found inside the engine archive", spec.exe))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for entry in std::fs::read_dir(exe.parent().unwrap())?.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_none() {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    smoke(&exe, spec)?;
    std::fs::write(dir.join(".installed"), key)?;
    store::maybe_save_engine_dir(store, &dir, &cache)?;
    ui::ok(format!("engine {label} {} ({key}) installed", entry.version));
    Ok(exe)
}

/// Installs the best llama.cpp build into the project module.
pub fn ensure(store: &Store, hw: &Hardware, catalog: &Catalog) -> anyhow::Result<(String, PathBuf)> {
    ensure_spec(store, hw, catalog, &LLAMA)
}

/// Installs the best build of an engine, retrying with the next build (Vulkan, then CPU)
/// if one fails its smoke test.
pub fn ensure_spec(
    store: &Store,
    hw: &Hardware,
    catalog: &Catalog,
    spec: &EngineSpec,
) -> anyhow::Result<(String, PathBuf)> {
    let entry = catalog
        .engine(spec.name)
        .ok_or_else(|| anyhow::anyhow!("the catalog has no {} engine; run `yourai update`", spec.label))?;
    let (key, note) = select_key_for(hw, entry, spec.label)?;
    if let Some(n) = note {
        ui::warn(n);
    }
    let mut last = match install_spec(store, spec, entry, &key) {
        Ok(exe) => return Ok((key, exe)),
        Err(e) => e,
    };
    let rest: Vec<String> = candidate_keys(hw)
        .into_iter()
        .skip_while(|k| *k != key)
        .skip(1)
        .filter(|k| entry.assets.contains_key(k))
        .collect();
    let mut tried = key;
    for k in rest {
        ui::warn(format!(
            "the {label} {tried} build did not work on this machine ({last}); trying {k}",
            label = spec.label
        ));
        match install_spec(store, spec, entry, &k) {
            Ok(exe) => return Ok((k, exe)),
            Err(e) => {
                last = e;
                tried = k;
            }
        }
    }
    Err(last)
}

/// Where an engine listens.
pub struct Bind<'a> {
    pub host: &'a str,
    pub port: u16,
}

/// Command-line arguments for llama-server. The API key is passed through a file,
/// never on the command line.
pub fn llama_args(
    cfg: &Config,
    model_path: &Path,
    mmproj: Option<&Path>,
    bind: Bind,
    api_key_file: Option<&Path>,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model_path.display().to_string(),
        "--host".into(),
        bind.host.to_string(),
        "--port".into(),
        bind.port.to_string(),
        "-c".into(),
        cfg.runtime.context_length.to_string(),
        "-ngl".into(),
        cfg.runtime.gpu_layers.clone(),
        "--alias".into(),
        cfg.name.clone(),
        "--temp".into(),
        cfg.generation.temperature.to_string(),
        "--top-p".into(),
        cfg.generation.top_p.to_string(),
        "-n".into(),
        cfg.generation.max_tokens.to_string(),
    ];
    if cfg.runtime.threads > 0 {
        a.extend(["-t".into(), cfg.runtime.threads.to_string()]);
    }
    // WebUI stays on by default. Browser chat can show llama.cpp's
    // "Stream resume produced no new bytes" when an SSE resume (`/v1/stream/...`)
    // returns empty after a quiet or interrupted generation — prefer `yourai chat`
    // or retry; local models also cannot fetch live weather/web data without a tool.
    if !cfg.server.web_ui {
        a.push("--no-webui".into());
    }
    if let Some(f) = api_key_file {
        a.extend(["--api-key-file".into(), f.display().to_string()]);
    }
    if let Some(p) = mmproj {
        a.extend(["--mmproj".into(), p.display().to_string()]);
    }
    a.push("--jinja".into());
    a
}

/// stable-diffusion.cpp server for one image or video model. `files` maps each role
/// (`model` or `diffusion_model`, `vae`, `t5xxl`, `clip_l`, ...) to its path.
pub fn sd_args(
    files: &std::collections::BTreeMap<String, String>,
    main_role: &str,
    port: u16,
    threads: u32,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--listen-ip".into(),
        "127.0.0.1".into(),
        "--listen-port".into(),
        port.to_string(),
    ];
    // sd-server spells most component flags with `_` (`--clip_l`, `--t5xxl`) but this one with `-`.
    let flag = |role: &str| match role {
        "model" => "-m".to_string(),
        "diffusion_model" => "--diffusion-model".to_string(),
        other => format!("--{other}"),
    };
    if let Some(p) = files.get(main_role) {
        a.extend([flag(main_role), p.clone()]);
    }
    for (role, p) in files {
        if role != main_role && role != "model" && role != "diffusion_model" {
            a.extend([flag(role), p.clone()]);
        }
    }
    if threads > 0 {
        a.extend(["-t".into(), threads.to_string()]);
    }
    a
}

pub fn whisper_args(model: &Path, port: u16, threads: u32, language: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model.display().to_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-l".into(),
        language.into(),
    ];
    if threads > 0 {
        a.extend(["-t".into(), threads.to_string()]);
    }
    a
}

/// One `llama-tts` run that writes a WAV file.
pub fn tts_args(
    model: &Path,
    mmproj: &Path,
    text: &str,
    language: &str,
    speaker_file: Option<&Path>,
    out: &Path,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model.display().to_string(),
        "-mm".into(),
        mmproj.display().to_string(),
        "-p".into(),
        text.into(),
        "--tts-lang".into(),
        language.into(),
        "-o".into(),
        out.display().to_string(),
    ];
    if let Some(s) = speaker_file {
        a.extend(["--tts-speaker-file".into(), s.display().to_string()]);
    }
    a
}

/// Video/image engines are only worth offering with enough fast memory.
pub fn model_fits(hw: &Hardware, m: &Model) -> Result<(), String> {
    let need = m.memory_gb(m.default_quant(), 0).unwrap_or(0.0);
    if m.kind == catalog::Kind::Video && super::hardware::cpu_only(hw) {
        return Err("needs a GPU or Apple Silicon; on CPU a short clip takes hours".into());
    }
    let have = super::hardware::budget_gb(hw);
    if need > have {
        return Err(format!(
            "needs about {need:.1} GB, this computer has {have:.1} GB to spare"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;
    use crate::core::hardware::fixtures;

    #[test]
    fn args_never_contain_the_key() {
        let cfg = sample();
        let bind = Bind {
            host: "127.0.0.1",
            port: 39100,
        };
        let args = llama_args(&cfg, Path::new("/m.gguf"), None, bind, Some(Path::new("/k")));
        assert!(args.contains(&"--api-key-file".to_string()));
        assert!(!args.contains(&"--api-key".to_string()));
        assert!(!args.contains(&"--no-webui".to_string()));
    }

    #[test]
    fn llama_args_add_projector_and_bind_locally() {
        let cfg = sample();
        let bind = Bind {
            host: "127.0.0.1",
            port: 39100,
        };
        let a = llama_args(&cfg, Path::new("/m.gguf"), Some(Path::new("/mm.gguf")), bind, None);
        let s = a.join(" ");
        assert!(s.contains("--mmproj /mm.gguf"));
        assert!(s.contains("--host 127.0.0.1 --port 39100"));
        assert!(s.contains("--jinja"));
    }

    #[test]
    fn sd_args_map_roles_to_flags() {
        let files: std::collections::BTreeMap<String, String> = [
            ("diffusion_model", "/w.gguf"),
            ("vae", "/vae.safetensors"),
            ("t5xxl", "/t5.gguf"),
            ("clip_l", "/clip.gguf"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let s = sd_args(&files, "diffusion_model", 39101, 0).join(" ");
        assert!(s.starts_with("--listen-ip 127.0.0.1 --listen-port 39101 --diffusion-model /w.gguf"));
        assert!(s.contains("--vae /vae.safetensors"));
        assert!(s.contains("--t5xxl /t5.gguf"));
        assert!(s.contains("--clip_l /clip.gguf"));
        let one: std::collections::BTreeMap<String, String> =
            [("model".to_string(), "/sd.gguf".to_string())].into_iter().collect();
        assert_eq!(
            sd_args(&one, "model", 1, 4).join(" "),
            "--listen-ip 127.0.0.1 --listen-port 1 -m /sd.gguf -t 4"
        );
    }

    #[test]
    fn whisper_and_tts_args() {
        let w = whisper_args(Path::new("/b.bin"), 39102, 0, "auto").join(" ");
        assert_eq!(w, "-m /b.bin --host 127.0.0.1 --port 39102 -l auto");
        let t = tts_args(
            Path::new("/q.gguf"),
            Path::new("/mm.gguf"),
            "hi there",
            "en",
            Some(Path::new("/me.wav")),
            Path::new("/o.wav"),
        );
        assert_eq!(t[5], "hi there", "text is one argument, never split");
        let s = t.join(" ");
        assert!(s.contains("-mm /mm.gguf") && s.contains("--tts-speaker-file /me.wav") && s.contains("-o /o.wav"));
    }

    #[test]
    fn candidate_keys_try_vulkan_before_cpu_for_nvidia() {
        let keys = candidate_keys(&fixtures::nvidia_8gb());
        if keys[0].ends_with("-cuda") {
            assert!(keys[1].ends_with("-vulkan"));
            assert!(keys[2].ends_with("-cpu"));
        }
        assert!(keys.last().unwrap().ends_with("-cpu"));
    }

    #[test]
    fn key_selection_falls_back_to_cpu() {
        let c = Catalog::bundled();
        let (key, _) = select_key(&fixtures::cpu_only_8gb(), c.llama()).unwrap();
        assert!(key.ends_with("-cpu") || key.ends_with("-metal"));
    }
}

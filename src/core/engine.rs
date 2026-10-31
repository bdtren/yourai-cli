use std::path::{Path, PathBuf};
use std::process::Command;

use super::catalog::{Catalog, EngineEntry};
use super::config::Config;
use super::downloader;
use super::hardware::{GpuVendor, Hardware};
use super::paths;
use crate::error::{CliError, Exit};
use crate::ui;

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

/// Picks the best asset key the catalog actually has, falling back to the CPU build.
/// Returns (key, reason-if-fallback).
pub fn select_key(hw: &Hardware, entry: &EngineEntry) -> anyhow::Result<(String, Option<String>)> {
    let want = preferred_key(hw);
    if entry.assets.contains_key(&want) {
        return Ok((want, None));
    }
    let cpu = cpu_key();
    if entry.assets.contains_key(&cpu) {
        return Ok((
            cpu,
            Some(format!(
                "no GPU build for `{want}` in the catalog, using the CPU build (slower)"
            )),
        ));
    }
    Err(
        CliError::new(Exit::Unsupported, format!("no llama.cpp build is available for {want}"))
            .hint("supported: macOS (Apple Silicon, Intel), Linux and Windows on x86_64/arm64")
            .into(),
    )
}

pub fn install_dir(version: &str, key: &str) -> PathBuf {
    paths::engines().join(format!("{version}-{key}"))
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn find_exe(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join(exe_name());
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_exe(&p) {
                return Some(found);
            }
        }
    }
    None
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

/// `llama-server --version` must exit 0.
pub fn smoke_test(exe: &Path) -> anyhow::Result<String> {
    let out = Command::new(exe)
        .arg("--version")
        .env(lib_env_var(), exe.parent().unwrap())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        anyhow::bail!("llama-server --version failed: {}", text.trim());
    }
    Ok(text
        .lines()
        .find(|l| l.starts_with("version"))
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

/// Downloads, verifies, unpacks, and smoke-tests one engine build. Idempotent.
pub fn install(entry: &EngineEntry, key: &str) -> anyhow::Result<PathBuf> {
    let dir = install_dir(&entry.version, key);
    if let Some(exe) = find_exe(&dir) {
        if dir.join(".installed").is_file() && smoke_test(&exe).is_ok() {
            ui::ok(format!("engine llama.cpp {} ({key}) is installed", entry.version));
            return Ok(exe);
        }
    }
    let assets = entry
        .assets
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("catalog has no engine asset `{key}`"))?;
    let staging = dir.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    for a in assets {
        let dest = paths::downloads().join(&a.file);
        let url = format!("{}/{}", entry.release_base, a.file);
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
    let exe = find_exe(&dir).ok_or_else(|| anyhow::anyhow!("llama-server not found inside the engine archive"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    smoke_test(&exe)?;
    std::fs::write(dir.join(".installed"), key)?;
    ui::ok(format!("engine llama.cpp {} ({key}) installed", entry.version));
    Ok(exe)
}

/// Installs the best build, retrying with the CPU build if the GPU build fails its smoke test.
pub fn ensure(hw: &Hardware, catalog: &Catalog) -> anyhow::Result<(String, PathBuf)> {
    let entry = catalog.llama();
    let (key, note) = select_key(hw, entry)?;
    if let Some(n) = note {
        ui::warn(n);
    }
    match install(entry, &key) {
        Ok(exe) => Ok((key, exe)),
        Err(e) if key != cpu_key() && entry.assets.contains_key(&cpu_key()) => {
            ui::warn(format!(
                "the {key} build did not work on this machine ({e}); falling back to the CPU build"
            ));
            let cpu = cpu_key();
            let exe = install(entry, &cpu)?;
            Ok((cpu, exe))
        }
        Err(e) => Err(e),
    }
}

/// Command-line arguments for llama-server. The API key is passed through a file,
/// never on the command line.
pub fn server_args(cfg: &Config, model_path: &Path, api_key_file: Option<&Path>) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model_path.display().to_string(),
        "--host".into(),
        cfg.server.host.clone(),
        "--port".into(),
        cfg.server.port.to_string(),
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
    if !cfg.server.web_ui {
        a.push("--no-webui".into());
    }
    if let Some(f) = api_key_file {
        a.extend(["--api-key-file".into(), f.display().to_string()]);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;
    use crate::core::hardware::fixtures;

    #[test]
    fn args_never_contain_the_key() {
        let cfg = sample();
        let args = server_args(&cfg, Path::new("/m.gguf"), Some(Path::new("/k")));
        assert!(args.contains(&"--api-key-file".to_string()));
        assert!(!args.contains(&"--api-key".to_string()));
        assert!(!args.contains(&"--no-webui".to_string()));
    }

    #[test]
    fn key_selection_falls_back_to_cpu() {
        let c = Catalog::bundled();
        let (key, _) = select_key(&fixtures::cpu_only_8gb(), c.llama()).unwrap();
        assert!(key.ends_with("-cpu") || key.ends_with("-metal"));
    }
}

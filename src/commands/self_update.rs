use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::{downloader, paths};
use crate::error::{fail_hint, Exit};
use crate::ui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Method {
    Script,
    Homebrew,
    Npm,
    Unknown,
}

pub fn detect_method(exe: &Path) -> Method {
    let s = exe.to_string_lossy().replace('\\', "/");
    if s.contains("/Caskroom/") || s.contains("/Cellar/") {
        return Method::Homebrew;
    }
    if s.contains("/node_modules/") {
        return Method::Npm;
    }
    let receipt: serde_json::Value = std::fs::read_to_string(paths::receipt())
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    if receipt["method"] == "script" {
        let dir = receipt["install_dir"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(paths::bin);
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        if exe.parent() == Some(dir.as_path()) {
            return Method::Script;
        }
    }
    Method::Unknown
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_default()
}

/// Rust target triple of the release archive that matches this build.
pub fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", _) => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("linux", _) => "x86_64-unknown-linux-musl",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        _ => "x86_64-pc-windows-msvc",
    }
}

pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(a), Some(b)) if a > b)
}

fn fetch_latest(timeout: Duration) -> anyhow::Result<String> {
    let client = reqwest::blocking::Client::builder().timeout(timeout).build()?;
    let v = client
        .get(format!("{}/latest.txt", paths::base_url()))
        .send()?
        .error_for_status()?
        .text()?;
    let v = v.trim().to_string();
    if parse_version(&v).is_none() {
        anyhow::bail!("latest.txt has an unexpected value");
    }
    Ok(v)
}

#[derive(Serialize, Deserialize, Default)]
struct Check {
    checked_at: u64,
    latest: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn notices_enabled() -> bool {
    std::env::var_os("YOURAI_NO_UPDATE_CHECK").is_none() && ui::interactive()
}

/// At most once a day, checks for a new version in a background thread. Never blocks.
pub fn spawn_background_check() {
    if !notices_enabled() {
        return;
    }
    let last: Check = std::fs::read_to_string(paths::update_check())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if now().saturating_sub(last.checked_at) < 24 * 3600 {
        return;
    }
    std::thread::spawn(|| {
        if let Ok(latest) = fetch_latest(Duration::from_secs(3)) {
            let _ = std::fs::create_dir_all(paths::home());
            let _ = std::fs::write(
                paths::update_check(),
                serde_json::to_string(&Check {
                    checked_at: now(),
                    latest,
                })
                .unwrap_or_default(),
            );
        }
    });
}

/// Prints a one-line notice if a newer version was found by an earlier check.
pub fn notice_if_newer() {
    if !notices_enabled() {
        return;
    }
    let Some(c) = std::fs::read_to_string(paths::update_check())
        .ok()
        .and_then(|t| serde_json::from_str::<Check>(&t).ok())
    else {
        return;
    };
    if is_newer(&c.latest, VERSION) {
        eprintln!(
            "\n{} yourai {} is available, run `yourai self-update` (hide with YOURAI_NO_UPDATE_CHECK=1)",
            console::style("update:").cyan().bold(),
            c.latest
        );
    }
}

pub fn run(check: bool) -> anyhow::Result<()> {
    let exe = current_exe();
    let method = detect_method(&exe);
    if !check {
        match method {
            Method::Homebrew => {
                ui::info("installed with Homebrew, so update with:\n  brew upgrade yourai");
                return Ok(());
            }
            Method::Npm => {
                ui::info("installed with npm, so update with:\n  npm update -g @yourai/cli");
                return Ok(());
            }
            _ => {}
        }
    }
    let pb = ui::spinner("Checking for a new version…");
    let latest = fetch_latest(Duration::from_secs(10));
    pb.finish_and_clear();
    let latest = latest.map_err(|e| {
        crate::error::CliError::new(Exit::Download, format!("could not check for updates: {e}"))
            .hint("check your connection, or set HTTPS_PROXY")
    })?;
    let _ = std::fs::write(
        paths::update_check(),
        serde_json::to_string(&Check {
            checked_at: now(),
            latest: latest.clone(),
        })
        .unwrap_or_default(),
    );
    if !is_newer(&latest, VERSION) {
        ui::ok(format!("yourai v{VERSION} is the latest version"));
        return Ok(());
    }
    ui::info(format!("yourai {latest} is available (you have v{VERSION})"));
    if check {
        return Ok(());
    }
    match method {
        Method::Homebrew | Method::Npm => Ok(()),
        Method::Unknown => fail_hint(
            Exit::Unsupported,
            format!("cannot tell how yourai was installed ({})", exe.display()),
            "reinstall with: curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash   (Windows: irm https://yourai.bdtren.com/cli/install.ps1 | iex)",
        ),
        Method::Script => install_script(&exe, &latest),
    }
}

fn install_script(exe: &Path, version: &str) -> anyhow::Result<()> {
    let windows = cfg!(windows);
    let ext = if windows { "zip" } else { "tar.gz" };
    let archive = format!("yourai-{version}-{}.{ext}", target());
    let base = format!("{}/releases/{version}", paths::base_url());
    let tmp = paths::downloads().join(format!("self-update-{version}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;

    let client = downloader::client()?;
    let sha_text = client
        .get(format!("{base}/{archive}.sha256"))
        .send()?
        .error_for_status()?
        .text()?;
    let expected = sha_text.split_whitespace().next().unwrap_or("").to_lowercase();
    if expected.len() != 64 {
        return fail_hint(Exit::Download, "the checksum file is malformed", "try again later");
    }
    let dest = tmp.join(&archive);
    downloader::download(&downloader::Request {
        url: &format!("{base}/{archive}"),
        dest: &dest,
        sha256: Some(&expected),
        size_bytes: 0,
        label: &format!("yourai {version}"),
        auth_token: None,
    })?;

    let bin_name = if windows { "yourai.exe" } else { "yourai" };
    if windows {
        zip::ZipArchive::new(std::fs::File::open(&dest)?)?.extract(&tmp)?;
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&dest)?)).unpack(&tmp)?;
    }
    let new_bin = tmp.join(bin_name);
    if !new_bin.is_file() {
        return fail_hint(
            Exit::Download,
            "the release archive does not contain the yourai binary",
            "try again later",
        );
    }
    replace_binary(exe, &new_bin)?;
    let mut receipt: serde_json::Value = std::fs::read_to_string(paths::receipt())
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    receipt["method"] = "script".into();
    receipt["version"] = version.into();
    receipt["install_dir"] = exe.parent().map(|d| d.display().to_string()).unwrap_or_default().into();
    let _ = std::fs::write(paths::receipt(), format!("{receipt}\n"));
    let _ = std::fs::remove_dir_all(&tmp);
    ui::ok(format!("updated to yourai {version}"));
    Ok(())
}

fn replace_binary(exe: &Path, new_bin: &Path) -> anyhow::Result<()> {
    let dir = exe.parent().unwrap();
    if cfg!(windows) {
        // A running .exe cannot be overwritten, but it can be renamed.
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old)?;
        if let Err(e) = std::fs::copy(new_bin, exe) {
            let _ = std::fs::rename(&old, exe);
            return Err(e.into());
        }
    } else {
        let staged = dir.join(".yourai.new");
        std::fs::copy(new_bin, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&staged, exe)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(parse_version("v1.2.3-beta.1"), Some((1, 2, 3)));
    }

    #[test]
    fn methods() {
        assert_eq!(
            detect_method(Path::new("/opt/homebrew/Caskroom/yourai/0.1.0/yourai")),
            Method::Homebrew
        );
        assert_eq!(
            detect_method(Path::new("/opt/homebrew/Cellar/yourai/0.0.1/bin/yourai")),
            Method::Homebrew
        );
        assert_eq!(
            detect_method(Path::new("/usr/lib/node_modules/@yourai/cli-linux-x64/bin/yourai")),
            Method::Npm
        );
        assert_eq!(detect_method(Path::new("/somewhere/else/yourai")), Method::Unknown);
    }
}

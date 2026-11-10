use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{CliError, Exit};
use crate::ui;

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

fn marker(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".sha256");
    PathBuf::from(s)
}

fn part(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// True if `dest` is already downloaded and was verified against `sha`.
pub fn is_verified(dest: &Path, sha: &str) -> bool {
    dest.is_file()
        && std::fs::read_to_string(marker(dest))
            .map(|m| m.trim().eq_ignore_ascii_case(sha))
            .unwrap_or(false)
}

pub fn client() -> anyhow::Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("yourai-cli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .build()?)
}

pub struct Request<'a> {
    pub url: &'a str,
    pub dest: &'a Path,
    /// Expected SHA-256. If None, the computed hash is recorded (trust on first use).
    pub sha256: Option<&'a str>,
    pub size_bytes: u64,
    pub label: &'a str,
    /// Optional Bearer token (Hugging Face gated models). Env `HF_TOKEN` is used when this is None.
    pub auth_token: Option<&'a str>,
}

/// Downloads with resume (`.part` file + Range requests) and SHA-256 verification.
/// Returns the file's SHA-256.
pub fn download(req: &Request) -> anyhow::Result<String> {
    if let Some(sha) = req.sha256 {
        if is_verified(req.dest, sha) {
            ui::ok(format!("{} already downloaded and verified", req.label));
            return Ok(sha.to_lowercase());
        }
        if req.dest.is_file() {
            let pb = ui::spinner(format!("Checking existing {}", req.label));
            let got = sha256_file(req.dest)?;
            pb.finish_and_clear();
            if got.eq_ignore_ascii_case(sha) {
                std::fs::write(marker(req.dest), &got)?;
                ui::ok(format!("{} already downloaded and verified", req.label));
                return Ok(got);
            }
            std::fs::remove_file(req.dest)?;
        }
    }
    let parent = req.dest.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    let part = part(req.dest);
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let free = super::hardware::free_disk_gb(parent) * 1024f64.powi(3);
    let need = req.size_bytes.saturating_sub(have) as f64;
    if free > 0.0 && need + 512.0 * 1024.0 * 1024.0 > free {
        return Err(CliError::new(
            Exit::Resources,
            format!(
                "not enough disk space for {}: needs {}, {} free",
                req.label,
                ui::gb(need / 1e9),
                ui::gb(free / 1e9)
            ),
        )
        .hint("free some disk space, or choose a smaller model / quantization")
        .into());
    }

    let client = client()?;
    let mut attempts = 0;
    loop {
        attempts += 1;
        match fetch_into(&client, req, &part) {
            Ok(()) => break,
            Err(e) if attempts < 4 && is_retryable(&e) => {
                ui::warn(format!("download interrupted ({e}), resuming…"));
                std::thread::sleep(Duration::from_secs(2 * attempts));
            }
            Err(e) => {
                let msg = e.to_string();
                let hint = if msg.contains("401") || msg.contains("403") {
                    "this may be a gated Hugging Face model: accept the license on the model page, set HF_TOKEN or .yourai/secrets.toml hf_token, then run `yourai build` again"
                } else {
                    "run the same command again; the download resumes where it stopped"
                };
                return Err(
                    CliError::new(Exit::Download, format!("download of {} failed: {e}", req.label))
                        .hint(hint)
                        .into(),
                );
            }
        }
    }

    let pb = ui::spinner(format!("Verifying {}", req.label));
    let got = sha256_file(&part)?;
    pb.finish_and_clear();
    if let Some(sha) = req.sha256 {
        if !got.eq_ignore_ascii_case(sha) {
            let _ = std::fs::remove_file(&part);
            return Err(CliError::new(
                Exit::Download,
                format!(
                    "checksum mismatch for {} (expected {sha}, got {got}); the file was deleted",
                    req.label
                ),
            )
            .hint("run `yourai update` to refresh the catalog, then try again")
            .into());
        }
    }
    std::fs::rename(&part, req.dest)?;
    std::fs::write(marker(req.dest), &got)?;
    ui::ok(format!("{} downloaded and verified", req.label));
    Ok(got)
}

fn is_retryable(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<std::io::Error>().is_some()
            || c.downcast_ref::<reqwest::Error>()
                .is_some_and(|r| r.is_timeout() || r.is_body() || r.is_connect())
    })
}

fn fetch_into(client: &reqwest::blocking::Client, req: &Request, part: &Path) -> anyhow::Result<()> {
    let have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mut rb = client.get(req.url);
    if have > 0 {
        rb = rb.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    if req.url.starts_with("https://huggingface.co/") {
        if let Some(tok) = req.auth_token.filter(|t| !t.is_empty()) {
            rb = rb.bearer_auth(tok);
        } else if let Ok(tok) = std::env::var("HF_TOKEN") {
            if !tok.trim().is_empty() {
                rb = rb.bearer_auth(tok.trim());
            }
        }
    }
    let mut resp = rb.send()?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have > 0 {
        return Ok(()); // already complete; verification decides
    }
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            anyhow::bail!(
                "server answered {status} for {} (gated or private Hugging Face file? set HF_TOKEN / hf_token and accept the model license)",
                req.url
            );
        }
        anyhow::bail!("server answered {status} for {}", req.url);
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut file = if resumed {
        OpenOptions::new().append(true).open(part)?
    } else {
        File::create(part)?
    };
    let start = if resumed { have } else { 0 };
    let total = resp.content_length().map(|l| l + start).unwrap_or(req.size_bytes);

    let pb = indicatif::ProgressBar::new(total);
    if ui::interactive() {
        pb.set_style(
            indicatif::ProgressStyle::with_template(
                "{msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ETA {eta}",
            )
            .unwrap()
            .progress_chars("=> "),
        );
    } else {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        println!("Downloading {}…", req.label);
    }
    pb.set_message(req.label.to_string());
    pb.set_position(start);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        pb.inc(n as u64);
    }
    file.flush()?;
    pb.finish_and_clear();
    let len = std::fs::metadata(part)?.len();
    if total > 0 && len < total {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed early").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resumes_from_part_file() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/f.bin"))
            .and(header("range", "bytes=50000-"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body[50_000..].to_vec()))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        std::fs::write(part(&dest), &body[..50_000]).unwrap();
        let url = format!("{}/f.bin", server.uri());
        let expected = sha(&body);
        let d = dest.clone();
        let got = tokio::task::spawn_blocking(move || {
            download(&Request {
                url: &url,
                dest: &d,
                sha256: Some(&expected),
                size_bytes: 200_000,
                label: "test",
                auth_token: None,
            })
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(got, sha(&body));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(is_verified(&dest, &got));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn checksum_mismatch_deletes_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bad.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"tampered".to_vec()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bad.bin");
        let url = format!("{}/bad.bin", server.uri());
        let d = dest.clone();
        let err = tokio::task::spawn_blocking(move || {
            download(&Request {
                url: &url,
                dest: &d,
                sha256: Some(&"0".repeat(64)),
                size_bytes: 8,
                label: "test",
                auth_token: None,
            })
        })
        .await
        .unwrap()
        .unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"));
        assert!(!dest.exists() && !part(&dest).exists());
    }
}

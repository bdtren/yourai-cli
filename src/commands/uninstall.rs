use std::path::{Path, PathBuf};

use super::self_update::{current_exe, detect_method, Method};
use crate::core::paths;
use crate::error::{fail_hint, CliError, Exit};
use crate::ui;

const MARKER: &str = "# added by yourai installer";

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| {
            let m = e.metadata();
            match m {
                Ok(m) if m.is_dir() => dir_size(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        })
        .sum()
}

fn shell_profiles() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    [".zshrc", ".bashrc", ".bash_profile", ".profile"]
        .iter()
        .map(|f| home.join(f))
        .filter(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains(MARKER)))
        .collect()
}

/// The extra symlink install.sh puts in a folder already on PATH, if it still points at `exe`.
fn installer_link(exe: &Path) -> Option<PathBuf> {
    let receipt: serde_json::Value = std::fs::read_to_string(paths::receipt())
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())?;
    let link = PathBuf::from(receipt["link"].as_str()?);
    let is_link = std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
    (is_link && std::fs::canonicalize(&link).ok()? == exe).then_some(link)
}

/// Removes the two lines the installer added (marker + export).
fn strip_profile(p: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(p)?;
    let mut out = Vec::new();
    let mut skip_next = false;
    for line in text.lines() {
        if skip_next {
            skip_next = false;
            if line.contains(".yourai/bin") {
                continue;
            }
        }
        if line.trim() == MARKER {
            skip_next = true;
            continue;
        }
        out.push(line);
    }
    std::fs::write(p, out.join("\n") + "\n")?;
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    let exe = current_exe();
    match detect_method(&exe) {
        Method::Homebrew => {
            ui::info("installed with Homebrew. Remove it with:\n  brew uninstall yourai");
            return Ok(());
        }
        Method::Npm => {
            ui::info("installed with npm. Remove it with:\n  npm uninstall -g yourai");
            return Ok(());
        }
        Method::Unknown => {
            return fail_hint(
                Exit::Unsupported,
                format!("yourai at {} was not installed by the yourai installer", exe.display()),
                "delete that file yourself; models and engines are in ~/.yourai",
            );
        }
        Method::Script => {}
    }
    if !ui::interactive() {
        return fail_hint(
            Exit::Cancelled,
            "uninstall needs an interactive terminal to confirm",
            "run it in a terminal",
        );
    }

    ui::heading("This will remove");
    ui::info(format!("the yourai CLI: {}", exe.display()));
    let link = installer_link(&exe);
    if let Some(l) = &link {
        ui::info(format!("the link {}", l.display()));
    }
    let profiles = shell_profiles();
    for p in &profiles {
        ui::info(format!("the PATH line added to {}", p.display()));
    }
    if cfg!(windows) {
        ui::info(format!("{} from your user PATH", paths::bin().display()));
    }
    ui::info("Your projects are never touched.");
    if !inquire::Confirm::new("Remove the yourai CLI?")
        .with_default(false)
        .prompt()?
    {
        return Err(CliError::new(Exit::Cancelled, "cancelled").into());
    }

    let data = [paths::models(), paths::engines(), paths::downloads()];
    let size: u64 = data.iter().map(|d| dir_size(d)).sum();
    let delete_data = size > 0
        && inquire::Confirm::new(&format!(
            "Also delete downloaded models and engines in {} ({})? They are slow to download again.",
            paths::home().display(),
            ui::gb(size as f64 / 1e9)
        ))
        .with_default(false)
        .prompt()?;

    for p in &profiles {
        strip_profile(p)?;
    }
    if let Some(l) = &link {
        let _ = std::fs::remove_file(l);
    }
    #[cfg(windows)]
    {
        let bin = paths::bin().display().to_string();
        let script = format!(
            "$p=[Environment]::GetEnvironmentVariable('Path','User'); \
             $n=($p -split ';' | Where-Object {{ $_ -and $_ -ne '{bin}' }}) -join ';'; \
             [Environment]::SetEnvironmentVariable('Path',$n,'User')"
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status();
    }
    if delete_data {
        for d in &data {
            let _ = std::fs::remove_dir_all(d);
        }
        let _ = std::fs::remove_file(paths::catalog_cache());
        ui::ok("deleted models and engines");
    }
    let _ = std::fs::remove_file(paths::receipt());
    let _ = std::fs::remove_file(paths::update_check());
    if cfg!(windows) {
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
        ui::ok(format!(
            "removed yourai (delete {} after closing this window)",
            old.display()
        ));
    } else {
        std::fs::remove_file(&exe)?;
        ui::ok("removed yourai. Open a new terminal to refresh PATH.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_installer_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".zshrc");
        std::fs::write(
            &p,
            format!("alias x=y\n\n{MARKER}\nexport PATH=\"/h/.yourai/bin:$PATH\"\necho hi\n"),
        )
        .unwrap();
        strip_profile(&p).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(!t.contains("yourai"));
        assert!(t.contains("alias x=y") && t.contains("echo hi"));
    }
}

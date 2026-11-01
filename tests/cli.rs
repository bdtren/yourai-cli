use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

/// Runs yourai with an isolated home and no network access to the real site.
fn yourai(home: &Path, cwd: &Path) -> Command {
    let mut c = Command::cargo_bin("yourai").unwrap();
    c.current_dir(cwd)
        .env("YOURAI_HOME", home)
        .env("YOURAI_BASE_URL", "http://127.0.0.1:9")
        .env("YOURAI_NO_UPDATE_CHECK", "1")
        .env("NO_COLOR", "1")
        .env_remove("YOURAI_PROJECT")
        .env_remove("YOURAI_API_KEY");
    c
}

#[test]
fn version_and_help() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    yourai(t.path(), t.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor").and(predicate::str::contains("init")));
}

#[test]
fn doctor_reports_hardware() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Your computer").and(predicate::str::contains("Engine build")));
}

#[test]
fn commands_outside_a_project_explain_what_to_do() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .arg("status")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a YourAI project"));
}

#[test]
fn init_yes_creates_valid_project_and_config_edits_work() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "demo",
            "--model",
            "qwen2.5-0.5b",
            "--quant",
            "Q4_K_M",
            "--port",
            "18765",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created demo"));

    let proj = t.path().join("demo");
    for f in [
        "yourai.toml",
        "prompts/system.md",
        "README.md",
        ".gitignore",
        "scripts/start.sh",
        "scripts/stop.ps1",
    ] {
        assert!(proj.join(f).is_file(), "missing {f}");
    }
    yourai(&home, &proj).args(["config", "validate"]).assert().success();
    yourai(&home, &proj)
        .args(["config", "get", "model.id"])
        .assert()
        .success()
        .stdout("qwen2.5-0.5b\n");
    yourai(&home, &proj)
        .args(["config", "set", "runtime.context_length", "8192"])
        .assert()
        .success();
    yourai(&home, &proj)
        .args(["config", "get", "runtime.context_length"])
        .assert()
        .stdout("8192\n");
    yourai(&home, &proj)
        .args(["config", "set", "server.port", "nope"])
        .assert()
        .code(2);
    yourai(&home, &proj)
        .args(["config", "set", "server.host", "0.0.0.0"])
        .assert()
        .code(6);

    // Re-running init into the same folder never overwrites it.
    yourai(&home, t.path())
        .args(["init", "--yes", "--name", "demo"])
        .assert()
        .failure();

    // Without a terminal and without --accept-license, build stops before downloading.
    yourai(&home, &proj)
        .arg("build")
        .assert()
        .code(7)
        .stderr(predicate::str::contains("--accept-license"));
    assert!(!home.join("models").join("qwen2.5-0.5b").exists());

    yourai(&home, &proj)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("stopped"));
    yourai(&home, &proj)
        .arg("start")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("yourai build"));
}

#[test]
fn lan_access_generates_a_secret_outside_the_config() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "lan",
            "--model",
            "qwen2.5-0.5b",
            "--access",
            "lan",
            "--port",
            "18766",
        ])
        .assert()
        .success();
    let proj = t.path().join("lan");
    let toml = std::fs::read_to_string(proj.join("yourai.toml")).unwrap();
    assert!(toml.contains("require_api_key = true"));
    let secret = std::fs::read_to_string(proj.join(".yourai/secrets.toml")).unwrap();
    let key = secret.lines().find(|l| l.starts_with("api_key")).unwrap();
    let value = key.split('"').nth(1).unwrap();
    assert!(!toml.contains(value));
    assert!(std::fs::read_to_string(proj.join(".gitignore"))
        .unwrap()
        .contains(".yourai/"));
}

#[test]
fn invalid_flag_values_are_rejected_with_reasons() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "x",
            "--quant",
            "Q9_FAKE",
            "--model",
            "qwen2.5-0.5b",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a valid choice"));
}

#[test]
fn capabilities_are_configured_and_media_commands_need_a_running_ai() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "media",
            "--model",
            "qwen2.5-0.5b",
            "--capabilities",
            "agent,image,stt",
            "--image-model",
            "sd-turbo",
            "--stt-model",
            "whisper-base",
            "--port",
            "18767",
        ])
        .assert()
        .success();
    let proj = t.path().join("media");
    for (key, want) in [
        ("agent.enabled", "true"),
        ("image.enabled", "true"),
        ("image.model", "sd-turbo"),
        ("speech_to_text.model", "whisper-base"),
        ("video.enabled", "false"),
        ("text_to_speech.enabled", "false"),
    ] {
        yourai(&home, &proj)
            .args(["config", "get", key])
            .assert()
            .success()
            .stdout(format!("{want}\n"));
    }
    yourai(&home, &proj)
        .args(["config", "set", "agent.tools", "read_file,generate_image"])
        .assert()
        .success();
    yourai(&home, &proj)
        .args(["config", "set", "agent.tools", "read_file,launch_rockets"])
        .assert()
        .code(2);
    yourai(&home, &proj)
        .args(["image", "a red fox"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("yourai start"));

    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "bad",
            "--model",
            "qwen2.5-0.5b",
            "--capabilities",
            "image",
            "--image-model",
            "whisper-base",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a valid choice"));
}

#[test]
fn completions_print() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("yourai"));
}

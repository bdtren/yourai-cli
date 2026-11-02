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
        "web/index.html",
        "api/routes.toml",
        "api/handlers/example_echo.js",
    ] {
        assert!(proj.join(f).is_file(), "missing {f}");
    }
    assert!(!proj.join("web/api").exists());
    yourai(&home, &proj).args(["config", "validate"]).assert().success();
    yourai(&home, &proj)
        .args(["config", "get", "model.id"])
        .assert()
        .success()
        .stdout("qwen2.5-0.5b\n");
    yourai(&home, &proj)
        .args(["config", "get", "server.customize_web_ui"])
        .assert()
        .success()
        .stdout("true\n");
    yourai(&home, &proj)
        .args(["config", "set", "server.customize_web_ui", "false"])
        .assert()
        .success();
    yourai(&home, &proj)
        .args(["config", "get", "server.customize_web_ui"])
        .assert()
        .stdout("false\n");
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
fn start_refuses_legacy_absolute_lock_without_modules() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "legacy",
            "--model",
            "qwen2.5-0.5b",
            "--port",
            "18767",
        ])
        .assert()
        .success();
    let proj = t.path().join("legacy");
    let abs_model = home.join("models/qwen2.5-0.5b/f.gguf");
    std::fs::create_dir_all(abs_model.parent().unwrap()).unwrap();
    std::fs::write(&abs_model, b"fake").unwrap();
    let abs_exe = home.join("engines/llama/bin/llama-server");
    std::fs::create_dir_all(abs_exe.parent().unwrap()).unwrap();
    std::fs::write(&abs_exe, b"fake").unwrap();
    let lock = serde_json::json!({
        "sections": {},
        "engine_version": "test",
        "engine_key": "test",
        "engine_exe": abs_exe.to_string_lossy(),
        "model_id": "qwen2.5-0.5b",
        "model_quant": "Q4_K_M",
        "model_path": abs_model.to_string_lossy(),
        "model_sha256": "0".repeat(64),
    });
    std::fs::write(proj.join("yourai.lock"), serde_json::to_string_pretty(&lock).unwrap()).unwrap();
    assert!(!proj.join("yourai_modules").exists());
    yourai(&home, &proj)
        .arg("start")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("not been built yet").and(predicate::str::contains("yourai build")));
    assert!(!proj.join("yourai_modules").exists());
    // Start must not rewrite the lock into yourai_modules/ paths.
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("yourai.lock")).unwrap()).unwrap();
    let model = after["model_path"].as_str().unwrap();
    assert!(std::path::Path::new(model).is_absolute());
    assert!(!model.replace('\\', "/").starts_with("yourai_modules/"));
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
    let gi = std::fs::read_to_string(proj.join(".gitignore")).unwrap();
    assert!(gi.contains(".yourai/"));
    assert!(gi.contains("yourai_modules/"));
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

#[test]
fn remote_access_writes_deploy_stub_under_yes() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "remote-demo",
            "--model",
            "qwen2.5-0.5b",
            "--access",
            "remote",
            "--port",
            "18767",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("deploy stub").and(predicate::str::contains("yourai deploy -e")));
    let proj = t.path().join("remote-demo");
    let stub = std::fs::read_to_string(proj.join(".yourai/deploy.toml")).unwrap();
    assert!(stub.contains("incomplete = true"));
    assert!(proj.join(".yourai/deploy-README.md").is_file());
    let toml = std::fs::read_to_string(proj.join("yourai.toml")).unwrap();
    assert!(toml.contains("host = \"127.0.0.1\""));
}

#[test]
fn deploy_dry_run_local_expose_from_saved_config() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    yourai(&home, t.path())
        .args([
            "init",
            "--yes",
            "--name",
            "expose-demo",
            "--model",
            "qwen2.5-0.5b",
            "--port",
            "18768",
        ])
        .assert()
        .success();
    let proj = t.path().join("expose-demo");
    std::fs::write(
        proj.join(".yourai/deploy.toml"),
        r#"
version = 1
mode = "local_expose"
expose_via = "ip"
"#,
    )
    .unwrap();
    yourai(&home, &proj)
        .args(["deploy", "--dry-run"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Local expose")
                .and(predicate::str::contains("[dry-run]"))
                .and(predicate::str::contains("0.0.0.0")),
        );
    assert!(proj.join(".yourai/deploy-README.md").is_file());
}

#[test]
fn deploy_help_lists_edit_and_dry_run() {
    let t = tempfile::tempdir().unwrap();
    yourai(t.path(), t.path())
        .args(["deploy", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--edit")
                .and(predicate::str::contains("--dry-run"))
                .and(predicate::str::contains("remote")),
        );
}

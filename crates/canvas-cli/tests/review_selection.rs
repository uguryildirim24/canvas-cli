//! Profile, environment, and identity containment regressions.
#![cfg(unix)]
mod common;
use common::{TestHome, host_from_server, mock_users_self};
use std::fs;
use wiremock::MockServer;

#[tokio::test]
async fn login_honors_env_profile_ignores_env_host_and_defaults_to_default_label() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 77, "Profile").await;
    let host = host_from_server(&server);
    home.cmd()
        .env("CANVAS_PROFILE", "school")
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("school-token")
        .assert()
        .success();
    home.cmd()
        .env("CANVAS_PROFILE", "school")
        .env("CANVAS_HOST", "ignored.invalid")
        .env("CANVAS_TOKEN", "school-token")
        .args(["auth", "login"])
        .assert()
        .success();
    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("default-token")
        .assert()
        .success();
    let raw = fs::read_to_string(home.config_dir.join("config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&raw).unwrap();
    assert_eq!(config["default_profile"].as_str(), Some("school"));
    assert!(config["profiles"].get("default").is_some());
    assert!(config["profiles"].get("school").is_some());
}

#[test]
fn config_environment_preserves_underscores_and_is_not_saved() {
    let home = TestHome::new();
    home.cmd()
        .env("CANVAS_NETWORK_API_CONCURRENCY", "7")
        .args(["config", "get", "network.api_concurrency"])
        .assert()
        .success()
        .stdout("7\n");
    home.cmd()
        .env("CANVAS_CACHE_TTL_COURSES", "2h")
        .args(["config", "get", "cache.ttl_courses"])
        .assert()
        .success()
        .stdout("2h\n");
    home.cmd()
        .env("CANVAS_DEFAULT_PROFILE", "temporary")
        .args(["config", "get", "default_profile"])
        .assert()
        .success()
        .stdout("temporary\n");
    home.cmd()
        .env("CANVAS_NETWORK_API_CONCURRENCY", "7")
        .args(["config", "set", "output.color", "never"])
        .assert()
        .success();
    home.cmd()
        .args(["config", "get", "network.api_concurrency"])
        .assert()
        .success()
        .stdout("4\n");
}

#[tokio::test]
async fn bound_env_identity_wrong_origin_is_removed_and_local_command_never_fetches() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 78, "Binding").await;
    let host = host_from_server(&server);
    home.cmd()
        .env("CANVAS_HOST", &host)
        .env("CANVAS_TOKEN", "env-token")
        .args(["auth", "login"])
        .assert()
        .success();
    let path = home.data_dir.join("env-bindings.toml");
    let mut bindings: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let doc = canvas_core::identity::IdentityDocument::new(
        "https://other.invalid",
        78,
        "2026-09-09T00:00:00Z",
    );
    let core = canvas_core::identity::Paths::for_identity(&home.data_dir, &doc.key);
    doc.write(&core.identity_json()).unwrap();
    for (_, entry) in bindings["bindings"].as_table_mut().unwrap() {
        entry["key"] = toml::Value::String(doc.key.to_string());
    }
    fs::write(&path, toml::to_string(&bindings).unwrap()).unwrap();
    let before = server.received_requests().await.unwrap().len();
    home.cmd()
        .env("CANVAS_HOST", &host)
        .env("CANVAS_TOKEN", "env-token")
        .args(["auth", "status", "--offline"])
        .assert()
        .code(3);
    assert_eq!(server.received_requests().await.unwrap().len(), before);
    let raw = fs::read_to_string(path).unwrap();
    assert!(!raw.contains(doc.key.as_str()));
}

#[tokio::test]
async fn concurrent_binding_writes_and_logins_preserve_both_bindings() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 79, "Concurrent").await;
    let host = host_from_server(&server);
    // Different env tokens, same identity; profile writes are independently serialized.
    let mut children = Vec::new();
    for token in ["one", "two"] {
        children.push(
            home.std_cmd()
                .env("CANVAS_HOST", &host)
                .env("CANVAS_TOKEN", token)
                .args(["auth", "login"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw = fs::read_to_string(home.data_dir.join("env-bindings.toml")).unwrap();
    let bindings: toml::Value = toml::from_str(&raw).unwrap();
    assert_eq!(bindings["bindings"].as_table().unwrap().len(), 2);
}

#[test]
fn dot_operands_are_rejected_before_path_access() {
    let home = TestHome::new();
    for key in [".", "..", "../escape", "/tmp", "C:\\escape"] {
        home.cmd()
            .args(["identity", "remove", key, "--yes"])
            .assert()
            .code(13);
    }
    assert!(!home.data_dir.join("locks").exists());
}

#[test]
fn insecure_origin_refused_before_credentials_or_network() {
    let home = TestHome::new();
    home.cmd()
        .env_remove("CANVAS_TEST_ALLOW_HTTP")
        .args([
            "auth",
            "login",
            "--host",
            "http://127.0.0.1:9",
            "--token-stdin",
        ])
        .write_stdin("test-token")
        .assert()
        .code(2);
    assert!(!home.config_dir.join("credentials.toml").exists());
}

#[test]
fn flags_override_environment_and_config_values() {
    let home = TestHome::new();
    home.cmd()
        .args(["config", "set", "output.color", "never"])
        .assert()
        .success();
    home.cmd()
        .env("CANVAS_OUTPUT_COLOR", "always")
        .args(["config", "get", "output.color"])
        .assert()
        .success()
        .stdout("always\n");
    home.cmd()
        .env("CANVAS_OUTPUT_COLOR", "always")
        .args(["--color", "auto", "config", "get", "output.color"])
        .assert()
        .success()
        .stdout("auto\n");
}

#[tokio::test]
async fn concurrent_named_logins_preserve_both_profiles() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 80, "Profiles").await;
    let mut children = Vec::new();
    for profile in ["one", "two"] {
        children.push(
            home.std_cmd()
                .env("CANVAS_TOKEN", "profile-token")
                .args([
                    "--profile",
                    profile,
                    "auth",
                    "login",
                    "--host",
                    &server.uri(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw = fs::read_to_string(home.config_dir.join("config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&raw).unwrap();
    assert_eq!(config["profiles"].as_table().unwrap().len(), 2);
}

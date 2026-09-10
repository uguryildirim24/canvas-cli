//! Doctor command integration tests.
#![cfg(unix)]

mod common;

use common::{TestHome, host_from_server, mock_users_self};
use predicates::prelude::*;
use wiremock::MockServer;

#[tokio::test]
async fn identity_free_checks_ok() {
    let home = TestHome::new();
    home.cmd()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"identity_selected\": false"))
        .stdout(predicate::str::contains("\"checks\""));
}

#[tokio::test]
async fn with_identity_and_network() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 33, "Doc").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-doc")
        .assert()
        .success();

    home.cmd()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"identity_selected\": true"));

    home.cmd()
        .args(["doctor", "--network", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("network_users_self"));
}

#[tokio::test]
async fn unbound_env_pair_online_binds_without_creating_profile_or_storing_token() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/api/v1/users/self"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("X-Rate-Limit-Remaining", "123")
                .insert_header("X-Request-Cost", "0.5")
                .insert_header("Date", "Wed, 09 Sep 2026 12:00:00 GMT")
                .set_body_json(serde_json::json!({"id": 91, "name": "Env"})),
        )
        .mount(&server)
        .await;
    let output = home
        .cmd()
        .env("CANVAS_HOST", server.uri())
        .env("CANVAS_TOKEN", "ephemeral-token")
        .args(["doctor", "--network", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["profile"], "env");
    assert_eq!(value["requests"]["api"], 1);
    assert_eq!(value["requests"]["cost"], 0.5);
    let checks = value["result"]["checks"].as_array().unwrap();
    assert!(
        checks
            .iter()
            .any(|c| c["name"] == "network_rate_limit" && c["message"] == "remaining=123")
    );
    assert!(
        checks
            .iter()
            .any(|c| c["name"] == "network_clock_skew" && c["status"] == "ok")
    );
    assert!(home.data_dir.join("env-bindings.toml").exists());
    assert!(!home.config_dir.join("credentials.toml").exists());
    assert!(!home.config_dir.join("config.toml").exists());
    home.cmd()
        .env("CANVAS_HOST", server.uri())
        .env("CANVAS_TOKEN", "ephemeral-token")
        .args(["auth", "status", "--offline"])
        .assert()
        .success();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[test]
fn corrupt_config_has_matching_nonzero_process_and_envelope_exit() {
    let home = TestHome::new();
    std::fs::write(home.config_dir.join("config.toml"), "[broken").unwrap();
    let output = home.cmd().args(["doctor", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(12));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["exit"], 12);
    assert_eq!(value["outcome"], "partial");
}

#[tokio::test]
async fn network_rejection_is_an_auth_abort_with_one_envelope() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let output = home
        .cmd()
        .env("CANVAS_HOST", server.uri())
        .env("CANVAS_TOKEN", "rejected-token")
        .args(["doctor", "--network", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/error@1");
    assert_eq!(value["result"]["http_status"], 401);
    assert!(!home.data_dir.join("env-bindings.toml").exists());
}

#[tokio::test]
async fn mismatched_user_never_changes_identity_state() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 92, "Expected").await;
    home.cmd()
        .args(["auth", "login", "--host", &server.uri(), "--token-stdin"])
        .write_stdin("original-token")
        .assert()
        .success();
    server.reset().await;
    mock_users_self(&server, 93, "Other").await;
    home.cmd()
        .env("CANVAS_TOKEN", "other-token")
        .args(["doctor", "--network"])
        .assert()
        .code(3);
    home.cmd()
        .args(["auth", "token", "--reveal"])
        .assert()
        .success()
        .stdout("original-token\n");
}

#[tokio::test]
async fn already_bound_env_token_cannot_silently_change_users() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 94, "Original").await;
    home.cmd()
        .env("CANVAS_HOST", server.uri())
        .env("CANVAS_TOKEN", "bound-token")
        .args(["auth", "login"])
        .assert()
        .success();
    let path = home.data_dir.join("env-bindings.toml");
    let before = std::fs::read(&path).unwrap();
    server.reset().await;
    mock_users_self(&server, 95, "Changed").await;
    home.cmd()
        .env("CANVAS_HOST", server.uri())
        .env("CANVAS_TOKEN", "bound-token")
        .args(["doctor", "--network"])
        .assert()
        .code(3);
    assert_eq!(std::fs::read(path).unwrap(), before);
    let new_key = canvas_core::identity::IdentityKey::compute(&server.uri(), 95);
    assert!(!home.data_dir.join(new_key.as_str()).exists());
}

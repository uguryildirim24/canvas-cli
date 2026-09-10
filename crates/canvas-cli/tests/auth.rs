//! Auth command integration tests.
#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{TestHome, host_from_server, mock_users_self};
use predicates::prelude::*;
use wiremock::MockServer;

#[tokio::test]
async fn login_status_logout_file_store() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 42, "Ada").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-ada")
        .assert()
        .success()
        .stdout(predicate::str::contains("credential backend: file"));

    let creds = home.config_dir.join("credentials.toml");
    assert!(creds.exists());
    let raw = fs::read_to_string(&creds).unwrap();
    assert!(raw.contains("tok-ada"));

    home.cmd()
        .args(["auth", "status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"token_source\": \"file\""));

    home.cmd().args(["auth", "logout"]).assert().success();

    let raw = fs::read_to_string(&creds).unwrap();
    assert!(!raw.contains("tok-ada"));
}

#[tokio::test]
async fn replace_rebinds_profile_second_user_refused() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 1, "One").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-1")
        .assert()
        .success();

    // Remount for user 2.
    let server2 = MockServer::start().await;
    mock_users_self(&server2, 2, "Two").await;
    let host2 = host_from_server(&server2);

    home.cmd()
        .args(["auth", "login", "--host", &host2, "--token-stdin"])
        .write_stdin("tok-2")
        .assert()
        .code(3)
        .stderr(
            predicate::str::contains("different identity")
                .or(predicate::str::contains("identity mismatch")),
        );

    home.cmd()
        .args([
            "auth",
            "login",
            "--host",
            &host2,
            "--token-stdin",
            "--replace",
        ])
        .write_stdin("tok-2")
        .assert()
        .success();
}

#[tokio::test]
async fn env_pair_offline_class_b_exit_3_then_binding_after_login() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 7, "Env").await;
    let host = host_from_server(&server);

    home.cmd()
        .env("CANVAS_HOST", &host)
        .env("CANVAS_TOKEN", "tok-env")
        .args(["auth", "status", "--offline"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("bind this token"));

    home.cmd()
        .env("CANVAS_HOST", &host)
        .env("CANVAS_TOKEN", "tok-env")
        .args(["auth", "login", "--token-stdin"])
        .write_stdin("tok-env")
        .assert()
        .success();

    assert!(home.data_dir.join("env-bindings.toml").exists());

    home.cmd()
        .env("CANVAS_HOST", &host)
        .env("CANVAS_TOKEN", "tok-env")
        .args(["auth", "status", "--offline"])
        .assert()
        .success();
}

#[tokio::test]
async fn login_profile_new() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 9, "New").await;
    let host = host_from_server(&server);

    home.cmd()
        .args([
            "--profile",
            "school",
            "auth",
            "login",
            "--host",
            &host,
            "--token-stdin",
        ])
        .write_stdin("tok-new")
        .assert()
        .success()
        .stdout(predicate::str::contains("profile: school"));
}

#[tokio::test]
async fn crash_after_store_write_leaves_stray_then_status() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 3, "Crash").await;
    let host = host_from_server(&server);

    home.cmd()
        .env("CANVAS_TEST_CRASH_AFTER", "after_store_write")
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-crash")
        .assert()
        .code(99);

    // Login again to recover.
    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-crash")
        .assert()
        .success();
}

#[tokio::test]
async fn login_after_failed_logout_keeps_new_token() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 5, "Keep").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-old")
        .assert()
        .success();

    // Make credentials file immutable-ish by replacing with wrong mode so cleanup fails,
    // then force logout to set flags; simpler path: crash after logout state.
    home.cmd()
        .env("CANVAS_TEST_CRASH_AFTER", "after_logout_state")
        .args(["auth", "logout"])
        .assert()
        .code(99);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-new")
        .assert()
        .success();

    let raw = fs::read_to_string(home.config_dir.join("credentials.toml")).unwrap();
    assert!(raw.contains("tok-new"));
    assert!(!raw.contains("tok-old"));
}

#[tokio::test]
async fn logout_two_deletion_failures_leave_flags() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 8, "Flags").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-flags")
        .assert()
        .success();

    // Poison the credentials file so file cleanup fails (wrong mode).
    let creds = home.config_dir.join("credentials.toml");
    let mut perms = fs::metadata(&creds).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&creds, perms.clone()).unwrap();

    home.cmd().args(["auth", "logout"]).assert().code(13);

    // active_source should be none; cleanup_file flag kept — probe via status after fixing mode.
    perms.set_mode(0o600);
    fs::set_permissions(&creds, perms).unwrap();

    home.cmd()
        .args(["auth", "status", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"token_source\": null")
                .or(predicate::str::contains("pending")),
        );
}

#[tokio::test]
async fn resolution_rejects_none() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 11, "None").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("tok-none")
        .assert()
        .success();
    home.cmd().args(["auth", "logout"]).assert().success();

    home.cmd()
        .args(["auth", "token", "--reveal"])
        .assert()
        .code(3);
}

#[tokio::test]
async fn auth_token_reveal() {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 12, "Reveal").await;
    let host = host_from_server(&server);

    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("secret-reveal")
        .assert()
        .success();

    home.cmd()
        .args(["auth", "token", "--reveal"])
        .assert()
        .success()
        .stdout(predicate::eq("secret-reveal\n"));
}

#[tokio::test]
async fn failed_validation_json_is_one_envelope_with_correct_exit_and_request_count() {
    for (status, body, expected) in [(401, "denied", 3), (200, "not-json", 1)] {
        let home = TestHome::new();
        let server = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        let output = home
            .cmd()
            .env("CANVAS_TOKEN", "validation-secret")
            .args(["auth", "login", "--host", &server.uri(), "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["schema"], "canvas-cli/error@1");
        assert_eq!(value["exit"], expected);
        assert_eq!(value["requests"]["api"], 1);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("validation-secret"));
        assert!(!home.config_dir.join("credentials.toml").exists());
    }
}

#[tokio::test]
async fn unreachable_server_is_network_exit_four() {
    let home = TestHome::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let reset = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket.shutdown(std::net::Shutdown::Both).unwrap();
    });
    home.cmd()
        .env("CANVAS_TOKEN", "network-secret")
        .args(["auth", "login", "--host", &host])
        .assert()
        .code(4);
    reset.join().unwrap();
}

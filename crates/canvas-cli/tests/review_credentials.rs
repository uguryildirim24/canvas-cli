//! Adversarial credential persistence and redaction regressions for M0-c.
#![cfg(unix)]
mod common;
use common::{TestHome, host_from_server, mock_users_self};
use predicates::prelude::*;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use wiremock::MockServer;

async fn login_home() -> (TestHome, MockServer, String) {
    let home = TestHome::new();
    let server = MockServer::start().await;
    mock_users_self(&server, 401, "Review").await;
    let host = host_from_server(&server);
    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("old-secret")
        .assert()
        .success();
    (home, server, host)
}

fn row(home: &TestHome) -> (String, bool, bool) {
    let path = fs::read_dir(&home.data_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path().join("state.sqlite"))
        .find(|p| p.exists())
        .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.query_row(
        "SELECT active_source, cleanup_keyring, cleanup_file FROM credential",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[tokio::test]
async fn rotation_crash_reports_stray_then_pending_and_recovers() {
    for step in ["after_store_write", "after_state_commit", "after_cleanup"] {
        let (home, _server, host) = login_home().await;
        home.cmd()
            .env("CANVAS_TEST_CRASH_AFTER", step)
            .args(["auth", "login", "--host", &host, "--token-stdin"])
            .write_stdin("rotated-secret")
            .assert()
            .code(99);
        let output = home
            .cmd()
            .args(["auth", "status", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if step == "after_store_write" {
            assert_eq!(
                value["result"]["stray_sources"],
                serde_json::json!(["file"])
            );
            home.cmd()
                .args(["auth", "token", "--reveal"])
                .assert()
                .code(3);
        } else {
            home.cmd()
                .args(["auth", "token", "--reveal"])
                .assert()
                .success()
                .stdout("rotated-secret\n");
            if step == "after_state_commit" {
                assert_eq!(
                    value["result"]["pending_cleanup"],
                    serde_json::json!(["keyring"])
                );
            }
        }
        home.cmd()
            .args(["auth", "login", "--host", &host, "--token-stdin"])
            .write_stdin("recovered-secret")
            .assert()
            .success();
        home.cmd()
            .args(["auth", "token", "--reveal"])
            .assert()
            .success()
            .stdout("recovered-secret\n");
    }
}

#[tokio::test]
async fn unavailable_backend_and_two_failed_deletions_keep_durable_flags() {
    let (home, _server, host) = login_home().await;
    for _ in 0..2 {
        home.cmd()
            .env("CANVAS_TEST_KEYRING_ERROR", "backend")
            .args(["auth", "login", "--host", &host, "--token-stdin"])
            .write_stdin("new-secret")
            .assert()
            .success();
        assert_eq!(row(&home), ("file".into(), true, false));
    }
    let path = home.config_dir.join("credentials.toml");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    home.cmd()
        .env("CANVAS_TEST_KEYRING_ERROR", "backend")
        .args(["auth", "logout"])
        .assert()
        .code(13);
    assert_eq!(row(&home), ("none".into(), true, true));
    home.cmd()
        .args(["auth", "token", "--reveal"])
        .assert()
        .code(3);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    home.cmd()
        .args(["auth", "login", "--host", &host, "--token-stdin"])
        .write_stdin("newest-secret")
        .assert()
        .success();
    home.cmd()
        .args(["auth", "token", "--reveal"])
        .assert()
        .success()
        .stdout("newest-secret\n");
}

#[tokio::test]
async fn failed_credential_removal_preserves_identity_and_profiles() {
    let (home, _server, _) = login_home().await;
    let output = home
        .cmd()
        .args(["identity", "list", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let key = value["result"]["identities"][0]["key"].as_str().unwrap();
    let config_before = fs::read(home.config_dir.join("config.toml")).unwrap();
    home.cmd()
        .env("CANVAS_TEST_KEYRING_ERROR", "backend")
        .args(["identity", "remove", key, "--yes"])
        .assert()
        .code(13);
    assert!(home.data_dir.join(key).join("identity.json").exists());
    assert_eq!(
        config_before,
        fs::read(home.config_dir.join("config.toml")).unwrap()
    );
}

#[tokio::test]
async fn unsafe_files_are_refused_without_modification_or_secret_diagnostics() {
    for kind in ["symlink", "mode", "malformed", "fifo"] {
        let (home, _server, host) = login_home().await;
        let path = home.config_dir.join("credentials.toml");
        match kind {
            "symlink" => {
                let target = home.config_dir.join("original");
                fs::rename(&path, &target).unwrap();
                symlink(target, &path).unwrap();
            }
            "mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            "malformed" => fs::write(&path, "token = SECRET_PARSE_SENTINEL\n").unwrap(),
            "fifo" => {
                fs::remove_file(&path).unwrap();
                assert!(
                    std::process::Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            _ => unreachable!(),
        }
        home.cmd()
            .timeout(std::time::Duration::from_secs(5))
            .args(["auth", "login", "--host", &host, "--token-stdin"])
            .write_stdin("replacement-secret")
            .assert()
            .code(13)
            .stderr(predicate::str::contains("credentials.toml"))
            .stderr(predicate::str::contains("SECRET_PARSE_SENTINEL").not())
            .stderr(predicate::str::contains("replacement-secret").not());
        if kind == "symlink" {
            assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        }
    }
}

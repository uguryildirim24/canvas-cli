//! Doctor command integration tests.

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

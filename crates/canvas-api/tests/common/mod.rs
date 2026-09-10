//! Shared helpers for canvas-api integration tests.

#![allow(dead_code)] // each integration crate pulls only the helpers it needs

use canvas_api::{Client, GovernorConfig, Secret};
use reqwest::Url;
use wiremock::MockServer;

/// Build a test client pointed at `server` with jitter disabled.
pub fn test_client(server: &MockServer) -> Client {
    let origin = Url::parse(&server.uri()).expect("mock server uri");
    Client::with_governor(
        origin,
        Secret::new("tok"),
        "canvas-cli/test",
        GovernorConfig {
            jitter: false,
            ..Default::default()
        },
    )
    .expect("client")
}

/// Build a test client with an explicit governor config.
pub fn test_client_with_governor(server: &MockServer, config: GovernorConfig) -> Client {
    let origin = Url::parse(&server.uri()).expect("mock server uri");
    Client::with_governor(origin, Secret::new("tok"), "canvas-cli/test", config).expect("client")
}

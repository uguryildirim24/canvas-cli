//! Shared helpers for canvas-cli integration tests.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command as StdCommand;

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Temp config + data directories with env forced to the file credential store.
pub struct TestHome {
    pub _config: TempDir,
    pub _data: TempDir,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl TestHome {
    pub fn new() -> Self {
        let config = TempDir::new().unwrap();
        let data = TempDir::new().unwrap();
        let config_dir = config.path().to_path_buf();
        let data_dir = data.path().to_path_buf();
        Self {
            _config: config,
            _data: data,
            config_dir,
            data_dir,
        }
    }

    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(cargo_bin!("canvas"));
        cmd.env("CANVAS_CONFIG_DIR", &self.config_dir)
            .env("CANVAS_DATA_DIR", &self.data_dir)
            .env("CANVAS_TEST_FORCE_FILE", "1")
            .env_remove("CANVAS_TOKEN")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE")
            .env_remove("CANVAS_TEST_CRASH_AFTER");
        cmd
    }

    pub fn std_cmd(&self) -> StdCommand {
        let mut cmd = StdCommand::new(cargo_bin!("canvas"));
        cmd.env("CANVAS_CONFIG_DIR", &self.config_dir)
            .env("CANVAS_DATA_DIR", &self.data_dir)
            .env("CANVAS_TEST_FORCE_FILE", "1")
            .env_remove("CANVAS_TOKEN")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE")
            .env_remove("CANVAS_TEST_CRASH_AFTER");
        cmd
    }
}

/// Mock `GET /api/v1/users/self`.
pub async fn mock_users_self(server: &MockServer, id: i64, name: &str) {
    Mock::given(method("GET"))
        .and(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "time_zone": "America/New_York",
        })))
        .mount(server)
        .await;
}

/// Host string suitable for `--host` from a mock server URI.
pub fn host_from_server(server: &MockServer) -> String {
    // Keep the http scheme so canonicalize_origin preserves it for wiremock.
    server.uri().trim_end_matches('/').to_owned()
}

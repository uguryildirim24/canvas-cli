//! M3-b `download` CLI tests.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixture {
    data: tempfile::TempDir,
    dest: tempfile::TempDir,
    key: String,
}

impl Fixture {
    fn prepare(origin: &str) -> Self {
        let data = tempfile::TempDir::new().unwrap();
        let dest = tempfile::TempDir::new().unwrap();
        let doc = IdentityDocument::new(origin, 12345, "2026-01-01T00:00:00Z");
        let key = doc.key.to_string();
        let paths = Paths::for_identity(data.path(), &doc.key);
        fs::create_dir_all(&paths.identity_dir).unwrap();
        fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        seed_course_and_files(&open);
        drop(open);
        Self { data, dest, key }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("canvas").unwrap();
        cmd.env("CANVAS_DATA_ROOT", self.data.path())
            .env("CANVAS_IDENTITY_KEY", &self.key)
            .env("CANVAS_TOKEN", "tok")
            .env("CANVAS_TEST_ALLOW_HTTP", "1")
            .env_remove("CANVAS_HOST")
            .env_remove("HOME");
        cmd
    }
}

fn seed_course_and_files(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO terms (id, name, start_at, end_at, data_json)
                 VALUES (7, 'Fall 2026', '2026-08-25T04:00:00Z', '2026-12-15T05:00:00Z', '{}')",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO courses (id, name, course_code, html_url, term_id, data_json)
                 VALUES (101, 'Intro', 'CS-101', 'https://example.test/courses/101', 7,
                         '{"enrollment_state":"active","is_favorite":true,"restricted":false}')"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('courses', 'active', 'course', '101', 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('courses', 'active', '2026-09-09T16:00:00Z', 1, 1, 0, 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO folders (id, course_id, name, full_name, parent_folder_id, data_json)
                 VALUES (1, 101, 'course files', 'course files', NULL, '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('folders', 'course:101', 'folder', '1', 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('folders', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0)",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO files (id, course_id, folder_id, display_name, size, data_json)
                 VALUES (50, 101, 1, 'lec.pdf', 4,
                         '{"updated_at":"2026-09-01T12:00:00Z","hidden":"false","locked_for_user":"false"}')"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('files', 'course:101', 'file', '50', 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('files', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0)",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO modules (id, course_id, name, position, items_count, items_complete, data_json)
                 VALUES (8, 101, 'Week 1', 1, 1, 1, '{"state":"unlocked"}')"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('modules', 'course:101', 'module', '8', 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('module_items', 'course:101:module:8', 'module_item', '80', 0)",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO module_items
                    (id, module_id, course_id, title, position, content_id, type, data_json)
                 VALUES (80, 8, 101, 'slides', 1, 50, 'File', '{"locked_for_user":"false"}')"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('modules', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

async fn mock_users_self(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 12345,
            "name": "Test",
            "time_zone": "UTC",
        })))
        .mount(server)
        .await;
}

async fn mock_file_and_storage(server: &MockServer, file_id: i64, body: &[u8]) {
    let url = format!("{}/storage/{file_id}", server.uri());
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/files/{file_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": file_id,
            "size": body.len(),
            "url": url,
            "updated_at": "2026-09-01T12:00:00Z",
            "locked_for_user": false,
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/storage/{file_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

#[test]
fn download_offline_is_exit_2() {
    let empty = tempfile::TempDir::new().unwrap();
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .env("CANVAS_DATA_ROOT", empty.path())
        .args([
            "download",
            "101",
            "--dest",
            "/tmp/x",
            "--offline",
            "--json",
            "--color",
            "never",
        ])
        .assert()
        .code(2);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("offline"), "stdout={stdout}");
}

#[test]
fn download_requires_dest() {
    let server_uri = "https://example.test";
    let fx = Fixture::prepare(server_uri);
    fx.cmd()
        .args(["download", "101", "--json", "--color", "never"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("download requires --dest"));
}

#[tokio::test]
async fn dry_run_writes_nothing_and_plans() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().to_path_buf();

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--dry-run",
            "--json",
            "--color",
            "never",
            "--quiet",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/download@1");
    assert_eq!(value["result"]["dry_run"], true);
    assert!(
        value["result"]["courses"][0]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["action"] == "planned" || f["action"] == "skipped_external")
    );
    assert!(!dest.join(".canvas-cli").exists());
}

#[tokio::test]
async fn download_then_rerun_skips_zero_bytes() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().display().to_string();

    let assert = fx
        .cmd()
        .args([
            "download", "101", "--dest", &dest, "--json", "--color", "never", "--quiet",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["downloaded"], 1);
    assert_eq!(v["result"]["totals"]["bytes"], 4);

    let assert = fx
        .cmd()
        .args([
            "download", "101", "--dest", &dest, "--json", "--color", "never", "--quiet",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["downloaded"], 0);
    assert_eq!(v["result"]["totals"]["skipped"], 1);
    assert_eq!(v["result"]["totals"]["bytes"], 0);
}

#[tokio::test]
async fn identity_mismatch_exits_8_before_write() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path();
    fs::create_dir_all(dest.join(".canvas-cli")).unwrap();
    fs::write(
        dest.join(".canvas-cli/dest.json"),
        r#"{"dest_id":"11111111-1111-4111-8111-111111111111","identity_key":"other-key","format_version":1}"#,
    )
    .unwrap();

    fx.cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--json",
            "--color",
            "never",
            "--quiet",
        ])
        .assert()
        .code(8);

    // Identity check runs after creating install.lock; refuse further writes.
    assert!(dest.join(".canvas-cli/dest.json").exists());
    assert!(!dest.join("CS-101-101").exists());
}

#[tokio::test]
async fn damaged_dest_json_exits_13() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path();
    fs::create_dir_all(dest.join(".canvas-cli")).unwrap();
    fs::write(dest.join(".canvas-cli/dest.json"), b"{not-json").unwrap();

    fx.cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--json",
            "--color",
            "never",
            "--quiet",
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::contains("damaged"));
}

#[tokio::test]
async fn module_and_file_filters_keep_ownership_path() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().display().to_string();

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            &dest,
            "--module",
            "Week",
            "--dry-run",
            "--json",
            "--color",
            "never",
            "--quiet",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let path = v["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    assert!(path.contains("/modules/"), "module ownership path: {path}");

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            &dest,
            "--file",
            "50",
            "--dry-run",
            "--json",
            "--color",
            "never",
            "--quiet",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let path2 = v["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    assert_eq!(path, path2, "filters must not change planned path");
}

#[tokio::test]
async fn download_json_snapshot() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().display().to_string();

    let assert = fx
        .cmd()
        .args([
            "download", "101", "--dest", &dest, "--json", "--color", "never", "--quiet",
        ])
        .assert()
        .success();
    let mut value: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    // Stabilize volatile fields.
    value["generated_at"] = json!("2026-09-09T17:05:12Z");
    value["identity"]["key"] = json!("KEY");
    value["identity"]["origin"] = json!("http://example.test");
    value["result"]["dest"] = json!("/tmp/out");
    value["requests"] = json!({"api": 0, "storage": 0, "cost": null});
    value["freshness"] = json!([]);
    insta::assert_json_snapshot!("download_json_ok", value);
}

#[tokio::test]
async fn download_human_snapshot() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().display().to_string();

    let assert = fx
        .cmd()
        .env("COLUMNS", "100")
        .args([
            "download", "101", "--dest", &dest, "--color", "never", "--quiet",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    insta::assert_snapshot!("download_human_ok", stdout.as_ref());
}

#[tokio::test]
async fn force_replaces_unmanaged() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path();

    // First plan path via dry-run.
    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--dry-run",
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let rel = v["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let full = dest.join(&rel);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(&full, b"xxxx").unwrap();

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["unmanaged"], 1);

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--force",
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["downloaded"], 1);
    assert_eq!(fs::read(&full).unwrap(), b"data");
}

#[tokio::test]
async fn verify_mismatch_exit_10() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path().display().to_string();

    fx.cmd()
        .args([
            "download", "101", "--dest", &dest, "--json", "--quiet", "--color", "never",
        ])
        .assert()
        .success();

    // Corrupt the installed file.
    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            &dest,
            "--dry-run",
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let rel = v["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    fs::write(PathBuf::from(&dest).join(rel), b"XXXX").unwrap();

    fx.cmd()
        .args([
            "download", "101", "--dest", &dest, "--verify", "--json", "--quiet", "--color", "never",
        ])
        .assert()
        .code(10);
}

#[tokio::test]
async fn storage_expired_refresh_once_then_failed() {
    let canvas = MockServer::start().await;
    let storage = MockServer::start().await;
    mock_users_self(&canvas).await;

    Mock::given(method("GET"))
        .and(path("/api/v1/files/50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 50,
            "size": 4,
            "url": format!("{}/obj", storage.uri()),
            "updated_at": "2026-09-01T12:00:00Z",
            "locked_for_user": false,
        })))
        .mount(&canvas)
        .await;
    Mock::given(method("GET"))
        .and(path("/obj"))
        .respond_with(ResponseTemplate::new(403))
        .expect(2)
        .mount(&storage)
        .await;

    let fx = Fixture::prepare(&canvas.uri());
    let dest = fx.dest.path().display().to_string();
    let assert = fx
        .cmd()
        .args([
            "download", "101", "--dest", &dest, "--json", "--quiet", "--color", "never",
        ])
        .assert()
        .code(12);
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["failed"], 1);
}

#[tokio::test]
async fn unsafe_path_dotdot_is_partial() {
    // Containment is covered in canvas-core; here we assert CLI maps UnsafePath.
    // A symlink final path under dest triggers unsafe_path via install.
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let dest = fx.dest.path();

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--dry-run",
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let rel = v["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let parent = dest.join(PathBuf::from(&rel).parent().unwrap());
    fs::create_dir_all(&parent).unwrap();
    let outside = fx.data.path().join("outside.txt");
    fs::write(&outside, b"x").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, dest.join(&rel)).unwrap();
    }
    #[cfg(not(unix))]
    {
        return;
    }

    let assert = fx
        .cmd()
        .args([
            "download",
            "101",
            "--dest",
            dest.to_str().unwrap(),
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .assert()
        .code(12);
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["result"]["totals"]["unsafe_path"], 1);
}

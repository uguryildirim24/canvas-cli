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
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("TZ", "UTC")
            .env("COLUMNS", "100")
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
                 VALUES ('courses', 'active', '2026-09-09T17:00:00Z', 1, 1, 0, 0)",
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
                 VALUES ('folders', 'course:101', '2026-09-09T17:00:00Z', 1, 1, 0, 0)",
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
                 VALUES ('files', 'course:101', '2026-09-09T17:00:00Z', 1, 1, 0, 0)",
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
                 VALUES ('modules', 'course:101', '2026-09-09T17:00:00Z', 1, 1, 0, 0)",
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
    assert!(!fx.data.path().join(&fx.key).join("downloads").exists());
    let state =
        rusqlite::Connection::open(fx.data.path().join(&fx.key).join("state.sqlite")).unwrap();
    assert_eq!(
        state
            .query_row("SELECT count(*) FROM destinations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
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
async fn symlinked_final_path_is_partial() {
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

fn run_json(fx: &Fixture, flags: &[&str], exit: i32) -> serde_json::Value {
    let assert = fx
        .cmd()
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("TZ", "UTC")
        .args([
            "download",
            "101",
            "--dest",
            fx.dest.path().to_str().unwrap(),
            "--json",
            "--quiet",
            "--color",
            "never",
        ])
        .args(flags)
        .assert()
        .code(exit);
    assert!(
        assert.get_output().stderr.is_empty(),
        "JSON must suppress progress: {:?}",
        assert.get_output().stderr
    );
    serde_json::from_slice(&assert.get_output().stdout).unwrap()
}

fn cache_edit(fx: &Fixture, sql: &str) {
    rusqlite::Connection::open(fx.data.path().join(&fx.key).join("cache.sqlite"))
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

fn manifest_edit(fx: &Fixture, sql: &str) {
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(fx.dest.path().join(".canvas-cli/dest.json")).unwrap())
            .unwrap();
    let path = fx
        .data
        .path()
        .join(&fx.key)
        .join("downloads")
        .join(format!("{}.sqlite", meta["dest_id"].as_str().unwrap()));
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

#[tokio::test]
async fn empty_files_install_and_missing_sizes_do_not_use_cached_hints() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"").await;
    let fx = Fixture::prepare(&server.uri());
    let value = run_json(&fx, &[], 0);
    let file = &value["result"]["courses"][0]["files"][0];
    assert_eq!(file["size"], 0);
    assert_eq!(
        fs::read(fx.dest.path().join(file["path"].as_str().unwrap())).unwrap(),
        b""
    );
    assert_eq!(run_json(&fx, &[], 0)["result"]["totals"]["skipped"], 1);
    server.reset().await;
    mock_users_self(&server).await;
    Mock::given(path("/api/v1/files/50"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": 50, "url": format!("{}/storage/50", server.uri())})),
        )
        .mount(&server)
        .await;
    let result = run_json(&fx, &[], 12);
    assert_eq!(result["result"]["totals"]["failed"], 1);
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/storage/50")
            .count(),
        0
    );
}

#[tokio::test]
async fn refreshed_metadata_rechecks_lock_revision_and_token_boundary() {
    for case in ["locked", "revision", "success"] {
        let canvas = MockServer::start().await;
        let storage = MockServer::start().await;
        mock_users_self(&canvas).await;
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = count.clone();
        let uri = storage.uri();
        Mock::given(path("/api/v1/files/50")).respond_with(move |_: &wiremock::Request| {
            let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"id": 50, "size": 4, "url": format!("{uri}/{}?signature=private", if n == 0 {"expired"} else {"fresh"}), "locked_for_user": n > 0 && case == "locked", "updated_at": if n > 0 && case == "revision" {"2026-09-02T12:00:00Z"} else {"2026-09-01T12:00:00Z"}}))
        }).mount(&canvas).await;
        Mock::given(path("/expired"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&storage)
            .await;
        Mock::given(path("/fresh"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data".to_vec()))
            .expect(u64::from(case == "success"))
            .mount(&storage)
            .await;
        let fx = Fixture::prepare(&canvas.uri());
        let value = run_json(&fx, &[], if case == "success" { 0 } else { 12 });
        let expected = match case {
            "locked" => "locked",
            "revision" => "failed",
            _ => "downloaded",
        };
        assert_eq!(value["result"]["totals"][expected], 1);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!value.to_string().contains("signature"));
        for request in storage.received_requests().await.unwrap() {
            assert!(!request.headers.contains_key("authorization"));
            assert_eq!(request.headers["accept-encoding"], "identity");
        }
    }
}

#[tokio::test]
async fn abort_keeps_installed_results_and_stops_new_requests() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    Mock::given(path("/api/v1/files/51"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let fx = Fixture::prepare(&server.uri());
    cache_edit(
        &fx,
        "INSERT INTO files (id,course_id,folder_id,display_name,size,data_json) VALUES (51,101,1,'z.pdf',4,'{}'),(52,101,1,'zz.pdf',4,'{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('files','course:101','file','51',1),('files','course:101','file','52',2);",
    );
    // Put all three in modules so path sorting starts with the successful file.
    cache_edit(
        &fx,
        "INSERT INTO module_items (id,module_id,course_id,title,position,content_id,type,data_json) VALUES (81,8,101,'z.pdf',2,51,'File','{}'),(82,8,101,'zz.pdf',3,52,'File','{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('module_items','course:101:module:8','module_item','81',1),('module_items','course:101:module:8','module_item','82',2);",
    );
    let value = run_json(&fx, &["--jobs", "1"], 3);
    assert_eq!(value["schema"], "canvas-cli/error@1");
    assert_eq!(value["result"]["http_status"], 401);
    assert_eq!(
        value["result"]["details"]["download"]["totals"]["downloaded"],
        1
    );
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.url.path() == "/api/v1/files/52")
    );
}

#[tokio::test]
async fn modified_force_mismatch_precedence_and_move_previous_path() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let first = run_json(&fx, &[], 0);
    let old = first["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    fs::write(fx.dest.path().join(old), b"changed").unwrap();
    let modified = run_json(&fx, &[], 0);
    assert_eq!(modified["result"]["totals"]["modified"], 1);
    assert!(!modified["warnings"].as_array().unwrap().is_empty());
    cache_edit(
        &fx,
        "INSERT INTO files (id,course_id,folder_id,display_name,size,data_json) VALUES (51,101,1,'locked.pdf',4,'{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('files','course:101','file','51',1);",
    );
    Mock::given(path("/api/v1/files/51"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":51,"locked_for_user":true})),
        )
        .mount(&server)
        .await;
    let mismatch = run_json(&fx, &["--verify"], 10);
    assert_eq!(mismatch["outcome"], "mismatch");
    assert_eq!(mismatch["result"]["totals"]["locked"], 1);
    let replaced = run_json(&fx, &["--verify", "--force", "--file", "50"], 0);
    assert_eq!(replaced["result"]["totals"]["downloaded"], 1);
    assert_eq!(fs::read(fx.dest.path().join(old)).unwrap(), b"data");
    cache_edit(&fx, "UPDATE modules SET name='Renamed' WHERE id=8;");
    let moved = run_json(&fx, &["--file", "50"], 0);
    let row = &moved["result"]["courses"][0]["files"][0];
    assert_eq!(row["action"], "moved");
    assert_eq!(row["previous_path"], old);
    assert!(!fx.dest.path().join(old).exists());
    assert_eq!(moved["result"]["totals"]["bytes"], 0);
}

#[tokio::test]
async fn recovery_reports_real_paths_even_when_file_filter_excludes_them() {
    for state in ["new", "old", "both", "neither"] {
        let server = MockServer::start().await;
        mock_users_self(&server).await;
        mock_file_and_storage(&server, 50, b"data").await;
        let fx = Fixture::prepare(&server.uri());
        let initial = run_json(&fx, &[], 0);
        let old = initial["result"]["courses"][0]["files"][0]["path"]
            .as_str()
            .unwrap();
        manifest_edit(
            &fx,
            "UPDATE files SET pending_move_to='recovered.pdf',move_sha256=sha256 WHERE file_id=50;",
        );
        if matches!(state, "new" | "both") {
            fs::write(fx.dest.path().join("recovered.pdf"), b"data").unwrap();
        }
        if matches!(state, "new" | "neither") {
            fs::remove_file(fx.dest.path().join(old)).unwrap();
        }
        let value = run_json(
            &fx,
            &["--file", "999"],
            if state == "neither" { 12 } else { 0 },
        );
        let files = value["result"]["courses"][0]["files"].as_array().unwrap();
        if state == "old" {
            assert!(files.is_empty());
        }
        if matches!(state, "new" | "both") {
            let moved = files.iter().find(|f| f["action"] == "moved").unwrap();
            assert_eq!(moved["path"], "recovered.pdf");
            assert_eq!(moved["previous_path"], old);
        }
        if state == "both" {
            assert_eq!(
                files.iter().find(|f| f["action"] == "unmanaged").unwrap()["path"],
                old
            );
            assert!(!value["warnings"].as_array().unwrap().is_empty());
        }
        if state == "neither" {
            assert_eq!(files[0]["action"], "unresolved_move");
        }
    }
}

#[tokio::test]
async fn orphan_manifest_refuses_registration() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    let id = "11111111-1111-4111-8111-111111111111";
    fs::create_dir_all(fx.dest.path().join(".canvas-cli")).unwrap();
    fs::write(
        fx.dest.path().join(".canvas-cli/dest.json"),
        serde_json::to_vec(&json!({"dest_id":id,"identity_key":fx.key,"format_version":1}))
            .unwrap(),
    )
    .unwrap();
    let downloads = fx.data.path().join(&fx.key).join("downloads");
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join(format!("{id}.sqlite")), b"orphan").unwrap();
    assert!(
        run_json(&fx, &[], 13)["result"]["message"]
            .as_str()
            .unwrap()
            .contains("unregistered")
    );
    let db = rusqlite::Connection::open(fx.data.path().join(&fx.key).join("state.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM destinations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn listing_denials_are_partial_but_dry_run_stays_zero() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    cache_edit(
        &fx,
        "UPDATE fetch_log SET fetched_at='2026-09-01T00:00:00Z' WHERE dataset IN ('files','folders');",
    );
    Mock::given(path("/api/v1/courses/101/files"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/101/folders"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let value = run_json(&fx, &[], 12);
    assert_eq!(value["partial"].as_array().unwrap().len(), 2);
    assert_eq!(value["result"]["totals"]["downloaded"], 1);
    assert_eq!(
        run_json(&fx, &["--dry-run"], 0)["partial"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn filters_keep_collision_suffixes_and_nonowner_module_links() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    let fx = Fixture::prepare(&server.uri());
    cache_edit(
        &fx,
        "INSERT INTO files (id,course_id,folder_id,display_name,size,data_json) VALUES (51,101,1,'LEC.pdf',8,'{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('files','course:101','file','51',1); INSERT INTO modules (id,course_id,name,position,items_count,items_complete,data_json) VALUES (9,101,'Week 2',2,2,1,'{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('modules','course:101','module','9',1); INSERT INTO module_items (id,module_id,course_id,title,position,content_id,type,data_json) VALUES (81,8,101,'collision',2,51,'File','{}'),(90,9,101,'same file',1,50,'File','{}'),(91,9,101,'tool',2,NULL,'ExternalTool','{}'); INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES ('module_items','course:101:module:8','module_item','81',1),('module_items','course:101:module:9','module_item','90',0),('module_items','course:101:module:9','module_item','91',1);",
    );
    let full = run_json(&fx, &["--dry-run"], 0);
    let files = full["result"]["courses"][0]["files"].as_array().unwrap();
    let owner = files.iter().find(|f| f["id"] == "50").unwrap();
    assert!(owner["path"].as_str().unwrap().ends_with("lec-50.pdf"));
    assert_eq!(full["result"]["totals"]["bytes"], 12);
    let filtered = run_json(&fx, &["--dry-run", "--module", "Week 2"], 0);
    let filtered_files = filtered["result"]["courses"][0]["files"]
        .as_array()
        .unwrap();
    assert_eq!(
        filtered_files.iter().find(|f| f["id"] == "50").unwrap()["path"],
        owner["path"]
    );
    assert_eq!(filtered["result"]["totals"]["skipped_external"], 1);
    let by_file = run_json(&fx, &["--dry-run", "--file", "50"], 0);
    assert_eq!(
        by_file["result"]["courses"][0]["files"][0]["path"],
        owner["path"]
    );
    assert!(!fx.dest.path().join(".canvas-cli").exists());
    assert!(!fx.data.path().join(&fx.key).join("downloads").exists());
}

#[tokio::test]
async fn all_courses_downloads_each_selected_course() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    mock_file_and_storage(&server, 60, b"second").await;
    Mock::given(path("/api/v1/courses")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id":101,"name":"Intro","course_code":"CS-101","enrollments":[{"type":"student","enrollment_state":"active"}]},
        {"id":202,"name":"Second","course_code":"BIO-202","enrollments":[{"type":"student","enrollment_state":"active"}]}
    ]))).mount(&server).await;
    for (dataset, body) in [
        (
            "folders",
            json!([{"id":2,"name":"course files","full_name":"course files"}]),
        ),
        (
            "files",
            json!([{"id":60,"folder_id":2,"display_name":"notes.pdf","size":6}]),
        ),
        ("modules", json!([])),
    ] {
        Mock::given(path(format!("/api/v1/courses/202/{dataset}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    let fx = Fixture::prepare(&server.uri());
    let assert = fx
        .cmd()
        .args([
            "download",
            "--all-courses",
            "--dest",
            fx.dest.path().to_str().unwrap(),
            "--json",
            "--quiet",
        ])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(value["result"]["courses"].as_array().unwrap().len(), 2);
    assert_eq!(value["result"]["totals"]["downloaded"], 2);
    assert_eq!(value["result"]["totals"]["bytes"], 10);
}

#[cfg(unix)]
#[tokio::test]
async fn containment_parent_final_and_transfer_time_symlink_swaps() {
    use std::os::unix::fs::symlink;
    for case in ["parent", "final", "swap_final", "swap_parent"] {
        let server = MockServer::start().await;
        mock_users_self(&server).await;
        let fx = Fixture::prepare(&server.uri());
        let plan = run_json(&fx, &["--dry-run"], 0);
        let rel = plan["result"]["courses"][0]["files"][0]["path"]
            .as_str()
            .unwrap();
        let full = fx.dest.path().join(rel);
        let parent = full.parent().unwrap().to_path_buf();
        let outside = fx.data.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel");
        fs::write(&sentinel, b"keep").unwrap();
        fs::create_dir_all(&parent).unwrap();
        if case == "parent" {
            fs::remove_dir(&parent).unwrap();
            symlink(&outside, &parent).unwrap();
        }
        if case == "final" {
            symlink(&sentinel, &full).unwrap();
        }
        Mock::given(path("/api/v1/files/50"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"id":50,"size":4,"url":format!("{}/storage",server.uri())}),
                ),
            )
            .mount(&server)
            .await;
        let out = outside.clone();
        let target = full.clone();
        Mock::given(path("/storage"))
            .respond_with(move |_: &wiremock::Request| {
                if case == "swap_final" {
                    symlink(out.join("sentinel"), &target).unwrap();
                }
                if case == "swap_parent" {
                    fs::rename(&parent, parent.with_extension("retained")).unwrap();
                    symlink(&out, &parent).unwrap();
                }
                ResponseTemplate::new(200).set_body_bytes(b"data".to_vec())
            })
            .expect(u64::from(case.starts_with("swap")))
            .mount(&server)
            .await;
        let result = run_json(&fx, &["--force", "--verify"], 12);
        assert_eq!(result["result"]["totals"]["unsafe_path"], 1);
        assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn absolute_and_parent_traversal_in_recovery_are_refused() {
    for bad in ["../outside", "/tmp/outside"] {
        let server = MockServer::start().await;
        mock_users_self(&server).await;
        mock_file_and_storage(&server, 50, b"data").await;
        let fx = Fixture::prepare(&server.uri());
        run_json(&fx, &[], 0);
        manifest_edit(
            &fx,
            &format!(
                "UPDATE files SET path='{bad}', pending_move_to='new',move_sha256=sha256 WHERE file_id=50;"
            ),
        );
        let result = run_json(&fx, &[], 12);
        assert_eq!(result["result"]["totals"]["unsafe_path"], 1);
        assert_eq!(result["result"]["totals"]["downloaded"], 0);
    }
}

#[tokio::test]
async fn two_process_installers_transfer_outside_the_lock() {
    use std::process::Stdio;
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    Mock::given(path("/api/v1/files/50")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":50,"size":4,"url":format!("{}/storage",server.uri()),"updated_at":"2026-09-01T12:00:00Z"}))).mount(&server).await;
    let received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = received.clone();
    Mock::given(path("/storage"))
        .respond_with(move |_: &wiremock::Request| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_bytes(b"data".to_vec())
                .set_delay(std::time::Duration::from_secs(1))
        })
        .expect(2)
        .mount(&server)
        .await;
    let fx = Fixture::prepare(&server.uri());
    let spawn = || {
        std::process::Command::new(assert_cmd::cargo::cargo_bin("canvas"))
            .env("CANVAS_DATA_ROOT", fx.data.path())
            .env("CANVAS_IDENTITY_KEY", &fx.key)
            .env("CANVAS_TOKEN", "tok")
            .env("CANVAS_TEST_ALLOW_HTTP", "1")
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env_remove("CANVAS_HOST")
            .env_remove("HOME")
            .args([
                "download",
                "101",
                "--dest",
                fx.dest.path().to_str().unwrap(),
                "--json",
                "--quiet",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn();
    let second = spawn();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while received.load(std::sync::atomic::Ordering::SeqCst) != 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both transfers must start before either install completes");
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    let mut actions = Vec::new();
    for output in outputs {
        assert!(output.status.success(), "{output:?}");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        actions.push(
            value["result"]["courses"][0]["files"][0]["action"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    actions.sort();
    assert_eq!(actions, ["downloaded", "skipped"]);
    let result = run_json(&fx, &["--verify"], 0);
    assert_eq!(result["result"]["totals"]["skipped"], 1);
}

#[tokio::test]
async fn interrupted_transfer_is_cleaned_and_next_run_restarts_without_range() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    Mock::given(path("/api/v1/files/50"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":50,"size":4,"url":format!("{}/short",server.uri())})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/short"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"bad".to_vec()))
        .mount(&server)
        .await;
    let fx = Fixture::prepare(&server.uri());
    let result = run_json(&fx, &[], 12);
    let rel = result["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    let full = fx.dest.path().join(rel);
    assert!(!full.exists());
    assert_eq!(fs::read_dir(full.parent().unwrap()).unwrap().count(), 0);
    server.reset().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    assert_eq!(run_json(&fx, &[], 0)["result"]["totals"]["downloaded"], 1);
    assert_eq!(fs::read(&full).unwrap(), b"data");
    for request in server.received_requests().await.unwrap() {
        assert!(!request.headers.contains_key("range"));
    }
}

#[tokio::test]
async fn remote_changes_replace_owned_files_and_rename_preserves_old_revision() {
    for rename in [false, true] {
        let server = MockServer::start().await;
        mock_users_self(&server).await;
        mock_file_and_storage(&server, 50, b"data").await;
        let fx = Fixture::prepare(&server.uri());
        let initial = run_json(&fx, &[], 0);
        let old = initial["result"]["courses"][0]["files"][0]["path"]
            .as_str()
            .unwrap();
        if rename {
            cache_edit(&fx, "UPDATE modules SET name='Changed' WHERE id=8;");
        }
        server.reset().await;
        mock_users_self(&server).await;
        mock_file_and_storage(&server, 50, b"new revision").await;
        let value = run_json(&fx, &[], 0);
        let row = &value["result"]["courses"][0]["files"][0];
        assert_eq!(row["action"], "downloaded");
        assert_eq!(
            fs::read(fx.dest.path().join(row["path"].as_str().unwrap())).unwrap(),
            b"new revision"
        );
        if rename {
            assert_eq!(fs::read(fx.dest.path().join(old)).unwrap(), b"data");
        }
    }
}

#[tokio::test]
async fn occupied_move_target_is_unmanaged_until_forced() {
    let server = MockServer::start().await;
    mock_users_self(&server).await;
    mock_file_and_storage(&server, 50, b"data").await;
    let fx = Fixture::prepare(&server.uri());
    let initial = run_json(&fx, &[], 0);
    let old = initial["result"]["courses"][0]["files"][0]["path"]
        .as_str()
        .unwrap();
    cache_edit(&fx, "UPDATE modules SET name='Changed' WHERE id=8;");
    let plan = run_json(&fx, &["--dry-run"], 0);
    let target = fx.dest.path().join(
        plan["result"]["courses"][0]["files"][0]["path"]
            .as_str()
            .unwrap(),
    );
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"mine").unwrap();
    assert_eq!(run_json(&fx, &[], 0)["result"]["totals"]["unmanaged"], 1);
    assert_eq!(fs::read(&target).unwrap(), b"mine");
    assert_eq!(
        run_json(&fx, &["--force"], 0)["result"]["totals"]["moved"],
        1
    );
    assert_eq!(fs::read(&target).unwrap(), b"data");
    assert!(!fx.dest.path().join(old).exists());
}

#[tokio::test]
async fn future_unlock_is_honored_and_http_test_gate_is_loopback_only() {
    for case in ["future", "http"] {
        let server = MockServer::start().await;
        mock_users_self(&server).await;
        Mock::given(path("/api/v1/files/50")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":50,"size":4,"url":"http://example.test/private?signature=secret", "locked_for_user":false,"unlock_at":if case=="future" {Some("2099-01-01T00:00:00Z")} else {None}}))).mount(&server).await;
        let fx = Fixture::prepare(&server.uri());
        let result = run_json(&fx, &[], if case == "future" { 12 } else { 4 });
        if case == "future" {
            assert_eq!(result["result"]["totals"]["locked"], 1);
        }
        assert!(!result.to_string().contains("signature"));
        assert!(
            !fx.dest
                .path()
                .join("CS-101-101/modules/01-Week 1/lec.pdf")
                .exists()
        );
    }
}

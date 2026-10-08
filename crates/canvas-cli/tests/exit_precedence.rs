//! Exit-code precedence for M1-b commands (SPEC §14).

use std::fs;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;

#[test]
fn courses_fresh_and_offline_is_usage_exit_2() {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .args(["courses", "--fresh", "--offline"])
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("--offline"),
        "stderr={stderr:?}"
    );
}

#[test]
fn courses_offline_json_without_identity_is_auth_exit_3() {
    let empty = tempfile::TempDir::new().unwrap();
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .env("CANVAS_DATA_ROOT", empty.path())
        .env_remove("CANVAS_IDENTITY_KEY")
        .env_remove("CANVAS_TOKEN")
        .args(["courses", "--offline", "--json", "--color", "never"])
        .assert()
        .code(3);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("canvas-cli/error@1"), "stdout={stdout}");
    assert!(stdout.contains("\"code\":\"auth\"") || stdout.contains("\"code\": \"auth\""));
}

#[test]
fn courses_offline_with_identity_but_no_coverage_is_exit_7() {
    let (dir, key) = prepare_identity_only();
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env_remove("CANVAS_TOKEN")
        .args(["courses", "--offline", "--json", "--color", "never"])
        .assert()
        .code(7);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("canvas-cli/error@1"), "stdout={stdout}");
    assert!(stdout.contains("\"code\":\"offline\"") || stdout.contains("\"code\": \"offline\""));
}

#[test]
fn sync_offline_is_usage_exit_2() {
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .args(["sync", "--offline", "--json"])
        .assert()
        .code(2);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stdout.contains("canvas-cli/error@1") || stderr.contains("offline"),
        "stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn courses_json_snapshot_from_fixture_cache() {
    let (dir, key) = prepare_seeded_courses();
    let output = StdCommand::new(env!("CARGO_BIN_EXE_canvas"))
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["courses", "--offline", "--json", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/courses@1");
    assert_eq!(value["outcome"], "ok");
    assert_eq!(value["exit"], 0);
    assert_eq!(value["generated_at"], "2026-09-09T17:05:12Z");
    assert_eq!(value["result"]["courses"][0]["code"], "SYN-61001");
    assert_eq!(value["freshness"][0]["dataset"], "courses");
    assert_eq!(value["freshness"][0]["source"], "cache");
    insta::assert_json_snapshot!(value);
}

fn prepare_identity_only() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example.test", 62001, "2026-01-01T00:00:00Z");
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    drop(open);
    (dir, key)
}

fn prepare_seeded_courses() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example.test", 62001, "2026-01-01T00:00:00Z");
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO terms (id, name, start_at, end_at, data_json)
                 VALUES (7, 'Fall 2026', '2026-08-25T04:00:00Z', '2026-12-15T05:00:00Z', '{}')",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO courses (id, name, course_code, html_url, term_id, data_json)
                 VALUES (
                    61001,
                    'Synthetic Computing',
                    'SYN-61001',
                    'https://canvas.example.test/courses/61001',
                    7,
                    '{"enrollment_state":"active","is_favorite":true,"restricted":false}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO course_totals (course_id, mode, current_score, current_grade, data_json)
                 VALUES (61001, 'all', 92.5, 'A-', '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('courses', 'active', 'course', '61001', 0)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('courses', 'active', '2026-09-09T16:00:00Z', 1, 1, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    drop(open);
    (dir, key)
}

//! M3-a `files` / `modules` CLI tests.

use std::fs;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;

fn bin() -> StdCommand {
    StdCommand::new(env!("CARGO_BIN_EXE_canvas"))
}

fn seed_course(open: &OpenIdentity) {
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
                    101,
                    'Intro to Computing',
                    'CS-101',
                    'https://lasell.instructure.com/courses/101',
                    7,
                    '{"enrollment_state":"active","is_favorite":true,"restricted":false}'
                 )"#,
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
            Ok(())
        })
        .unwrap();
}

fn seed_files_modules_ok(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO folders (id, course_id, name, full_name, parent_folder_id, data_json)
                 VALUES (1, 101, 'course files', 'course files', NULL, '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO folders (id, course_id, name, full_name, parent_folder_id, data_json)
                 VALUES (2, 101, 'Slides', 'course files/Slides', 1, '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('folders', 'course:101', 'folder', '1', 0),
                        ('folders', 'course:101', 'folder', '2', 1)",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO files (id, course_id, folder_id, display_name, size, data_json)
                 VALUES (
                    50, 101, 2, 'lec.pdf', 10,
                    '{"updated_at":"2026-09-01T12:00:00Z","hidden":"false","locked_for_user":"false"}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('files', 'course:101', 'file', '50', 0)",
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
                r#"INSERT INTO module_items
                    (id, module_id, course_id, title, position, content_id, type, data_json)
                 VALUES (80, 8, 101, 'slides', 1, 50, 'File', '{"locked_for_user":"false"}')"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen, error)
                 VALUES
                   ('folders', 'course:101', '2026-09-09T16:00:00Z', 1, 2, 0, 0, NULL),
                   ('files', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0, NULL),
                   ('modules', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0, NULL)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn seed_files_denial_with_module_file(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen, error)
                 VALUES
                   ('folders', 'course:101', '2026-09-09T16:00:00Z', 1, 0, 0, 0, 'unavailable:403'),
                   ('files', 'course:101', '2026-09-09T16:00:00Z', 1, 0, 0, 0, 'unavailable:403'),
                   ('modules', 'course:101', '2026-09-09T16:00:00Z', 1, 1, 0, 0, NULL)",
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
                r#"INSERT INTO module_items
                    (id, module_id, course_id, title, position, content_id, type, data_json)
                 VALUES (80, 8, 101, 'notes.pdf', 1, 99, 'File', '{"locked_for_user":"false"}')"#,
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn prepare_seeded_files() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new(
        "https://lasell.instructure.com",
        12345,
        "2026-01-01T00:00:00Z",
    );
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    seed_course(&open);
    seed_files_modules_ok(&open);
    drop(open);
    (dir, key)
}

fn prepare_seeded_denial() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new(
        "https://lasell.instructure.com",
        12345,
        "2026-01-01T00:00:00Z",
    );
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    seed_course(&open);
    seed_files_denial_with_module_file(&open);
    drop(open);
    (dir, key)
}

#[test]
fn files_auth_without_identity_is_exit_3() {
    let empty = tempfile::TempDir::new().unwrap();
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .env("CANVAS_DATA_ROOT", empty.path())
        .env_remove("CANVAS_IDENTITY_KEY")
        .env_remove("CANVAS_TOKEN")
        .args(["files", "101", "--offline", "--json", "--color", "never"])
        .assert()
        .code(3);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("canvas-cli/error@1"), "stdout={stdout}");
    assert!(stdout.contains("\"code\":\"auth\"") || stdout.contains("\"code\": \"auth\""));
}

#[test]
fn modules_auth_without_identity_is_exit_3() {
    let empty = tempfile::TempDir::new().unwrap();
    let assert = Command::cargo_bin("canvas")
        .unwrap()
        .env("CANVAS_DATA_ROOT", empty.path())
        .env_remove("CANVAS_IDENTITY_KEY")
        .env_remove("CANVAS_TOKEN")
        .args(["modules", "101", "--offline", "--json", "--color", "never"])
        .assert()
        .code(3);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("canvas-cli/error@1"), "stdout={stdout}");
}

#[test]
fn files_offline_denial_json_exit_0_with_partial() {
    let (dir, key) = prepare_seeded_denial();
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["files", "101", "--offline", "--json", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/files@1");
    assert_eq!(value["exit"], 0);
    assert_eq!(value["result"]["listing"]["available"], false);
    assert_eq!(value["result"]["listing"]["http_status"], 403);
    assert_eq!(value["result"]["files"][0]["source"], "module");
    assert_eq!(value["result"]["files"][0]["name"], "notes.pdf");
    let partial = &value["partial"][0];
    assert_eq!(partial["http_status"], 403);
    assert!(
        partial["message"]
            .as_str()
            .unwrap()
            .contains("Files listing unavailable (HTTP 403)"),
        "partial={partial}"
    );
}

#[test]
fn files_json_snapshot_from_fixture_cache() {
    let (dir, key) = prepare_seeded_files();
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["files", "101", "--offline", "--json", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/files@1");
    assert_eq!(value["result"]["files"][0]["name"], "lec.pdf");
    assert_eq!(value["result"]["listing"]["available"], true);
    insta::assert_json_snapshot!(value);
}

#[test]
fn files_human_snapshot_from_fixture_cache() {
    let (dir, key) = prepare_seeded_files();
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["files", "101", "--offline", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!(stdout);
}

#[test]
fn files_tree_and_search_smoke() {
    let (dir, key) = prepare_seeded_files();
    let tree = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["files", "101", "--offline", "--tree", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(tree.status.code(), Some(0));
    let tree_out = String::from_utf8_lossy(&tree.stdout);
    assert!(tree_out.contains("Slides/"), "tree={tree_out}");
    assert!(tree_out.contains("lec.pdf"), "tree={tree_out}");

    let search = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env_remove("CANVAS_TOKEN")
        .args([
            "files",
            "101",
            "--offline",
            "--search",
            "LEC",
            "--json",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    assert_eq!(search.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(value["result"]["files"].as_array().unwrap().len(), 1);

    let miss = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env_remove("CANVAS_TOKEN")
        .args([
            "files",
            "101",
            "--offline",
            "--search",
            "zzz",
            "--json",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    assert_eq!(miss.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&miss.stdout).unwrap();
    assert!(value["result"]["files"].as_array().unwrap().is_empty());
}

#[test]
fn modules_json_snapshot_from_fixture_cache() {
    let (dir, key) = prepare_seeded_files();
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args([
            "modules",
            "101",
            "--items",
            "--offline",
            "--json",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "canvas-cli/modules@1");
    assert_eq!(value["result"]["modules"][0]["name"], "Week 1");
    assert_eq!(value["result"]["modules"][0]["items"][0]["title"], "slides");
    insta::assert_json_snapshot!(value);
}

#[test]
fn modules_human_snapshot_without_items() {
    let (dir, key) = prepare_seeded_files();
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(["modules", "101", "--offline", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value_check = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", &key)
        .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
        .env_remove("CANVAS_TOKEN")
        .args(["modules", "101", "--offline", "--json", "--color", "never"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&value_check.stdout).unwrap();
    assert!(
        value["result"]["modules"][0]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!(stdout);
}

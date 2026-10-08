//! `xtask record` against `wiremock`, and the `sanitize` pass over its output.
//!
//! Recording from a real account is not approved (SPEC §19 item 5), so this is
//! the only place `record` runs. Two properties matter more than the shape of
//! the files: the token reaches no byte on disk, and the four headers the
//! client reads survive.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "record-secret-token-8f2a";

/// Every file under `dir`, keyed by name.
fn read_dir(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("read output directory") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 name")
            .to_owned();
        out.insert(name, std::fs::read_to_string(&path).expect("read file"));
    }
    out
}

fn xtask(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .env("CANVAS_TOKEN", TOKEN)
        .output()
        .expect("run xtask")
}

/// Mount the endpoints a one-course recording walks.
async fn mount(server: &MockServer) {
    // Everything the recorder sends must carry the bearer token; a request
    // without it would mean the walk is not authenticated the normal way.
    let authorized = || header("authorization", format!("Bearer {TOKEN}").as_str());

    server
        .register(
            Mock::given(method("GET"))
                .and(path("/api/v1/users/self"))
                .and(authorized())
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({
                            "id": 4242, "name": "Ada Lovelace",
                            "login_id": "alovelace", "primary_email": "ada@campus.example.test",
                            "avatar_url": "https://canvas.real.edu/images/9/a?token=capability"
                        }))
                        .append_header("Date", "Tue, 09 Sep 2026 17:05:12 GMT")
                        .append_header("X-Rate-Limit-Remaining", "597.4")
                        .append_header("X-Request-Cost", "0.31")
                        // Neither of these may reach a fixture.
                        .append_header("Set-Cookie", format!("session={TOKEN}"))
                        .append_header("X-Amz-Signature", "deadbeefcafe"),
                ),
        )
        .await;

    // Page two of the courses walk. The `Link` URL carries only `page`, so
    // it needs its own mock.
    server
        .register(
            Mock::given(method("GET"))
                .and(path("/api/v1/courses"))
                .and(query_param("page", "2"))
                .and(authorized())
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([]))),
        )
        .await;

    // Two pages, so the `Link` walk is exercised.
    let next = format!("<{}/api/v1/courses?page=2>; rel=\"next\"", server.uri());
    server
        .register(
            Mock::given(method("GET"))
                .and(path("/api/v1/courses"))
                .and(query_param("enrollment_state", "active"))
                .and(authorized())
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!([{
                            "id": 77, "course_code": "CHEM-101", "name": "Chemistry",
                            "syllabus_body": "<p>Read <b>chapter 1</b>.</p>"
                        }]))
                        .append_header("Link", next.as_str())
                        .append_header("X-Rate-Limit-Remaining", "596.0"),
                ),
        )
        .await;

    for (endpoint, body) in [
        (
            "/api/v1/courses/77/files",
            json!([{
                "id": 501, "display_name": "Syllabus.pdf", "filename": "Syllabus.pdf",
                "size": 2048, "folder_id": 20,
                "url": "https://files.real.edu/files/501/download?verifier=capability"
            }]),
        ),
        ("/api/v1/courses/77/folders", json!([])),
        ("/api/v1/courses/77/modules", json!([])),
        ("/api/v1/courses/77/assignments", json!([])),
        ("/api/v1/courses/77/assignment_groups", json!([])),
        (
            "/api/v1/courses/77/grading_periods",
            json!({"grading_periods": []}),
        ),
        ("/api/v1/courses/77", json!({"id": 77, "name": "Chemistry"})),
    ] {
        server
            .register(
                Mock::given(method("GET"))
                    .and(path(endpoint))
                    .and(authorized())
                    .respond_with(ResponseTemplate::new(200).set_body_json(body)),
            )
            .await;
    }

    mount_context(server, &authorized).await;
}

/// The two Appendix B endpoints addressed by a list of course context codes.
async fn mount_context<M: wiremock::Match + 'static>(
    server: &MockServer,
    authorized: &impl Fn() -> M,
) {
    server
        .register(
            Mock::given(method("GET"))
                .and(path("/api/v1/announcements"))
                .and(query_param("context_codes[]", "course_77"))
                .and(authorized())
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                    "id": 88, "title": "Welcome", "context_code": "course_77",
                    "message": "<p>Office hours moved.</p>",
                    "author": {"display_name": "Ada Lovelace"}
                }]))),
        )
        .await;
    for endpoint in [
        "/api/v1/calendar_events",
        "/api/v1/planner/items",
        "/api/v1/users/self/missing_submissions",
        "/api/v1/users/self/enrollments",
    ] {
        server
            .register(
                Mock::given(method("GET"))
                    .and(path(endpoint))
                    .and(authorized())
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!([]))),
            )
            .await;
    }
}

#[tokio::test]
async fn record_keeps_the_read_headers_and_writes_the_token_nowhere() {
    let server = MockServer::start().await;
    mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("recording");

    let result = xtask(&[
        "record",
        "--host",
        &server.uri(),
        "--out",
        out.to_str().unwrap(),
        "--course",
        "77",
    ]);
    assert!(
        result.status.success(),
        "record failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let files = read_dir(&out);
    assert!(
        files.contains_key("get-users-self.json"),
        "names: {:?}",
        files.keys().collect::<Vec<_>>()
    );
    assert!(files.contains_key("get-courses-77-files.json"));

    // The four headers the client reads are kept; nothing else is.
    let user: Value = serde_json::from_str(&files["get-users-self.json"]).unwrap();
    assert_eq!(user["headers"]["x-rate-limit-remaining"], "597.4");
    assert_eq!(user["headers"]["x-request-cost"], "0.31");
    assert_eq!(user["headers"]["date"], "Tue, 09 Sep 2026 17:05:12 GMT");
    let kept: Vec<&String> = user["headers"]
        .as_object()
        .unwrap()
        .keys()
        .collect::<Vec<_>>();
    assert_eq!(
        kept,
        vec!["date", "x-rate-limit-remaining", "x-request-cost"]
    );
    assert_eq!(user["status"], 200);
    assert_eq!(user["path"], "/api/v1/users/self");

    // A `Link` header on a collection is kept and the next page is followed.
    let courses_name = "get-courses-enrollment_type-student-enrollment_state-active.json";
    let courses: Value = serde_json::from_str(files.get(courses_name).unwrap_or_else(|| {
        panic!(
            "no {courses_name} in {:?}",
            files.keys().collect::<Vec<_>>()
        )
    }))
    .unwrap();
    assert!(
        courses["headers"]["link"]
            .as_str()
            .unwrap()
            .contains("next")
    );
    assert!(
        files.keys().any(|n| n.ends_with("-page-2.json")),
        "the next page was not recorded: {:?}",
        files.keys().collect::<Vec<_>>()
    );

    // The two context endpoints of Appendix B are walked as well.
    assert!(
        files.keys().any(|n| n.starts_with("get-announcements")),
        "announcements not recorded: {:?}",
        files.keys().collect::<Vec<_>>()
    );
    assert!(
        files.keys().any(|n| n.starts_with("get-calendar_events")),
        "calendar events not recorded: {:?}",
        files.keys().collect::<Vec<_>>()
    );

    // Nothing on disk carries the token, a cookie, or a capability. A recording
    // is a scratch artifact, but a capability in it is live the moment it lands
    // (SPEC §15), so it is stripped at record time, not at sanitize time.
    for (name, text) in &files {
        let lower = text.to_lowercase();
        assert!(!text.contains(TOKEN), "{name} carries the token");
        assert!(!lower.contains("set-cookie"), "{name}");
        assert!(!lower.contains("authorization"), "{name}");
        assert!(!lower.contains("x-amz-"), "{name}");
        for capability in ["verifier", "\"sig\"", "signature", "capability"] {
            assert!(
                !lower.contains(capability),
                "{name} carries {capability}: {text}"
            );
        }
    }

    // The capability came off the URL; the rest of the URL is untouched, which
    // is what `sanitize` later pseudonymizes.
    let course_files: Value = serde_json::from_str(&files["get-courses-77-files.json"]).unwrap();
    assert_eq!(
        course_files["body"][0]["url"],
        "https://files.real.edu/files/501/download"
    );
    let user_avatar = user["body"]["avatar_url"].as_str().unwrap();
    assert_eq!(user_avatar, "https://canvas.real.edu/images/9/a");
    // The identifying values are still there: only `sanitize` removes those.
    assert_eq!(user["body"]["name"], "Ada Lovelace");
}

#[tokio::test]
async fn record_refuses_the_tracked_fixture_directory() {
    let server = MockServer::start().await;
    mount(&server).await;
    let result = xtask(&[
        "record",
        "--host",
        &server.uri(),
        "--out",
        "crates/canvas-api/tests/fixtures/pretend-set",
    ]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("refusing to record"), "{stderr}");
    assert!(stderr.contains("xtask sanitize"), "{stderr}");
    assert!(!Path::new("crates/canvas-api/tests/fixtures/pretend-set").exists());
}

#[tokio::test]
async fn a_recording_sanitizes_into_a_tracked_set_and_a_rerun_changes_nothing() {
    let server = MockServer::start().await;
    mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("recording");
    let once = dir.path().join("fixtures").join("demo");
    let twice = dir.path().join("fixtures").join("demo-again");

    let result = xtask(&[
        "record",
        "--host",
        &server.uri(),
        "--out",
        raw.to_str().unwrap(),
        "--course",
        "77",
    ]);
    assert!(result.status.success());

    for (input, output) in [(&raw, &once), (&once, &twice)] {
        let result = xtask(&[
            "sanitize",
            "--in",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "sanitize failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let first = read_dir(&once);
    let second = read_dir(&twice);
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        second.keys().collect::<Vec<_>>()
    );
    for (name, text) in &first {
        if name == "MANIFEST.json" {
            continue; // The manifest names its own directory.
        }
        assert_eq!(text, &second[name], "{name} changed on the second pass");
    }

    // The identifying values are gone, in every file.
    for leak in [
        "Ada Lovelace",
        "alovelace",
        "ada@campus.example.test",
        "capability",
        "verifier",
        "canvas.real.edu",
        "files.real.edu",
        TOKEN,
    ] {
        for (name, text) in &first {
            assert!(
                !text.contains(leak),
                "{leak} survived sanitizing in {name}: {text}"
            );
        }
    }
    // The free text kept its Markdown and HTML structure.
    let course = &first["get-courses-enrollment_type-student-enrollment_state-active.json"];
    assert!(course.contains("<p>"), "{course}");
    assert!(course.contains("<b>"), "{course}");
    assert!(!course.contains("chapter"), "{course}");

    // The manifest describes the set.
    let manifest: Value = serde_json::from_str(&first["MANIFEST.json"]).unwrap();
    assert_eq!(manifest["set"], "demo");
    assert_eq!(manifest["redaction_version"], 1);
    assert!(manifest["recorded_at"].as_str().unwrap().contains('T'));
    // The manifest lists pseudonyms only, never the values they replaced.
    let pseudonyms = manifest["pseudonyms"].to_string();
    for leak in [
        "Ada",
        "Lovelace",
        "alovelace",
        "campus",
        "Syllabus",
        "\"77\"",
    ] {
        assert!(!pseudonyms.contains(leak), "{leak} is in {pseudonyms}");
    }
    assert!(
        manifest["pseudonyms"]["issued_ids"]
            .as_array()
            .unwrap()
            .iter()
            .all(|id| id.as_i64().unwrap() >= 900_000_000)
    );
    let endpoints = manifest["endpoints"].as_array().unwrap();
    assert!(endpoints.iter().any(|e| e == "GET /api/v1/users/self"));
    // Course IDs in the endpoint list went through the same mapping.
    assert!(
        !endpoints
            .iter()
            .any(|e| e.as_str().unwrap().contains("/77/")),
        "{endpoints:?}"
    );
}

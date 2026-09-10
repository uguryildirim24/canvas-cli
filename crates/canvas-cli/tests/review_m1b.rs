//! End-to-end M1-b regressions. Each invocation has isolated identity/config data.
use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, process::Command};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

const TOKEN: &str = "review-secret-token";
struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}
impl Fixture {
    fn new(origin: &str, validated: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new(origin, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        if validated {
            let key = doc.key.to_string();
            open.store.call_blocking(move |conns| {
                conns.state.execute("INSERT INTO credential (identity_key,token_sha256,validated_at) VALUES (?1,?2,'2026-01-01T00:00:00Z')", rusqlite::params![key, format!("{:x}",Sha256::digest(TOKEN.as_bytes()))])?;
                Ok(())
            }).unwrap();
        }
        Self { dir, doc }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_canvas"));
        command
            .env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("TZ", "America/New_York")
            .env("COLUMNS", "100")
            .env("CANVAS_TOKEN", TOKEN)
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        command
    }
    async fn run(&self, args: &[&str], code: i32) -> Value {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["exit"], code);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        v
    }
}
fn course(id: i64, code: &str) -> Value {
    json!({"id":id,"course_code":code,"name":"Test Course", "term":{"id":7,"name":"Fall"},
        "enrollments":[{"type":"student","computed_current_score":95,"computed_final_score":90,"current_grading_period_id":8,"current_grading_period_title":"Q1","current_period_computed_current_score":80,"current_period_computed_final_score":75}],"is_favorite":true})
}

#[tokio::test]
async fn courses_network_cached_offline_and_grade_mode() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    Mock::given(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-request-cost", "0.75")
                .set_body_json(json!([course(1, "A")])),
        )
        .expect(3)
        .mount(&server)
        .await;
    let network = f.run(&["courses"], 0).await;
    assert_eq!(
        network["requests"],
        json!({"api":1,"storage":0,"cost":0.75})
    );
    assert_eq!(
        network["result"]["courses"][0]["grades"]["current_score"],
        80.0
    );
    assert_eq!(
        network["result"]["courses"][0]["grades"]["period"]["mode"],
        "current"
    );
    let cached = f.run(&["courses"], 0).await;
    assert_eq!(cached["requests"]["api"], 0);
    let bad_token = f
        .command()
        .env("CANVAS_TOKEN", "invalid\nheader")
        .args(["courses", "--json"])
        .output()
        .unwrap();
    assert!(
        bad_token.status.success(),
        "{}",
        String::from_utf8_lossy(&bad_token.stdout)
    );
    let offline = f.run(&["courses", "--offline"], 0).await;
    assert!(
        offline["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["stale"] == true)
    );
    assert_eq!(
        f.run(&["courses", "--fresh"], 0).await["requests"]["api"],
        1
    );
    let mut later = f.command();
    later
        .env("CANVAS_NOW", "2026-09-09T17:16:12Z")
        .args(["courses", "--json"]);
    let output = tokio::task::spawn_blocking(move || later.output().unwrap())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["requests"]["api"],
        1
    );
}

#[tokio::test]
async fn course_detail_uses_one_object_request_and_caches_allowlisted_markdown() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    let mut body = course(42, "CS");
    body["syllabus_body"] = json!("<p>Hello <strong>class</strong></p>");
    body["teachers"] = json!([{"id":7,"name":"Ada","email":"private@example.com"}]);
    body["unexpected_raw_body"] = json!("untrusted secret body");
    Mock::given(path("/api/v1/courses/42"))
        .and(query_param("include[]", "syllabus_body"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;
    let live = f.run(&["course", "42"], 0).await;
    assert_eq!(live["requests"]["api"], 1);
    assert_eq!(
        live["result"]["course"]["syllabus_markdown"],
        "Hello **class**"
    );
    assert_eq!(
        live["result"]["course"]["teachers"],
        json!([{"id":"7","name":"Ada"}])
    );
    assert_eq!(
        f.run(&["course", "42", "--offline"], 0).await["requests"]["api"],
        0
    );
    let paths = Paths::for_identity(f.dir.path().join("data"), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).unwrap();
    open.store
        .call(|conns| {
            let data: String = conns
                .cache
                .query_row("SELECT data_json FROM courses", [], |r| r.get(0))?;
            assert!(!data.contains("private@example.com"));
            assert!(!data.contains("unexpected_raw_body"));
            assert!(!data.contains("<p>"));
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn cold_offline_detail_is_miss_and_local_alias_never_uses_token() {
    let f = Fixture::new("https://canvas.test", false);
    f.run(&["course", "42", "--offline"], 7).await;
    let mut cmd = f.command();
    cmd.env("CANVAS_TOKEN", "invalid\nheader")
        .args(["alias", "set", "cs", "42", "--json"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["requests"]["api"],
        0
    );
    let missing = f.run(&["alias", "set", "other", "biology"], 6).await;
    assert_eq!(missing["result"]["message"], "use a numeric ID or a URL");
}

#[tokio::test]
async fn resolver_refreshes_both_missing_scopes_before_detail() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    Mock::given(path("/api/v1/courses"))
        .and(query_param("enrollment_state", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses"))
        .and(query_param("enrollment_state", "completed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([course(9, "HIST")])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses"))
        .and(query_param("enrollment_state", "invited_or_pending"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(course(9, "HIST")))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(f.run(&["course", "hist"], 0).await["requests"]["api"], 5);
}

#[tokio::test]
async fn token_identity_mismatch_aborts_before_course_data_is_joined() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), false);
    Mock::given(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":999})))
        .expect(1)
        .mount(&server)
        .await;
    let result = f.run(&["courses"], 3).await;
    assert_eq!(result["requests"]["api"], 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn api_abort_codes_status_and_request_telemetry() {
    for (status, exit) in [(401, 3), (404, 6), (422, 8)] {
        let server = MockServer::start().await;
        let f = Fixture::new(&server.uri(), true);
        Mock::given(path("/api/v1/courses"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"errors":[TOKEN]})))
            .mount(&server)
            .await;
        let result = f.run(&["courses"], exit).await;
        assert_eq!(result["result"]["http_status"], status);
        assert_eq!(result["requests"]["api"], 1);
    }
}

#[tokio::test]
async fn sync_always_refreshes_and_reports_partial_course_denials() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    Mock::given(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([course(1, "A"), course(2, "B")])),
        )
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/users/self/enrollments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/1/grading_periods"))
        .respond_with(ResponseTemplate::new(403))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/2/grading_periods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"grading_periods":[]})))
        .expect(2)
        .mount(&server)
        .await;
    for _ in 0..2 {
        let result = f.run(&["sync"], 12).await;
        assert_eq!(result["outcome"], "partial");
        assert_eq!(result["partial"][0]["http_status"], 403);
        assert_eq!(result["requests"]["api"], 4);
        let rows = result["result"]["datasets"].as_array().unwrap();
        let keys: Vec<_> = rows
            .iter()
            .map(|r| format!("{}:{}", r["dataset"], r["scope"]))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(rows.iter().any(|r| r["dataset"] == "terms"));
        assert!(rows.iter().any(|r| r["dataset"] == "course_totals"));
    }
}

#[test]
fn profile_errors_are_identity_free_and_corrupt_config_is_local() {
    let f = Fixture::new("https://canvas.test", false);
    let output = f
        .command()
        .args(["courses", "--profile", "absent", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(v["profile"].is_null());
    assert!(v["identity"].is_null());
    let config = f.dir.path().join("config/canvas-cli");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("config.toml"), "invalid = [").unwrap();
    let output = f.command().args(["courses", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(13));
    let output = f
        .command()
        .env("CANVAS_IDENTITY_KEY", "../outside")
        .args(["cache", "path", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(13));
}

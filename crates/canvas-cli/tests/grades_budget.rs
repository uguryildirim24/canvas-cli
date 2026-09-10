//! M4-a request budgets on fixtures (SPEC §10: 2 for the overview, +1 per
//! course for the course view, +1 grading periods, +1 enrollments per
//! explicit period). Each invocation has isolated identity/config data.

use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::process::Command;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const TOKEN: &str = "grades-secret-token";

struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}

impl Fixture {
    fn new(origin: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new(origin, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        let key = doc.key.to_string();
        open.store
            .call_blocking(move |conns| {
                conns.state.execute(
                    "INSERT INTO credential (identity_key, token_sha256, validated_at)
                     VALUES (?1, ?2, '2026-01-01T00:00:00Z')",
                    rusqlite::params![key, format!("{:x}", Sha256::digest(TOKEN.as_bytes()))],
                )?;
                Ok(())
            })
            .unwrap();
        Self { dir, doc }
    }

    async fn run(&self, args: &[&str], code: i32) -> Value {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_canvas"));
        cmd.env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("TZ", "America/New_York")
            .env("COLUMNS", "100")
            .env("CANVAS_TOKEN", TOKEN)
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE")
            .args(args)
            .args(["--json", "--color", "never"]);
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
        // A token must never reach the envelope.
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        v
    }
}

fn course(id: i64, code: &str) -> Value {
    json!({
        "id": id,
        "course_code": code,
        "name": format!("Course {code}"),
        "enrollments": [{
            "type": "student",
            "computed_current_score": 88.0,
            "computed_current_grade": "B+",
            "computed_final_score": 85.0,
            "computed_final_grade": "B",
            "current_period_computed_current_score": 91.5,
            "current_period_computed_current_grade": "A-",
            "current_grading_period_id": "5",
            "current_grading_period_title": "Fall Term 1"
        }]
    })
}

async fn mount_courses(server: &MockServer, count: u64) {
    Mock::given(method("GET"))
        .and(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([course(1, "CHEM"), course(2, "MATH")])),
        )
        .expect(count)
        .mount(server)
        .await;
}

async fn mount_enrollments(server: &MockServer, period: Option<&str>, count: u64) {
    let mut mock = Mock::given(method("GET")).and(path("/api/v1/users/self/enrollments"));
    if let Some(p) = period {
        mock = mock.and(query_param("grading_period_id", p));
    }
    mock.respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id": 900, "course_id": 1, "grades": {"current_score": 88.0, "current_grade": "B+"}},
        {"id": 901, "course_id": 2, "grades": {"current_score": 70.0, "current_grade": "C-"}}
    ])))
    .expect(count)
    .mount(server)
    .await;
}

#[tokio::test]
async fn the_overview_costs_two_requests_and_reruns_from_cache() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount_courses(&server, 1).await;
    mount_enrollments(&server, None, 1).await;

    // SPEC §10 baseline: courses + enrollments.
    let out = f.run(&["grades", "--period", "all"], 0).await;
    assert_eq!(out["requests"]["api"], 2, "{out}");
    assert_eq!(out["result"]["courses"].as_array().unwrap().len(), 2);
    assert_eq!(out["result"]["period_mode"], "all");

    // A rerun inside the TTL costs nothing.
    let again = f.run(&["grades", "--period", "all"], 0).await;
    assert_eq!(again["requests"]["api"], 0, "{again}");
    assert_eq!(again["result"], out["result"]);
    server.verify().await;
}

#[tokio::test]
async fn the_course_view_adds_grading_periods_and_one_group_fetch() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount_courses(&server, 1).await;
    mount_enrollments(&server, None, 1).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/grading_periods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "grading_periods": [
                {"id": "5", "title": "Fall Term 1",
                 "start_date": "2026-09-01T00:00:00Z", "end_date": "2026-10-31T00:00:00Z"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignment_groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "20", "name": "Homework", "position": 1, "group_weight": 100.0,
             "rules": {}, "assignments": [
                {"id": "31", "name": "PS1", "points_possible": 25.0,
                 "due_at": "2026-09-14T03:59:00Z",
                 "submission": {"score": 23.0, "workflow_state": "graded"}}
             ]}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    // courses + enrollments + grading periods + one group fetch.
    let out = f.run(&["grades", "CHEM", "--period", "all"], 0).await;
    assert_eq!(out["requests"]["api"], 4, "{out}");
    let view = &out["result"]["course"];
    assert_eq!(view["groups"][0]["name"], "Homework");
    assert_eq!(view["groups"][0]["assignments"][0]["id"], "31");
    assert_eq!(view["periods"][0]["id"], "5");
    server.verify().await;
}

#[tokio::test]
async fn an_explicit_period_costs_one_more_enrollments_fetch() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount_courses(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/grading_periods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "grading_periods": [{"id": "5", "title": "Fall Term 1"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/self/enrollments"))
        .and(query_param("grading_period_id", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 900, "course_id": 1, "grades": {"current_score": 77.0, "current_grade": "C+"}}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignment_groups"))
        .and(query_param("grading_period_id", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    // courses + grading periods + period enrollments + period groups.
    let out = f.run(&["grades", "CHEM", "--period", "5"], 0).await;
    assert_eq!(out["requests"]["api"], 4, "{out}");
    let grades = &out["result"]["courses"][0]["grades"];
    // The explicit period's own values, titled from grading_periods.
    assert_eq!(grades["current_score"], 77.0);
    assert_eq!(grades["period"]["mode"], "id");
    assert_eq!(grades["period"]["id"], "5");
    assert_eq!(grades["period"]["title"], "Fall Term 1");
    server.verify().await;
}

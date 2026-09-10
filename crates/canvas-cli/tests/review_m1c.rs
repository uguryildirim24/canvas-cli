//! End-to-end M1-c regressions. Each invocation has isolated identity/config data.
use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::process::Command;
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
impl Fixture {
    fn store(&self) -> OpenIdentity {
        OpenIdentity::open(
            &Paths::for_identity(self.dir.path().join("data"), &self.doc.key),
            &self.doc,
        )
        .unwrap()
    }
    fn seed(&self) {
        self.store().store.call_blocking(|c|{
            c.cache.execute("INSERT INTO courses(id,course_code,name) VALUES(1,'CHEM','Chemistry')",[])?;
            c.cache.execute("INSERT INTO assignments(id,course_id,name,due_at,submitted,graded,missing) VALUES(2,1,'Homework','2026-09-10T03:59:00Z',1,1,0)",[])?;
            for (dataset,scope,kind,id) in [("courses","active","course","1"),("assignments","course:1","assignment","2"),("assignment","assignment:2","assignment","2")] {
                c.cache.execute("INSERT INTO fetch_log(dataset,scope,fetched_at,complete,count,stale,epoch_seen) VALUES(?1,?2,'2026-09-09T17:05:12Z',1,1,0,0)",rusqlite::params![dataset,scope])?;
                c.cache.execute("INSERT INTO membership(dataset,scope,entity_kind,entity_id,position) VALUES(?1,?2,?3,?4,0)",rusqlite::params![dataset,scope,kind,id])?;
            }
            for (dataset,scope) in [("planner","window:2026-09-08..2026-09-21"),("missing","all")] {
                c.cache.execute("INSERT INTO fetch_log(dataset,scope,fetched_at,complete,count,stale,epoch_seen,contexts,window_start,window_end) VALUES(?1,?2,'2026-09-09T17:05:12Z',1,0,0,0,'','2026-09-08T00:00:00Z','2026-09-22T00:00:00Z')",[dataset,scope])?;
            }
            Ok(())
        }).unwrap();
    }
    async fn human(&self, args: &[&str]) -> String {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
async fn mount(server: &MockServer, url: &str, value: Value, count: u64) {
    Mock::given(path(url))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .expect(count)
        .mount(server)
        .await;
}
fn assignment(id: i64) -> Value {
    json!({"id":id,"course_id":1,"name":format!("Homework {id}"),"due_at":"2026-09-10T03:59:00Z","points_possible":10,"description":"<p>Explain <strong>chemistry</strong>.</p>","submission_types":["online_upload"],"allowed_extensions":["pdf"],"allowed_attempts":3,"locked_for_user":false,"group_category_id":null,"rubric":[{"id":"c1","description":"Reasoning","points":10}],"submission":{"workflow_state":"unsubmitted","attempt":0,"late":false,"missing":false,"excused":false},"workflow_state":"published"})
}
fn normalized(mut value: Value) -> Value {
    value["identity"] = json!({"origin":"https://canvas.test","user_id":"123","key":"fixture"});
    value
}

#[tokio::test]
async fn todo_baseline_three_requests_and_all_complete_offline() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    mount(
        &server,
        "/api/v1/courses",
        json!([{"id":1,"course_code":"CHEM","name":"Chemistry"}]),
        1,
    )
    .await;
    mount(&server,"/api/v1/planner/items",json!([{"plannable_type":"assignment","plannable_id":2,"course_id":1,"plannable_date":"2026-09-10T03:59:00Z","plannable":{"id":2,"title":"Homework 2","due_at":"2026-09-10T03:59:00Z"},"submissions":{"submitted":false,"graded":false}}]),1).await;
    mount(
        &server,
        "/api/v1/users/self/missing_submissions",
        json!([]),
        1,
    )
    .await;
    let out = f.run(&["todo"], 0).await;
    assert_eq!(out["requests"]["api"], 3);
    assert_eq!(out["freshness"].as_array().unwrap().len(), 3);
    assert_eq!(out["result"]["counts"]["due_today"], 1);
    insta::assert_json_snapshot!("todo_json", normalized(out));
    insta::assert_snapshot!("todo_human", f.human(&["todo", "--offline"]).await);
    assert_eq!(f.run(&["todo", "--offline"], 0).await["requests"]["api"], 0);
    f.run(&["todo", "--all", "--offline"], 7).await;
    let mut undated = assignment(3);
    undated["due_at"] = Value::Null;
    mount(
        &server,
        "/api/v1/courses/1/assignments",
        json!([assignment(2), undated]),
        1,
    )
    .await;
    let all = f.run(&["todo", "--all"], 0).await;
    assert_eq!(all["requests"]["api"], 1);
    assert_eq!(all["result"]["items"].as_array().unwrap().len(), 2);
    let offline = f.run(&["todo", "--all", "--offline"], 0).await;
    assert_eq!(offline["result"], all["result"]);
    assert_eq!(offline["requests"]["api"], 0);
}

#[tokio::test]
async fn assignment_detail_is_one_request_and_renderers_are_complete() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    let mut value = assignment(2);
    value["can_submit"] = json!(false);
    value["html_url"] = json!("https://canvas.test/courses/1/assignments/2");
    Mock::given(path("/api/v1/courses/1/assignments/2"))
        .and(query_param("include[]", "can_submit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .expect(1)
        .mount(&server)
        .await;
    let detail = f.run(&["assignment", "1", "2"], 0).await;
    assert_eq!(detail["requests"]["api"], 1);
    assert_eq!(detail["result"]["assignment"]["can_submit"], false);
    assert!(
        detail["result"]["assignment"]["description_markdown"]
            .as_str()
            .unwrap()
            .contains("**chemistry**")
    );
    insta::assert_json_snapshot!("assignment_json", normalized(detail));
    insta::assert_snapshot!(
        "assignment_human",
        f.human(&["assignment", "1", "2", "--offline"]).await
    );
    // Numeric and URL forms never fetch the list.
    f.run(&["assignments", "1", "--offline"], 7).await;
    let url = format!("{}/courses/1/assignments/2", server.uri());
    assert_eq!(f.run(&["assignment", &url], 0).await["requests"]["api"], 0);
    let mut later = assignment(3);
    later["due_at"] = Value::Null;
    mount(
        &server,
        "/api/v1/courses/1/assignments",
        json!([later, assignment(2)]),
        1,
    )
    .await;
    let list = f.run(&["assignments", "1", "--bucket", "all"], 0).await;
    assert_eq!(list["result"]["assignments"][0]["id"], "2");
    assert_eq!(list["result"]["assignments"][1]["id"], "3");
    insta::assert_json_snapshot!("assignments_json", normalized(list));
    insta::assert_snapshot!(
        "assignments_human",
        f.human(&["assignments", "1", "--bucket", "all", "--offline"])
            .await
    );
    let unsubmitted = f
        .run(
            &["assignments", "1", "--bucket", "unsubmitted", "--offline"],
            0,
        )
        .await;
    assert_eq!(
        unsubmitted["result"]["assignments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(unsubmitted["result"]["assignments"][0]["id"], "3");
}

#[tokio::test]
async fn graded_detail_fetches_feedback_and_external_tool_prints_open() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    let mut value = assignment(2);
    value["can_submit"] = json!(true);
    value["submission"]["workflow_state"] = json!("graded");
    value["submission"]["score"] = json!(9);
    value["submission_types"] = json!(["external_tool"]);
    value["external_tool_tag_attributes"] = json!({"name":"Lab Tool"});
    mount(&server, "/api/v1/courses/1/assignments/2", value, 1).await;
    mount(&server,"/api/v1/courses/1/assignments/2/submissions/self",json!({"id":20,"assignment_id":2,"rubric_assessment":{"c1":{"points":9,"comments":"Good"}},"submission_comments":[]}),1).await;
    let out = f.run(&["assignment", "1", "2"], 0).await;
    assert_eq!(out["requests"]["api"], 2);
    assert_eq!(
        out["result"]["assignment"]["rubric_assessment"][0]["points"],
        9.0
    );
    assert_eq!(out["result"]["assignment"]["rubric_assessed"], true);
    let human = f.human(&["assignment", "1", "2", "--offline"]).await;
    assert!(human.contains("Lab Tool"));
    assert!(human.contains("canvas open assignment 1 2"));
}

#[tokio::test]
async fn incomplete_offline_resolution_and_open_origin_errors() {
    let f = Fixture::new("https://canvas.test", true);
    for args in [
        vec!["todo", "--offline"],
        vec!["assignments", "1", "--offline"],
        vec!["assignment", "1", "2", "--offline"],
        vec!["assignment", "1", "homework", "--offline"],
    ] {
        f.run(&args, 7).await;
    }
    for args in [
        vec!["open", "chem"],
        vec!["open", "assignment", "1", "homework"],
        vec!["open", "https://canvas.test.evil.test/courses/1"],
        vec!["open", "https://canvas.test@evil.test/courses/1"],
        vec!["assignment", "https://evil.test/courses/1/assignments/2"],
        vec![
            "assignment",
            "1",
            "https://canvas.test/courses/2/assignments/2",
        ],
    ] {
        let out = f.run(&args, 6).await;
        assert_eq!(out["requests"]["api"], 0);
    }
}

#[tokio::test]
async fn name_resolution_fetches_once_and_reports_ambiguity_candidates() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    mount(
        &server,
        "/api/v1/courses",
        json!([{"id":1,"course_code":"CHEM","name":"Chemistry"}]),
        1,
    )
    .await;
    mount(
        &server,
        "/api/v1/courses/1/assignments",
        json!([assignment(2), assignment(3)]),
        1,
    )
    .await;
    let ambiguous = f.run(&["assignment", "chem", "homework"], 6).await;
    assert_eq!(ambiguous["requests"]["api"], 2);
    assert_eq!(
        ambiguous["result"]["details"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut detail = assignment(2);
    detail["can_submit"] = json!(true);
    mount(&server, "/api/v1/courses/1/assignments/2", detail, 1).await;
    assert_eq!(
        f.run(&["assignment", "chem", "homework 2"], 0).await["requests"]["api"],
        1
    );
}

#[tokio::test]
async fn pending_read_in_every_state_is_unknown_and_never_transitions() {
    let f = Fixture::new("https://canvas.test", true);
    f.seed();
    for state in [
        "planned",
        "uploading",
        "uploaded",
        "posting",
        "outcome_unknown",
    ] {
        let expected = state;
        let state = state.to_owned();
        f.store().store.call_blocking(move|c|{
            c.state.execute("DELETE FROM submission_journal",[])?;
            c.state.execute("INSERT INTO submission_journal(journal_id,identity_key,course_id,assignment_id,kind,state,created_at) VALUES('j1','k',1,2,'online_upload',?1,'2026-09-09T17:00:00Z')",[state])?;Ok(())
        }).unwrap();
        for args in [
            vec!["assignments", "1", "--bucket", "all", "--offline"],
            vec!["assignment", "1", "2", "--offline"],
            vec!["todo", "--all", "--offline"],
        ] {
            let out = f.run(&args, 0).await;
            let status = if args[0] == "assignment" {
                &out["result"]["assignment"]["status"]
            } else if args[0] == "assignments" {
                &out["result"]["assignments"][0]["status"]
            } else {
                &out["result"]["items"][0]["status"]
            };
            assert_eq!(status["pending"], true);
            assert!(status["submitted"].is_null());
            assert!(status["graded"].is_null());
        }
        let actual = f
            .store()
            .store
            .call_blocking(|c| {
                Ok(c.state
                    .query_row("SELECT state FROM submission_journal", [], |r| {
                        r.get::<_, String>(0)
                    })?)
            })
            .unwrap();
        assert_eq!(actual, expected);
    }
    f.store()
        .store
        .call_blocking(|c| {
            c.state.execute(
                "UPDATE submission_journal SET acknowledged_at='2026-09-09T17:04:00Z'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.run(&["assignments", "1", "--bucket", "all", "--offline"], 0)
            .await["result"]["assignments"][0]["status"]["pending"],
        false
    );
    f.store().store.call_blocking(|c|{c.state.execute("UPDATE submission_journal SET acknowledged_at=NULL",[])?;c.state.execute("INSERT INTO submission_journal(journal_id,identity_key,course_id,assignment_id,kind,state,created_at) VALUES('j2','k',1,2,'online_upload','submitted','2026-09-09T17:01:00Z')",[])?;Ok(())}).unwrap();
    assert_eq!(
        f.run(&["assignments", "1", "--bucket", "all", "--offline"], 0)
            .await["result"]["assignments"][0]["status"]["pending"],
        false
    );
}

#[tokio::test]
async fn authentication_and_api_failures_are_not_silently_successful() {
    for (status, exit) in [(401, 3), (403, 8), (404, 6), (422, 8)] {
        let server = MockServer::start().await;
        let f = Fixture::new(&server.uri(), true);
        Mock::given(path("/api/v1/courses/1/assignments/2"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({"errors":[{"message":"secret"}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let out = f.run(&["assignment", "1", "2"], exit).await;
        assert_eq!(out["requests"]["api"], 1);
        assert_eq!(out["result"]["http_status"], status);
    }
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), false);
    mount(&server, "/api/v1/users/self", json!({"id":999}), 1).await;
    assert_eq!(f.run(&["assignments", "1"], 3).await["requests"]["api"], 1);
}

#[tokio::test]
async fn stale_eligibility_refreshes_detail_and_never_fetches_orphans() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    f.seed();
    f.store().store.call_blocking(|c|{
        c.cache.execute("UPDATE assignments SET can_submit=0 WHERE id=2",[])?;
        c.cache.execute("INSERT INTO assignments(id,course_id,name,can_submit) VALUES(99,1,'orphan',0)",[])?;
        for id in ["2","99"]{c.cache.execute("INSERT INTO field_obs(entity_kind,entity_key,field,observed_at) VALUES('assignment',?1,'can_submit','2026-09-01T00:00:00Z')",[id])?;}
        Ok(())
    }).unwrap();
    let mut detail = assignment(2);
    detail["can_submit"] = json!(true);
    mount(&server, "/api/v1/courses/1/assignments/2", detail, 1).await;
    let out = f.run(&["assignments", "1", "--bucket", "all"], 0).await;
    assert_eq!(out["requests"]["api"], 1);
    assert_eq!(
        out["result"]["assignments"][0]["availability"]["submittable"],
        true
    );
    server.reset().await;
    f.store()
        .store
        .call_blocking(|c| {
            c.cache.execute(
                "UPDATE field_obs SET observed_at='2026-09-01T00:00:00Z' WHERE field='can_submit'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    Mock::given(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let stale = f.run(&["assignments", "1", "--bucket", "all"], 0).await;
    assert_eq!(stale["requests"]["api"], 1);
    assert!(stale["result"]["assignments"][0]["availability"]["submittable"].is_null());
    assert!(
        stale["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["stale"] == true)
    );
    assert!(!stale["warnings"].as_array().unwrap().is_empty());
}

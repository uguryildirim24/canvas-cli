//! M2-b over the merged M1-c tree: operand resolution, the `submission`
//! command, and the reader paths while a submission is in an active phase.
use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

const TOKEN: &str = "review-secret-token";

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
        open.store.call_blocking(move |conns| {
            conns.state.execute("INSERT INTO credential (identity_key,token_sha256,validated_at) VALUES (?1,?2,'2026-01-01T00:00:00Z')", rusqlite::params![key, format!("{:x}", Sha256::digest(TOKEN.as_bytes()))])?;
            Ok(())
        }).unwrap();
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

    fn store(&self) -> OpenIdentity {
        OpenIdentity::open(
            &Paths::for_identity(self.dir.path().join("data"), &self.doc.key),
            &self.doc,
        )
        .unwrap()
    }

    /// Cache the `courses:active` dataset so a course name or alias resolves
    /// without a fetch. The `assignments` dataset is left for the command.
    fn seed_courses(&self) {
        self.store().store.call_blocking(|c| {
            c.cache.execute("INSERT INTO courses(id,course_code,name) VALUES(1,'CHEM','Chemistry')", [])?;
            c.cache.execute("INSERT INTO fetch_log(dataset,scope,fetched_at,complete,count,stale,epoch_seen) VALUES('courses','active','2026-09-09T17:05:12Z',1,1,0,0)", [])?;
            c.cache.execute("INSERT INTO membership(dataset,scope,entity_kind,entity_id,position) VALUES('courses','active','course','1',0)", [])?;
            Ok(())
        }).unwrap();
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
            "args={args:?} stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit"], code);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        value
    }

    async fn human(&self, args: &[&str], code: i32) -> String {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

/// The assignment pre-flight `GET` (SPEC §12.2 step 1).
async fn mock_assignment(server: &MockServer) {
    Mock::given(path("/api/v1/courses/1/assignments/2"))
        .and(query_param("include[]", "can_submit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 2, "course_id": 1, "name": "Problem Set 3",
            "submission_types": ["online_text_entry", "online_upload", "online_url"],
            "can_submit": true, "due_at": "2026-09-10T03:59:00Z",
            "submission": {"attempt": 0}
        })))
        .mount(server)
        .await;
}

/// The `submission` dataset endpoint (M1-c).
fn submission_body() -> Value {
    json!({
        "id": 55, "assignment_id": 2, "user_id": 123, "attempt": 2,
        "score": 8.5, "grade": "8.5", "submitted_at": "2026-09-09T16:05:00Z",
        "workflow_state": "graded", "late": false, "missing": false, "excused": false,
        "submission_type": "online_upload", "posted_at": "2026-09-09T16:30:00Z",
        "body": null, "url": null,
        "attachments": [{"id": 55001, "display_name": "essay.pdf", "size": 24576, "content_type": "application/pdf"}],
        "submission_comments": [{"id": 9, "comment": "Nice work.", "author_name": "Prof. Ada", "created_at": "2026-09-09T16:40:00Z"}],
        "rubric_assessment": {"c1": {"points": 8.5, "comments": "Clear reasoning.", "rating_id": "_5721"}},
        "submission_history": [
            {"id": 55, "attempt": 2, "submitted_at": "2026-09-09T16:05:00Z", "workflow_state": "graded", "score": 8.5,
             "attachments": [{"id": 55001, "display_name": "essay.pdf", "size": 24576, "content_type": "application/pdf"}]},
            {"id": 54, "attempt": 1, "submitted_at": "2026-09-08T12:00:00Z", "workflow_state": "submitted", "score": null,
             "attachments": [{"id": 55000, "display_name": "draft.pdf", "size": 1024, "content_type": "application/pdf"}]}
        ]
    })
}

/// The `assignments:course:1` listing that backs assignment-name matching.
async fn mock_assignments_list(server: &MockServer) {
    Mock::given(path("/api/v1/courses/1/assignments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": 2, "course_id": 1, "name": "Problem Set 3",
            "due_at": "2026-09-10T03:59:00Z", "submission_types": ["online_upload"]
        }])))
        .mount(server)
        .await;
}

async fn mock_submission(server: &MockServer) {
    Mock::given(path("/api/v1/courses/1/assignments/2/submissions/self"))
        .and(query_param("include[]", "submission_history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(submission_body()))
        .mount(server)
        .await;
}

/// Pre-flight resolves `<course> <assignment>` and a bare assignment URL, and
/// refuses a URL whose origin or course does not agree (SPEC §6).
#[tokio::test]
async fn submit_resolves_names_aliases_and_urls_over_the_m1c_datasets() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    f.seed_courses();
    f.store()
        .store
        .call_blocking(|c| {
            canvas_core::resolve::alias_set(&c.state, "chem", 1).unwrap();
            Ok(())
        })
        .unwrap();
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    mock_assignment(&server).await;
    mock_assignments_list(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"id": 55, "attempt": 1, "submitted_at": "2026-09-09T17:05:12Z", "body": "<p>hello</p>"}),
        ))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/1/assignments/2/submissions/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"attempt": 1, "submission_history": [{"id": 55, "attempt": 1, "submitted_at": "2026-09-09T17:05:12Z", "body": "<p>hello</p>", "attachments": []}]}),
        ))
        .mount(&server)
        .await;

    let text = input.to_str().unwrap();
    // An alias for the course and a name substring for the assignment.
    let by_name = f
        .run(
            &["submit", "chem", "Problem Set", "--text", text, "--yes"],
            0,
        )
        .await;
    assert_eq!(by_name["result"]["state"], "submitted");
    assert!(
        by_name["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["dataset"] == "assignments" && row["scope"] == "course:1"),
        "the fetched resolution dataset is reported: {by_name}"
    );

    // A single assignment URL supplies both ids.
    let url = format!("{}/courses/1/assignments/2", server.uri());
    let by_url = f.run(&["submit", &url, "--text", text, "--yes"], 0).await;
    assert_eq!(by_url["result"]["state"], "submitted");

    // A URL naming a different course than the course operand: exit 6.
    let other_course = format!("{}/courses/9/assignments/2", server.uri());
    let mismatch = f
        .run(&["submit", "1", &other_course, "--text", text, "--yes"], 6)
        .await;
    assert_eq!(mismatch["result"]["code"], "resolution");
    assert!(
        mismatch["result"]["message"]
            .as_str()
            .unwrap()
            .contains("course id mismatch")
    );

    // A URL from another origin: exit 6.
    let foreign = f
        .run(
            &[
                "submit",
                "https://other.instructure.com/courses/1/assignments/2",
                "--text",
                text,
                "--yes",
            ],
            6,
        )
        .await;
    assert_eq!(foreign["result"]["code"], "resolution");

    // A name that matches nothing lists candidates and never posts.
    let unknown = f
        .run(
            &["submit", "chem", "Nonexistent", "--text", text, "--yes"],
            6,
        )
        .await;
    assert_eq!(unknown["result"]["code"], "resolution");

    // One operand that is not a URL cannot name both ids.
    let usage = f.run(&["submit", "chem", "--text", text, "--yes"], 2).await;
    assert_eq!(usage["result"]["code"], "usage");

    // Neither refusal created a journal beyond the two real submissions.
    let journals = f.run(&["receipts", "list"], 0).await;
    assert_eq!(journals["result"]["journals"].as_array().unwrap().len(), 2);
}

/// `submission <course> <assignment> [--history]` over the M1-c dataset.
#[tokio::test]
async fn submission_show_reads_the_m1c_dataset_and_matches_the_registry_fixture() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    f.seed_courses();
    mock_assignments_list(&server).await;
    mock_submission(&server).await;

    // Resolving the assignment by name fetches and reports its dataset.
    let out = f.run(&["submission", "chem", "Problem Set"], 0).await;
    assert_eq!(out["schema"], "canvas-cli/submission@1");
    assert_eq!(out["requests"]["api"], 2);
    assert!(
        out["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["dataset"] == "assignments" && row["scope"] == "course:1"),
        "the resolved dataset is reported: {out}"
    );
    assert!(
        out["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["dataset"] == "submission" && row["scope"] == "assignment:2"),
        "the submission dataset is reported: {out}"
    );
    let result = &out["result"];
    assert_eq!(result["submission"]["attempt"], 2);
    assert_eq!(result["submission"]["graded"], true);
    assert_eq!(result["submission"]["submitted"], true);
    assert_eq!(result["submission"]["score"], 8.5);
    assert_eq!(result["submission"]["pending"], false);
    assert_eq!(result["submission"]["posted_at"], "2026-09-09T16:30:00Z");
    assert_eq!(
        result["submission"]["submitted_at_local"],
        "2026-09-09T12:05:00-04:00"
    );
    assert_eq!(result["submission"]["attachments"][0]["id"], "55001");
    assert_eq!(result["submission"]["comments"][0]["text"], "Nice work.");
    assert_eq!(result["submission"]["comments"][0]["author"], "Prof. Ada");
    assert_eq!(result["submission"]["rubric_assessed"], true);
    assert_eq!(
        result["submission"]["rubric_assessment"][0]["criterion_id"],
        "c1"
    );
    // Without --history the array is present and empty (Appendix D).
    assert_eq!(result["history"], json!([]));
    assert_eq!(result["pending_journals"], json!([]));

    // --history is sorted by attempt ascending.
    let with_history = f.run(&["submission", "1", "2", "--history"], 0).await;
    let history = with_history["result"]["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["attempt"], 1);
    assert_eq!(history[1]["attempt"], 2);
    assert_eq!(history[0]["attachments"][0]["id"], "55000");
    assert_eq!(history[1]["score"], 8.5);
    assert_eq!(
        history[0]["submitted_at_local"],
        "2026-09-08T08:00:00-04:00"
    );

    // Offline serves the cached dataset without a request.
    let offline = f
        .run(&["submission", "1", "2", "--history", "--offline"], 0)
        .await;
    assert_eq!(offline["requests"]["api"], 0);
    assert_eq!(offline["result"], with_history["result"]);

    // The registry fixture is this command's own output, not a placeholder.
    let fixture: Value =
        serde_json::from_str(include_str!("../src/output/schemas/submission.json")).unwrap();
    assert_eq!(
        fixture, with_history["result"],
        "crates/canvas-cli/src/output/schemas/submission.json must hold this result"
    );

    // An assignment URL is accepted next to the course operand, and its
    // course id must agree with it (SPEC §6).
    let url = format!("{}/courses/1/assignments/2", server.uri());
    let by_url = f.run(&["submission", "1", &url], 0).await;
    assert_eq!(by_url["result"]["submission"]["attempt"], 2);
    let other = format!("{}/courses/9/assignments/2", server.uri());
    let mismatch = f.run(&["submission", "1", &other], 6).await;
    assert_eq!(mismatch["result"]["code"], "resolution");

    // The CLI contract requires both operands; clap keeps its own text output.
    let mut cmd = f.command();
    cmd.args(["submission", "chem", "--json"]);
    let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires exactly two operands"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    insta::assert_snapshot!(
        "submission_human",
        f.human(&["submission", "1", "2", "--history", "--offline"], 0)
            .await
    );
}

/// A live owner holding the journal in an active phase, for the reader tests.
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Mount every endpoint the `todo` and `submission` readers use.
async fn mock_reader_sources(server: &MockServer) {
    mock_submission(server).await;
    Mock::given(path("/api/v1/courses"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{"id": 1, "course_code": "CHEM", "name": "Chemistry"}])),
        )
        .mount(server)
        .await;
    Mock::given(path("/api/v1/planner/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "plannable_type": "assignment", "plannable_id": 2, "course_id": 1,
            "plannable_date": "2026-09-10T03:59:00Z",
            "plannable": {"id": 2, "title": "Problem Set 3", "due_at": "2026-09-10T03:59:00Z"},
            "submissions": {"submitted": false, "graded": false}
        }])))
        .mount(server)
        .await;
    Mock::given(path("/api/v1/users/self/missing_submissions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
}

/// Run every read command against a journal held in `state` by a live owner.
///
/// The owner lock stays in this process, so each command below is a real
/// second process running its whole reader and renderer path.
async fn assert_readers_see_pending(f: &Fixture, journal_id: &str, state: &str) {
    let pending = f.run(&["submission", "1", "2", "--history"], 0).await;
    let submission = &pending["result"]["submission"];
    assert_eq!(submission["pending"], true, "{state}");
    assert_eq!(
        pending["result"]["pending_journals"],
        json!([journal_id]),
        "{state}"
    );
    for field in [
        "submitted",
        "graded",
        "score",
        "grade",
        "late",
        "excused",
        "workflow_state",
        "submitted_at",
        "submitted_at_local",
        "attempt",
        "posted_at",
    ] {
        assert_eq!(submission[field], Value::Null, "{field} at {state}");
    }
    // The server-observed payload around the status is still reported.
    assert_eq!(submission["attachments"][0]["id"], "55001", "{state}");
    assert_eq!(
        pending["result"]["history"].as_array().unwrap().len(),
        2,
        "history stays available at {state}"
    );
    let human = f.human(&["submission", "1", "2", "--history"], 0).await;
    assert!(human.contains("pending:"), "{state}: {human}");
    assert!(human.contains(journal_id), "{state}: {human}");

    let todo = f.run(&["todo"], 0).await;
    let item = todo["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["assignment_id"] == "2")
        .unwrap_or_else(|| panic!("assignment 2 is in the todo window at {state}"));
    assert_eq!(item["status"]["pending"], true, "{state}");
    assert_eq!(item["status"]["submitted"], Value::Null, "{state}");
    assert_eq!(item["status"]["graded"], Value::Null, "{state}");
    let todo_human = f.human(&["todo"], 0).await;
    assert!(todo_human.contains("pending"), "{state}: {todo_human}");

    let listed = f.run(&["receipts", "list"], 0).await;
    let journal = listed["result"]["journals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["journal_id"] == journal_id)
        .unwrap_or_else(|| panic!("journal listed at {state}"));
    assert_eq!(journal["owner"], "live", "{state}");
    assert_eq!(journal["state"], state, "{state}");

    let reconcile = f.run(&["submission", "reconcile", journal_id], 9).await;
    assert_eq!(reconcile["result"]["outcome"], "recovery", "{state}");
    assert_eq!(reconcile["result"]["owner"], "live", "{state}");
    assert_eq!(reconcile["result"]["state"], state, "{state}");
    assert_eq!(reconcile["requests"]["api"], 0, "{state}");
}

/// The `todo`, `submission`, `receipts` and `reconcile` reader paths run in a
/// second process during every active phase and never transition the journal.
#[tokio::test]
async fn readers_run_during_every_active_phase_without_transitioning() {
    use canvas_core::journal::{
        AdmissionLock, CreateOpts, State, TransitionPatch, create, get_journal, mark_posting,
        transition,
    };
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    f.seed_courses();
    mock_assignments_list(&server).await;
    mock_reader_sources(&server).await;
    // Warm both reader datasets before any journal exists.
    f.run(&["todo"], 0).await;
    f.run(&["submission", "1", "2", "--history"], 0).await;

    let open = f.store();
    let paths = Paths::for_identity(f.dir.path().join("data"), &f.doc.key);
    let payload = serde_json::to_string(&json!({
        "files": [{"name": "ps3.pdf", "size": 4, "sha256": "abc"}]
    }))
    .unwrap();

    for phase in [
        State::Planned,
        State::Uploading,
        State::Uploaded,
        State::Posting,
    ] {
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
        let (journal_id, owner) = create(
            &open.store,
            &paths.identity_dir,
            &admission,
            &CreateOpts {
                identity_key: f.doc.key.to_string(),
                course_id: 1,
                assignment_id: 2,
                kind: "online_upload".into(),
                intended_payload_json: payload.clone(),
                baseline_attempt: Some(0),
                baseline_submission_id: None,
            },
        )
        .unwrap();
        drop(admission);
        // Advance to the phase under test; the owner lock stays in this process.
        if phase != State::Planned {
            transition(
                &open.store,
                &owner,
                &journal_id,
                State::Planned,
                State::Uploading,
                TransitionPatch::default(),
            )
            .unwrap();
        }
        if matches!(phase, State::Uploaded | State::Posting) {
            transition(
                &open.store,
                &owner,
                &journal_id,
                State::Uploading,
                State::Uploaded,
                TransitionPatch::default(),
            )
            .unwrap();
        }
        if phase == State::Posting {
            mark_posting(&open.store, &owner, &journal_id).unwrap();
        }

        assert_readers_see_pending(&f, &journal_id, phase.as_str()).await;

        // No read moved the journal.
        assert_eq!(
            get_journal(&open.store, &journal_id)
                .unwrap()
                .unwrap()
                .state,
            phase,
            "a read transitioned the journal at {}",
            phase.as_str()
        );
        // Retire it so the next phase can take the admission lock. `posting`
        // may only leave for `outcome_unknown`; acknowledge that one so it
        // stops being pending for the assignment.
        if phase == State::Posting {
            transition(
                &open.store,
                &owner,
                &journal_id,
                phase,
                State::OutcomeUnknown,
                TransitionPatch::default(),
            )
            .unwrap();
            canvas_core::journal::acknowledge(&open.store, &journal_id).unwrap();
        } else {
            transition(
                &open.store,
                &owner,
                &journal_id,
                phase,
                State::Refused,
                TransitionPatch::default(),
            )
            .unwrap();
        }
        drop(owner);
    }
}

/// The same readers during a real `submit` that is waiting on its `POST`.
#[tokio::test]
async fn readers_report_pending_while_a_real_submit_is_posting() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    f.seed_courses();
    mock_assignment(&server).await;
    mock_assignments_list(&server).await;
    mock_reader_sources(&server).await;
    // The POST never answers within the test, so the owner stays in `posting`.
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({"id": 55, "attempt": 1}))
                .set_delay(Duration::from_secs(120)),
        )
        .mount(&server)
        .await;
    f.run(&["todo"], 0).await;
    f.run(&["submission", "1", "2", "--history"], 0).await;

    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    let mut owner = f.command();
    owner
        .args([
            "submit",
            "1",
            "2",
            "--text",
            input.to_str().unwrap(),
            "--yes",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _owner = Owner(owner.spawn().unwrap());

    let open = f.store();
    let deadline = Instant::now() + Duration::from_secs(30);
    let journal_id = loop {
        assert!(Instant::now() < deadline, "submit never reached `posting`");
        let row = open
            .store
            .call_blocking(|c| {
                use rusqlite::OptionalExtension;
                Ok(c.state
                    .query_row(
                        "SELECT journal_id FROM submission_journal WHERE state = 'posting'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?)
            })
            .unwrap();
        if let Some(id) = row {
            break id;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    assert_readers_see_pending(&f, &journal_id, "posting").await;

    let state: String = open
        .store
        .call_blocking(move |c| {
            Ok(c.state.query_row(
                "SELECT state FROM submission_journal WHERE journal_id = ?1",
                [&journal_id],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(state, "posting");
}

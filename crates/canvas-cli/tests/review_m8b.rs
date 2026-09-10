//! M8-b acceptance at the command surface: reply, send, status, reconcile.
//!
//! Every case here is one row of the REPORT §4 M8-b acceptance column that
//! needs a whole `canvas` process: the exit codes, the two new envelopes in
//! both modes, the receipts listing, and the promise that a refusal reaches
//! Canvas with nothing.

use std::process::Command;

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        OpenIdentity::open(&paths, &doc).unwrap();
        Self { dir, doc }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_canvas"));
        cmd.env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("TZ", "America/New_York")
            .env("COLUMNS", "100")
            .env("CANVAS_TOKEN", "m8b-test-token")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        cmd
    }

    async fn run(&self, args: &[&str], exit: i32) -> Value {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit"], exit);
        value
    }

    /// The human rendering of one command, for the table-mode snapshots.
    async fn text(&self, args: &[&str], exit: i32) -> String {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(exit), "{args:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

fn normalized(mut value: Value, origin: &str) -> Value {
    value["identity"] = json!({"origin":"https://canvas.test","user_id":"123","key":"fixture"});
    value["generated_at"] = json!("<generated_at>");
    scrub(&mut value["result"], origin.trim_end_matches('/'));
    value
}

/// The ids and read times this run generated are stable in shape, not value.
fn stabilize(value: &mut Value) {
    for key in ["journal_id", "plan_id", "receipt_id", "read_at"] {
        if let Some(field) = value.get_mut(key)
            && !field.is_null()
        {
            *field = json!(format!("<{key}>"));
        }
    }
}

fn scrub(value: &mut Value, origin: &str) {
    match value {
        Value::String(text) => *text = text.replace(origin, "<origin>"),
        Value::Array(items) => items.iter_mut().for_each(|item| scrub(item, origin)),
        Value::Object(map) => map.values_mut().for_each(|item| scrub(item, origin)),
        _ => {}
    }
}

/// The readback line names the moment it ran, which no snapshot can hold.
fn hide_read_time(text: &str) -> String {
    text.lines()
        .map(|line| match line.split_once(" at ") {
            Some((head, _)) if head.starts_with("read ") => format!("{head} at <read_at>"),
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

async fn mount(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route.to_owned()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn courses() -> Value {
    json!([{"id": 5, "course_code": "CHEM", "name": "Chemistry"}])
}

fn open_topic() -> Value {
    json!({
        "id": 55,
        "title": "Week 3 reading",
        "message": "<p>What did you make of it?</p>",
        "locked": false,
        "locked_for_user": false,
        "require_initial_post": false,
        "user_can_see_posts": true,
        "group_category_id": null,
        "group_topic_children": [],
        "discussion_type": "threaded",
        "published": true
    })
}

/// The entry `text_to_html("My reply.")` produces, as Canvas echoes it.
fn my_reply(id: i64) -> Value {
    json!({
        "id": id,
        "user_id": 123,
        "user_name": "You",
        "message": "<p>My reply.</p>",
        "created_at": "2026-09-09T17:00:00Z"
    })
}

fn somebody_else() -> Value {
    json!({
        "id": 900,
        "user_id": 31,
        "user_name": "Alex Kim",
        "message": "<p>Existing.</p>",
        "created_at": "2026-09-09T12:00:00Z"
    })
}

fn conversation() -> Value {
    json!({
        "id": 700,
        "subject": "Lab partner",
        "workflow_state": "read",
        "last_message_at": "2026-09-09T12:00:00Z",
        "context_name": "CHEM",
        "participants": [{"id": 123, "name": "You"}, {"id": 31, "name": "Alex Kim"}],
        "messages": [{
            "id": 9001,
            "author_id": 31,
            "created_at": "2026-09-09T12:00:00Z",
            "body": "Can we meet before the lab?",
            "generated": false,
            "attachments": []
        }]
    })
}

/// Every read the three writes and their read-backs make.
///
/// `entries` is the thread as a readback sees it, which is what tells the
/// three attribution steps apart.
async fn mount_reads(server: &MockServer, entries: Value) {
    mount(server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(server, "/api/v1/courses", courses()).await;
    mount(
        server,
        "/api/v1/courses/5/discussion_topics/55",
        open_topic(),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        entries,
    )
    .await;
    mount(server, "/api/v1/conversations/700", conversation()).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search/recipients"))
        .and(query_param("user_id", "31"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id": 31, "name": "Alex Kim"}])),
        )
        .mount(server)
        .await;
}

async fn mount_reply_post(server: &MockServer, status: u16, body: Value) {
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/5/discussion_topics/55/entries"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

/// A Canvas-shaped 500: the answer proves nothing either way.
fn server_error() -> Value {
    json!({"errors": [{"message": "internal"}]})
}

async fn posts_seen(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.method.as_str() == "POST")
        .count()
}

// -------------------------------------------------------------- the happy path

#[tokio::test]
async fn a_reply_posts_once_and_lists_beside_a_submission_receipt() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    mount_reply_post(&server, 200, my_reply(5003)).await;
    let f = Fixture::new(&server.uri());

    let out = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "55",
                "--text",
                "My reply.",
                "--yes",
            ],
            0,
        )
        .await;
    assert_eq!(out["schema"], "canvas-cli/operation@1");
    assert_eq!(out["outcome"], "ok");
    assert_eq!(out["result"]["state"], "posted");
    assert_eq!(out["result"]["kind"], "discussion_reply");
    assert_eq!(out["result"]["attribution"], "accepted");
    assert_eq!(out["result"]["delivery"], "observable");
    assert_eq!(out["result"]["response"]["id"], "5003");
    assert_eq!(out["result"]["replayed"], false);
    assert_eq!(posts_seen(&server).await, 1);

    // The receipt lists beside a submission, and carries its own kind.
    let receipts = f.run(&["receipts", "list"], 0).await;
    let journals = receipts["result"]["journals"].as_array().unwrap();
    assert_eq!(journals.len(), 1, "{receipts}");
    assert_eq!(journals[0]["kind"], "discussion_reply");
    assert_eq!(journals[0]["course_id"], "5");
    assert_eq!(journals[0]["assignment_id"], Value::Null);

    // `receipts show` finds it by journal id and by receipt id.
    let journal_id = out["result"]["journal_id"].as_str().unwrap();
    let shown = f.run(&["receipts", "show", journal_id], 0).await;
    assert_eq!(shown["result"]["journal"]["kind"], "discussion_reply");
    // The approval channel the flag stands for is on the record.
    assert_eq!(
        shown["result"]["journal"]["approval"]["channel"],
        "yes-flag"
    );

    let receipt_id = out["result"]["receipt_id"].as_str().unwrap();
    let by_receipt = f.run(&["receipts", "show", receipt_id], 0).await;
    let receipt = &by_receipt["result"]["receipt"];
    assert_eq!(receipt["operation"]["kind"], "discussion_reply");
    assert_eq!(receipt["operation"]["delivery"], "observable");
    assert_eq!(receipt["assignment_id"], Value::Null);

    // And it exports, by journal id, with the operation block intact.
    let out_path = f.dir.path().join("receipt.json");
    let exported = f
        .run(
            &[
                "receipts",
                "export",
                journal_id,
                "--out",
                out_path.to_str().unwrap(),
            ],
            0,
        )
        .await;
    assert_eq!(exported["result"]["receipt_id"], receipt_id, "{exported}");
    let document: Value = serde_json::from_slice(&std::fs::read(&out_path).unwrap()).unwrap();
    assert_eq!(document["operation"]["kind"], "discussion_reply");
    assert_eq!(document["assignment_id"], Value::Null);

    // §10: the thread is settled once the write resolves.
    let read = f.run(&["discussion", "5", "55"], 0).await;
    assert_eq!(read["result"]["pending"], false);
    assert!(
        read["result"]["pending_journals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_conversation_canvas_accepted_is_never_called_delivered() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/conversations"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!([{
            "id": 700,
            "subject": "Lab partner",
            "messages": [{"id": 8100, "author_id": 123, "created_at": "2026-09-09T17:00:00Z"}]
        }])))
        .mount(&server)
        .await;
    let f = Fixture::new(&server.uri());

    let out = f
        .run(
            &[
                "inbox",
                "send",
                "--to",
                "31",
                "--subject",
                "Lab partner",
                "--text",
                "Are you free Tuesday?",
                "--yes",
            ],
            0,
        )
        .await;
    assert_eq!(out["result"]["state"], "posted");
    assert_eq!(out["result"]["attribution"], "accepted");
    assert_eq!(out["result"]["delivery"], "not_observable");

    let human = f.text(&["receipts", "list"], 0).await;
    assert!(human.contains("inbox_send"), "{human}");

    // Neither mode ever turns an acceptance into delivered mail.
    let printed = format!("{out}{human}");
    for forbidden in ["delivered", "was received", "has read"] {
        assert!(!printed.contains(forbidden), "{forbidden} in {printed}");
    }
}

// --------------------------------------------------------------- the refusals

#[tokio::test]
async fn every_topic_refusal_is_exit_eight_with_its_reason_and_no_post() {
    for (patch, reason) in [
        (json!({"group_category_id": 770}), "group_write"),
        (json!({"locked_for_user": true}), "locked"),
        (
            json!({"require_initial_post": true, "user_can_see_posts": false}),
            "initial_post_required",
        ),
    ] {
        let server = MockServer::start().await;
        mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
        mount(&server, "/api/v1/courses", courses()).await;
        let mut topic = open_topic();
        for (key, value) in patch.as_object().unwrap() {
            topic[key.as_str()] = value.clone();
        }
        mount(&server, "/api/v1/courses/5/discussion_topics/55", topic).await;
        mount_reply_post(&server, 200, my_reply(5003)).await;
        let f = Fixture::new(&server.uri());

        let out = f
            .run(
                &[
                    "discussion",
                    "reply",
                    "5",
                    "55",
                    "--text",
                    "Hello.",
                    "--yes",
                ],
                8,
            )
            .await;
        assert_eq!(out["outcome"], "refused", "{out}");
        assert_eq!(out["result"]["details"]["reason"], reason);
        // A gate is never opened by a placeholder, and neither is anything else.
        assert_eq!(posts_seen(&server).await, 0, "{reason} posted something");
    }
}

#[tokio::test]
async fn an_unknown_recipient_is_refused_and_a_discussion_attachment_is_unsupported() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search/recipients"))
        .and(query_param("user_id", "999"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let f = Fixture::new(&server.uri());

    let unknown = f
        .run(
            &["inbox", "send", "--to", "999", "--text", "Hello.", "--yes"],
            8,
        )
        .await;
    assert_eq!(
        unknown["result"]["details"]["reason"], "unresolved",
        "{unknown}"
    );

    let note = f.dir.path().join("note.txt");
    std::fs::write(&note, b"x").unwrap();
    let unsupported = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "55",
                "--text",
                "Hello.",
                "--attach",
                note.to_str().unwrap(),
                "--yes",
            ],
            8,
        )
        .await;
    assert_eq!(
        unsupported["result"]["details"]["reason"], "unsupported",
        "{unsupported}"
    );
    assert_eq!(posts_seen(&server).await, 0);
}

#[tokio::test]
async fn a_url_from_another_origin_is_exit_six() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    let f = Fixture::new(&server.uri());

    let out = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "https://elsewhere.test/courses/5/discussion_topics/55",
                "--text",
                "Hello.",
                "--yes",
            ],
            6,
        )
        .await;
    assert_eq!(out["result"]["code"], "resolution", "{out}");
    assert_eq!(posts_seen(&server).await, 0);
}

#[tokio::test]
async fn an_unknown_journal_is_exit_six() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["operation", "status", "no-such-journal"], 6).await;
    assert_eq!(out["result"]["code"], "resolution", "{out}");
}

// ------------------------------------------------ unknown outcome and recovery

#[tokio::test]
async fn a_server_error_stays_unknown_and_reconcile_resolves_it_without_resending() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else()])).await;
    mount_reply_post(&server, 500, server_error()).await;
    let f = Fixture::new(&server.uri());

    let out = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "55",
                "--text",
                "My reply.",
                "--yes",
            ],
            9,
        )
        .await;
    assert_eq!(out["outcome"], "recovery", "{out}");
    assert_eq!(out["result"]["state"], "outcome_unknown");
    assert_eq!(out["result"]["attribution"], "none");
    let journal_id = out["result"]["journal_id"].as_str().unwrap().to_owned();

    // §10: the thread is uncertain while a write of the user's own is open.
    let read = f.run(&["discussion", "5", "55"], 0).await;
    assert_eq!(read["result"]["pending"], true, "{read}");
    assert_eq!(read["result"]["pending_journals"][0], journal_id.as_str());

    let before = posts_seen(&server).await;
    let resolved = f.run(&["operation", "reconcile", &journal_id], 9).await;
    assert_eq!(resolved["schema"], "canvas-cli/operation_reconcile@1");
    assert_eq!(resolved["result"]["verdict"], "not_found");
    assert_eq!(resolved["result"]["state"], "outcome_unknown");
    // The journal is minutes old, so nothing may be asserted about it yet.
    assert_eq!(resolved["result"]["assume_not_posted_available"], false);
    assert_eq!(posts_seen(&server).await, before, "reconcile reposted");

    // And the assertion itself is refused rather than quietly accepted.
    let too_early = f
        .run(
            &["operation", "reconcile", &journal_id, "--assume-not-posted"],
            9,
        )
        .await;
    assert_eq!(too_early["result"]["state"], "outcome_unknown");
    assert!(
        !too_early["warnings"].as_array().unwrap().is_empty(),
        "{too_early}"
    );

    // A journal that bears no receipt is an ineligible export: exit 8
    // (SPEC §12.2 and §14), not exit 13 for a journal that plainly exists.
    let out_path = f.dir.path().join("receipt.json");
    let ineligible = f
        .run(
            &[
                "receipts",
                "export",
                &journal_id,
                "--out",
                out_path.to_str().unwrap(),
            ],
            8,
        )
        .await;
    assert_eq!(ineligible["outcome"], "error", "{ineligible}");
    assert_eq!(ineligible["result"]["code"], "export", "{ineligible}");
}

#[tokio::test]
async fn a_digest_match_is_unproven_and_status_reads_the_thread_back() {
    let server = MockServer::start().await;
    // The thread already holds an entry of this identity with the same body,
    // and nothing links it to the request this process made.
    mount_reads(&server, json!([my_reply(6100)])).await;
    mount_reply_post(&server, 500, server_error()).await;
    let f = Fixture::new(&server.uri());

    let out = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "55",
                "--text",
                "My reply.",
                "--yes",
            ],
            9,
        )
        .await;
    let journal_id = out["result"]["journal_id"].as_str().unwrap().to_owned();

    let resolved = f.run(&["operation", "reconcile", &journal_id], 0).await;
    assert_eq!(resolved["result"]["verdict"], "matched", "{resolved}");
    assert_eq!(resolved["result"]["state"], "matched");
    assert_eq!(resolved["result"]["attribution"], "unproven");
    assert_eq!(resolved["result"]["server_match"]["id"], "6100");

    let status = f.run(&["operation", "status", &journal_id], 0).await;
    assert_eq!(status["schema"], "canvas-cli/operation@1");
    assert_eq!(status["result"]["state"], "matched");
    assert_eq!(status["result"]["attribution"], "unproven");
    assert_eq!(status["result"]["readback"]["id"], "6100");
}

// -------------------------------------------------------------- the snapshots

#[tokio::test]
async fn the_new_schemas_render_in_both_modes() {
    let server = MockServer::start().await;
    mount_reads(&server, json!([somebody_else(), my_reply(5003)])).await;
    mount_reply_post(&server, 200, my_reply(5003)).await;
    let f = Fixture::new(&server.uri());

    let mut out = f
        .run(
            &[
                "discussion",
                "reply",
                "5",
                "55",
                "--text",
                "My reply.",
                "--yes",
            ],
            0,
        )
        .await;
    let journal_id = out["result"]["journal_id"].as_str().unwrap().to_owned();
    let receipt_id = out["result"]["receipt_id"].as_str().unwrap().to_owned();
    stabilize(&mut out["result"]);
    insta::assert_json_snapshot!("m8b_operation_json", normalized(out, &server.uri()));

    let mut reconciled = f.run(&["operation", "reconcile", &journal_id], 0).await;
    stabilize(&mut reconciled["result"]);
    stabilize(&mut reconciled["result"]["readback"]);
    insta::assert_json_snapshot!(
        "m8b_operation_reconcile_json",
        normalized(reconciled, &server.uri())
    );

    let human = f.text(&["operation", "status", &journal_id], 0).await;
    insta::assert_snapshot!(
        "m8b_operation_table",
        human
            .replace(&journal_id, "<journal_id>")
            .replace(&receipt_id, "<receipt_id>")
    );

    let reconcile_human = f.text(&["operation", "reconcile", &journal_id], 0).await;
    insta::assert_snapshot!(
        "m8b_operation_reconcile_table",
        hide_read_time(
            &reconcile_human
                .replace(&journal_id, "<journal_id>")
                .replace(&receipt_id, "<receipt_id>")
        )
    );
}

/// `canvas schema` describes both new documents from their own result types.
#[tokio::test]
async fn schema_names_the_new_commands() {
    let server = MockServer::start().await;
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    let f = Fixture::new(&server.uri());

    for (command, id) in [
        ("operation status", "canvas-cli/operation@1"),
        ("operation reconcile", "canvas-cli/operation_reconcile@1"),
    ] {
        let text = f.text(&["schema", command], 0).await;
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(doc["schema"], id, "{command}");
        assert_eq!(doc["command"], command);
        assert_eq!(doc["result_source"], "result type", "{command}");
    }
}

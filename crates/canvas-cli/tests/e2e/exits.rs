//! One test per SPEC §14 exit code, each with its envelope snapshot.
//!
//! An abort carries the `error` schema; a completed command with a non-success
//! outcome carries its own schema with `outcome` set. Both are asserted here,
//! because the exit code alone does not say which of the two happened.

use serde_json::{Value, json};
use wiremock::ResponseTemplate;

use crate::harness::{ASSIGNMENT_ID, COURSE_ID, CanvasServer, E2e, Run};

/// Assert one invocation's exit code, schema, and `outcome`, then snapshot it.
fn expect(env: &E2e, name: &str, run: &Run, code: i32, schema: &str, outcome: &str) {
    run.assert_code(code);
    let value = run.json();
    assert_eq!(value["exit"], code, "the envelope repeats the process exit");
    assert_eq!(value["schema"], schema, "schema for {name}");
    assert_eq!(value["outcome"], outcome, "outcome for {name}");
    env.snapshot_json(name, run);
}

/// Assert an abort: the `error` schema with the given `result.code`.
fn expect_abort(env: &E2e, name: &str, run: &Run, code: i32, error_code: &str) {
    expect(env, name, run, code, "canvas-cli/error@1", "error");
    assert_eq!(run.json()["result"]["code"], error_code, "code for {name}");
}

#[tokio::test]
async fn exit_0_success() {
    let env = E2e::new();
    let run = env.run_local(&["version", "--json"]);
    expect(&env, "exit_0", &run, 0, "canvas-cli/version@1", "ok");
}

#[tokio::test]
async fn exit_1_unexpected_response_shape() {
    let server = CanvasServer::start().await;
    // Canvas answers 200, but with an object where the list belongs.
    server
        .override_get("/api/v1/courses", 200, json!({ "courses": "not a list" }))
        .await;
    let env = E2e::with_server(&server);
    let run = env.run(&["courses", "--json"]);
    expect_abort(&env, "exit_1", &run, 1, "response");
}

#[tokio::test]
async fn exit_2_usage() {
    let env = E2e::new();
    let run = env.run_local(&["courses", "--fresh", "--offline", "--json"]);
    // `--fresh --offline` is caught by clap, which writes its own message.
    run.assert_code(2);
    assert!(run.stdout.is_empty(), "a clap usage error writes no stdout");
    env.snapshot("exit_2_clap", &run);

    // The same code, raised by a command that already opened its session.
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["sync", "--offline", "--json"]);
    expect_abort(&env, "exit_2", &run, 2, "usage");
}

#[tokio::test]
async fn exit_3_auth() {
    let server = CanvasServer::start().await;
    // The credential row is already validated, so `sync` never revisits
    // `users/self`; the 401 has to come from the dataset request itself.
    server.override_status("/api/v1/courses", 401).await;
    let env = E2e::with_server(&server);
    let run = env.run(&["sync", "--json"]);
    expect_abort(&env, "exit_3", &run, 3, "auth");
}

#[tokio::test]
async fn exit_4_network() {
    // Port 1 is reserved and never bound, so the connection is refused before
    // any request is written.
    let env = E2e::with_identity("http://127.0.0.1:1");
    let run = env.run(&["sync", "--json"]);
    expect_abort(&env, "exit_4", &run, 4, "network");
}

#[tokio::test]
async fn exit_5_rate_limited() {
    let server = CanvasServer::start().await;
    server
        .override_post_or_get(
            "/api/v1/courses",
            ResponseTemplate::new(429).insert_header("retry-after", "0"),
        )
        .await;
    let env = E2e::with_server(&server);
    let run = env.run(&["sync", "--json"]);
    expect_abort(&env, "exit_5", &run, 5, "rate_limited");
}

#[tokio::test]
async fn exit_6_resolution() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["course", "NOT-A-COURSE", "--json"]);
    expect_abort(&env, "exit_6", &run, 6, "resolution");
}

#[tokio::test]
async fn exit_7_offline_miss() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["courses", "--offline", "--json"]);
    expect_abort(&env, "exit_7", &run, 7, "offline");
}

#[tokio::test]
async fn exit_8_refused() {
    let server = CanvasServer::start().await;
    let mut assignment = crate::harness::Fixtures::assignment();
    assignment["can_submit"] = json!(false);
    assignment["lock_explanation"] = json!("the assignment is closed");
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}"),
            200,
            assignment,
        )
        .await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");
    let run = env.run(&[
        "submit",
        &COURSE_ID.to_string(),
        &ASSIGNMENT_ID.to_string(),
        "--text",
        body.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    expect_abort(&env, "exit_8", &run, 8, "refused");
}

#[tokio::test]
async fn exit_9_submission_recovery() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    // A Canvas-shaped 500 says nothing about whether the attempt landed, and
    // the readback still shows no attempt, so the journal stays unknown.
    server
        .override_post_or_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions"),
            ResponseTemplate::new(500).set_body_json(json!({
                "status": "internal_server_error",
                "message": "unexpected",
                "error_report_id": 4242,
            })),
        )
        .await;
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions/self"),
            200,
            json!({ "id": 60077, "attempt": 0, "submission_history": [] }),
        )
        .await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");
    let run = env.run(&[
        "submit",
        &COURSE_ID.to_string(),
        &ASSIGNMENT_ID.to_string(),
        "--text",
        body.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    run.assert_code(9);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/submit@1");
    assert_eq!(value["result"]["state"], "outcome_unknown");
    env.mask_ids_from_journals();
    env.snapshot_json("exit_9", &run);
}

#[tokio::test]
async fn exit_10_verification_mismatch() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");
    env.run(&[
        "submit",
        &COURSE_ID.to_string(),
        &ASSIGNMENT_ID.to_string(),
        "--text",
        body.to_str().unwrap(),
        "--yes",
    ])
    .assert_code(0);
    let receipt = env.mask_ids_from_journals();

    // Canvas now reports a different body for the same attempt.
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions/self"),
            200,
            json!({
                "id": 60077,
                "attempt": 1,
                "submitted_at": crate::harness::NOW,
                "workflow_state": "submitted",
                "submission_type": "online_text_entry",
                "body": "<p>something else entirely</p>",
                "attachments": [],
                "submission_history": [{
                    "id": 60077,
                    "attempt": 1,
                    "submitted_at": crate::harness::NOW,
                    "workflow_state": "submitted",
                    "submission_type": "online_text_entry",
                    "body": "<p>something else entirely</p>",
                    "attachments": [],
                }],
            }),
        )
        .await;
    let run = env.run(&["submission", "verify", &receipt, "--json"]);
    expect(&env, "exit_10", &run, 10, "canvas-cli/verify@1", "mismatch");
}

#[tokio::test]
async fn exit_11_cancelled() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");
    let run = env.run_on_a_terminal(
        &[
            "submit",
            &COURSE_ID.to_string(),
            &ASSIGNMENT_ID.to_string(),
            "--text",
            body.to_str().unwrap(),
            "--json",
        ],
        "Submit? [y/N]",
        "n\n",
    );
    // Nothing was written, so the cancellation is an abort, not a completed
    // command: it carries `error@1` with `code = "cancelled"`.
    expect_abort(&env, "exit_11", &run, 11, "cancelled");
    assert_eq!(
        run.json()["requests"]["api"],
        1,
        "the plan fetch is the only request a cancelled submit makes"
    );
}

#[tokio::test]
async fn exit_12_partial() {
    let server = CanvasServer::start().await;
    // Canvas hides the folder listing but still serves the files.
    server
        .override_status(&format!("/api/v1/courses/{COURSE_ID}/folders"), 403)
        .await;
    let env = E2e::with_server(&server);
    let run = env.run(&["files", &COURSE_ID.to_string(), "--json"]);
    expect(&env, "exit_12", &run, 12, "canvas-cli/files@1", "partial");
    assert!(
        !run.json()["partial"].as_array().unwrap().is_empty(),
        "a partial outcome names its scopes"
    );
}

/// The other exit-13 cause: the credential store refuses (§14, `Denied`).
///
/// The harness always runs on the file store, so the fake keyring is how a
/// backend that is present and refuses is reached.
#[tokio::test]
async fn exit_13_credential_store_denied() {
    let server = CanvasServer::start().await;
    let env = E2e::new();
    env.track_origin(&server.uri());
    env.run_stdin_local(
        &["auth", "login", "--host", &server.uri(), "--token-stdin"],
        crate::harness::TOKEN,
    )
    .assert_code(0);
    env.track_logged_in_identity();

    // Logout clears both stores; the keyring denies its half.
    let run = env.run_with_keyring_error(&["auth", "logout", "--json"], "denied");
    expect_abort(&env, "exit_13_credential_denied", &run, 13, "local");

    // The refusal is durable: the token is gone and the flag stays set.
    let status = env.run_local(&["auth", "status", "--json"]);
    status.assert_code(0);
    assert_eq!(status.json()["result"]["token_source"], Value::Null);
}

#[tokio::test]
async fn exit_13_local_persistence() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_newer_cache_schema();
    let run = env.run(&["courses", "--json"]);
    expect_abort(&env, "exit_13", &run, 13, "local");
}

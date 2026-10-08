//! Every v1 command in table and `--json` mode (SPEC §5, §16 row 3 item 1).

use crate::harness::{
    ASSIGNMENT_ID, COURSE_ID, CanvasServer, E2e, JOURNAL_PLACEHOLDER, RECEIPT_PLACEHOLDER,
};

/// Snapshot one command in both modes against the fixture server.
///
/// Table mode runs first, so the `--json` run reads the cache the first one
/// filled; both freshness blocks are therefore fixed, not racy.
async fn both_modes(name: &str, args: &[&str]) {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    snapshot_both(&env, name, args, 0);
}

/// Snapshot both modes of `args` in an environment the caller prepared.
fn snapshot_both(env: &E2e, name: &str, args: &[&str], code: i32) {
    let table = env.run(args);
    table.assert_code(code);
    env.snapshot(&format!("{name}_table"), &table);

    let mut json_args = args.to_vec();
    json_args.push("--json");
    let json = env.run(&json_args);
    json.assert_code(code);
    env.snapshot_json(&format!("{name}_json"), &json);
}

/// Snapshot both modes of a command that never opens a session.
fn snapshot_both_local(env: &E2e, name: &str, args: &[&str], code: i32) {
    let table = env.run_local(args);
    table.assert_code(code);
    env.snapshot(&format!("{name}_table"), &table);

    let mut json_args = args.to_vec();
    json_args.push("--json");
    let json = env.run_local(&json_args);
    json.assert_code(code);
    env.snapshot_json(&format!("{name}_json"), &json);
}

// ---------------------------------------------------------------- class A --

#[tokio::test]
async fn version() {
    let env = E2e::new();
    snapshot_both_local(&env, "version", &["version"], 0);
}

#[tokio::test]
async fn doctor() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.set_default_profile("default");
    snapshot_both(&env, "doctor", &["doctor", "--network"], 0);
}

/// Without `--network`, the network checks are reported as `skipped` (§5).
///
/// `doctor` is the one command whose two forms produce different check lists,
/// and the local form is the one a class-B invocation actually runs.
#[tokio::test]
async fn doctor_without_network() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.set_default_profile("default");
    snapshot_both(&env, "doctor_local", &["doctor"], 0);

    let run = env.run(&["doctor", "--json"]);
    let value = run.json();
    let checks = value["result"]["checks"].as_array().expect("checks");
    for name in [
        "network_users_self",
        "network_rate_limit",
        "network_clock_skew",
    ] {
        let check = checks
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("{name} is still reported"));
        assert_eq!(check["status"], "skipped", "{name} without --network");
    }
    assert_eq!(
        value["requests"]["api"], 0,
        "the local form opens no connection"
    );
}

// ------------------------------------------------------------------ auth ---

#[tokio::test]
async fn auth_login_status_token_logout() {
    let server = CanvasServer::start().await;
    let env = E2e::new();
    let host = server.uri();
    env.track_origin(&host);

    let login = env.run_stdin_local(&["auth", "login", "--host", &host, "--token-stdin"], TOKEN);
    login.assert_code(0);
    env.track_logged_in_identity();
    env.snapshot("auth_login_table", &login);

    // A second login on a fresh root gives the `--json` envelope for the same
    // step; logging in twice into one root would report a rebind instead.
    let json_env = E2e::new();
    json_env.track_origin(&host);
    let login_json = json_env.run_stdin_local(
        &["auth", "login", "--host", &host, "--token-stdin", "--json"],
        TOKEN,
    );
    login_json.assert_code(0);
    json_env.track_logged_in_identity();
    json_env.snapshot_json("auth_login_json", &login_json);

    snapshot_both_local(&env, "auth_status", &["auth", "status"], 0);
    snapshot_both_local(&env, "auth_token", &["auth", "token"], 0);
    snapshot_both_local(&env, "auth_logout", &["auth", "logout"], 0);
}

const TOKEN: &str = crate::harness::TOKEN;

// -------------------------------------------------------------- identity ---

#[tokio::test]
async fn identity_list_and_remove() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    snapshot_both_local(&env, "identity_list", &["identity", "list"], 0);

    let key = env.identity_key();
    let removed = env.run_local(&["identity", "remove", &key, "--yes"]);
    removed.assert_code(0);
    env.snapshot("identity_remove_table", &removed);

    let json_env = E2e::with_server(&server);
    let key = json_env.identity_key();
    let removed = json_env.run_local(&["identity", "remove", &key, "--yes", "--json"]);
    removed.assert_code(0);
    json_env.snapshot_json("identity_remove_json", &removed);
}

// ------------------------------------------------------------ cache-backed --

#[tokio::test]
async fn courses() {
    both_modes("courses", &["courses"]).await;
}

#[tokio::test]
async fn course() {
    both_modes("course", &["course", &COURSE_ID.to_string()]).await;
}

#[tokio::test]
async fn todo() {
    both_modes("todo", &["todo"]).await;
}

#[tokio::test]
async fn assignments() {
    both_modes("assignments", &["assignments", &COURSE_ID.to_string()]).await;
}

#[tokio::test]
async fn assignment() {
    both_modes(
        "assignment",
        &[
            "assignment",
            &COURSE_ID.to_string(),
            &ASSIGNMENT_ID.to_string(),
        ],
    )
    .await;
}

#[tokio::test]
async fn submission() {
    both_modes(
        "submission",
        &[
            "submission",
            &COURSE_ID.to_string(),
            &ASSIGNMENT_ID.to_string(),
            "--history",
        ],
    )
    .await;
}

#[tokio::test]
async fn grades() {
    both_modes("grades", &["grades"]).await;
}

#[tokio::test]
async fn files() {
    both_modes("files", &["files", &COURSE_ID.to_string()]).await;
}

#[tokio::test]
async fn modules() {
    both_modes("modules", &["modules", &COURSE_ID.to_string(), "--items"]).await;
}

#[tokio::test]
async fn sync() {
    both_modes("sync", &["sync"]).await;
}

#[tokio::test]
async fn sync_full() {
    both_modes("sync_full", &["sync", "--full"]).await;
}

#[tokio::test]
async fn announcements() {
    both_modes("announcements", &["announcements"]).await;
}

#[tokio::test]
async fn announcement() {
    both_modes(
        "announcement",
        &["announcement", &COURSE_ID.to_string(), "60040"],
    )
    .await;
}

#[tokio::test]
async fn calendar() {
    both_modes("calendar", &["calendar"]).await;
}

#[tokio::test]
async fn download_dry_run() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let dest = env.write_file("downloads/.keep", b"");
    let dest = dest.parent().unwrap().to_str().unwrap().to_owned();
    snapshot_both(
        &env,
        "download",
        &[
            "download",
            &COURSE_ID.to_string(),
            "--dest",
            &dest,
            "--dry-run",
        ],
        0,
    );
}

#[tokio::test]
async fn open_course() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    snapshot_both(&env, "open", &["open", &COURSE_ID.to_string()], 0);
}

/// The three `open` subcommands, which build a URL without resolving a course.
#[tokio::test]
async fn open_subcommands() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let course = COURSE_ID.to_string();
    let assignment = ASSIGNMENT_ID.to_string();
    snapshot_both(
        &env,
        "open_assignment",
        &["open", "assignment", &course, &assignment],
        0,
    );
    snapshot_both(&env, "open_file", &["open", "file", "60501"], 0);
    snapshot_both(
        &env,
        "open_announcement",
        &["open", "announcement", &course, "60040"],
        0,
    );
}

// ------------------------------------------------------------ local state --

#[tokio::test]
async fn cache_stats_path_clear() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.run(&["sync"]).assert_code(0);
    snapshot_both_local(&env, "cache_stats", &["cache", "stats"], 0);
    snapshot_both_local(&env, "cache_path", &["cache", "path"], 0);
    snapshot_both_local(&env, "cache_clear", &["cache", "clear"], 0);
}

#[tokio::test]
async fn config_path_get_set() {
    let env = E2e::new();
    snapshot_both_local(&env, "config_path", &["config", "path"], 0);
    snapshot_both_local(
        &env,
        "config_set",
        &["config", "set", "network.api_concurrency", "3"],
        0,
    );
    snapshot_both_local(
        &env,
        "config_get",
        &["config", "get", "network.api_concurrency"],
        0,
    );
}

#[tokio::test]
async fn alias_set_list_remove() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    snapshot_both_local(
        &env,
        "alias_set",
        &["alias", "set", "chem", &COURSE_ID.to_string()],
        0,
    );
    snapshot_both_local(&env, "alias_list", &["alias", "list"], 0);
    snapshot_both_local(&env, "alias_remove", &["alias", "remove", "chem"], 0);
}

// -------------------------------------------------------- submit lifecycle --

/// `submit`, `receipts`, and `submission verify` over one real attempt.
///
/// They share one test because each step needs the receipt the previous step
/// produced; splitting them would mean re-running `submit` four times.
#[tokio::test]
async fn submit_receipts_and_verify() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");

    let submit = env.run(&[
        "submit",
        &COURSE_ID.to_string(),
        &ASSIGNMENT_ID.to_string(),
        "--text",
        body.to_str().unwrap(),
        "--yes",
    ]);
    submit.assert_code(0);
    let receipt_id = receipt_id_from(&env);
    env.snapshot("submit_table", &submit);

    snapshot_both_local(&env, "receipts_list", &["receipts", "list"], 0);
    snapshot_both_local(&env, "receipts_show", &["receipts", "show", &receipt_id], 0);

    let out = env.scratch_path().join("receipt.json");
    snapshot_both_local(
        &env,
        "receipts_export",
        &[
            "receipts",
            "export",
            &receipt_id,
            "--out",
            out.to_str().unwrap(),
        ],
        0,
    );
    assert!(out.exists(), "export wrote the receipt file");
    let written = std::fs::metadata(&out).unwrap().len();
    let reported = env.run_local(&[
        "receipts",
        "export",
        &receipt_id,
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        reported.json()["result"]["bytes"].as_u64(),
        Some(std::fs::metadata(&out).unwrap().len()),
        "the reported byte count is the file it wrote (was {written})"
    );

    server.echo_posted_submission().await;
    snapshot_both(
        &env,
        "submission_verify",
        &["submission", "verify", &receipt_id],
        0,
    );
}

/// The `--json` half of `submit`, in its own environment.
///
/// One assignment allows two attempts, so the second `submit` in the same
/// environment would report a different attempt number.
#[tokio::test]
async fn submit_json() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    let env = E2e::with_server(&server);
    let body = env.write_file("essay.txt", b"hello\n");
    let submit = env.run(&[
        "submit",
        &COURSE_ID.to_string(),
        &ASSIGNMENT_ID.to_string(),
        "--text",
        body.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    submit.assert_code(0);
    receipt_id_from(&env);
    env.snapshot_json("submit_json", &submit);
}

/// Record the ids one `submit` generated so later snapshots stay stable.
fn receipt_id_from(env: &E2e) -> String {
    let listed = env.run_local(&["receipts", "list", "--json"]);
    listed.assert_code(0);
    let value = listed.json();
    let journals = value["result"]["journals"]
        .as_array()
        .expect("journals is an array");
    let entry = journals.first().expect("submit left one journal");
    let journal_id = entry["journal_id"]
        .as_str()
        .expect("journal_id is a string")
        .to_owned();
    env.mask(&journal_id, JOURNAL_PLACEHOLDER);
    let receipt_id = entry["receipt_id"]
        .as_str()
        .expect("receipt_id is a string")
        .to_owned();
    env.mask(&receipt_id, RECEIPT_PLACEHOLDER);
    receipt_id
}

/// `submission reconcile` and `receipts acknowledge`, over an unknown journal.
///
/// Both commands only accept a journal whose `POST` outcome was never
/// observed, so the test has to create one: the server answers the submission
/// `POST` with a Canvas-shaped 500 and then shows no attempt.
#[tokio::test]
async fn reconcile_and_acknowledge_an_unknown_journal() {
    let server = CanvasServer::start().await;
    server.allow_text_submission().await;
    server
        .override_post_or_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions"),
            wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
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
            serde_json::json!({ "id": 60077, "attempt": 0, "submission_history": [] }),
        )
        .await;
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
    .assert_code(9);

    let listed = env.run_local(&["receipts", "list", "--json"]);
    listed.assert_code(0);
    let journal_id = listed.json()["result"]["journals"][0]["journal_id"]
        .as_str()
        .expect("submit left one journal")
        .to_owned();
    env.mask(&journal_id, JOURNAL_PLACEHOLDER);

    // Nothing new is visible, so the journal stays unknown: outcome `recovery`.
    snapshot_both(
        &env,
        "submission_reconcile",
        &["submission", "reconcile", &journal_id],
        9,
    );

    // `acknowledge` retires the pending flag and changes nothing else. Each
    // call restamps `acknowledged_at`, so both instants are masked before the
    // run that printed them is snapshotted.
    let json = env.run_local(&["receipts", "acknowledge", &journal_id, "--json"]);
    json.assert_code(0);
    let stamped = json.json()["result"]["acknowledged_at"]
        .as_str()
        .expect("acknowledge records an instant")
        .to_owned();
    env.mask(&stamped, "<clock>");
    env.snapshot_json("receipts_acknowledge_json", &json);

    let acknowledged = env.run_local(&["receipts", "acknowledge", &journal_id]);
    acknowledged.assert_code(0);
    // Each call restamps, so mask the instant this one wrote as well.
    let listed = env.run_local(&["receipts", "list", "--json"]);
    listed.assert_code(0);
    let restamped = listed.json()["result"]["journals"][0]["acknowledged_at"]
        .as_str()
        .expect("the journal is acknowledged")
        .to_owned();
    env.mask(&restamped, "<clock>");
    env.snapshot("receipts_acknowledge_table", &acknowledged);
}

//! Raw-output commands and the `--json` usage error each one raises (§7, §14).
//!
//! A raw-output command writes bytes another program consumes: a completion
//! script, a receipt document, an iCalendar stream, the token itself. There is
//! no envelope to wrap them in, so `--json` is a usage error (exit 2) at every
//! level of the command tree.

use crate::harness::{ASSIGNMENT_ID, COURSE_ID, CanvasServer, E2e};

/// Assert `--json` is refused on a raw-output command and snapshot the error.
fn refuses_json(env: &E2e, name: &str, args: &[&str]) {
    let mut json_args = args.to_vec();
    json_args.push("--json");
    let refused = env.run_local(&json_args);
    refused.assert_code(2);
    assert!(
        refused.stdout.is_empty(),
        "a usage error writes nothing to stdout: {:?}",
        refused.stdout
    );
    env.snapshot(&format!("{name}_json_refused"), &refused);
}

#[tokio::test]
async fn completions() {
    let env = E2e::new();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let run = env.run_local(&["completions", shell]);
        run.assert_code(0);
        assert!(!run.stdout.is_empty(), "{shell} completions are not empty");
        assert!(run.stderr.is_empty(), "{shell} completions are quiet");
    }
    // One shell carries the snapshot; the rest are covered by `dist.rs`, which
    // compares every script with the copy the release archives ship.
    let bash = env.run_local(&["completions", "bash"]);
    env.snapshot("completions_bash_first_lines", &head(&bash, 12));
    refuses_json(&env, "completions", &["completions", "bash"]);
}

/// Keep the first `lines` lines of a run's stdout.
///
/// A completion script is thousands of lines of generated shell; the snapshot
/// only needs to prove the command emitted one, unwrapped.
fn head(run: &crate::harness::Run, lines: usize) -> crate::harness::Run {
    let mut stdout = String::new();
    for line in run.stdout.lines().take(lines) {
        stdout.push_str(line);
        stdout.push('\n');
    }
    stdout.push_str("…\n");
    crate::harness::Run {
        code: run.code,
        stdout,
        stderr: run.stderr.clone(),
    }
}

#[tokio::test]
async fn receipts_export_to_stdout() {
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

    let listed = env.run_local(&["receipts", "list", "--json"]);
    listed.assert_code(0);
    let value = listed.json();
    let entry = &value["result"]["journals"][0];
    let receipt_id = entry["receipt_id"].as_str().expect("a receipt").to_owned();
    env.mask(&receipt_id, "00000000-0000-4000-8000-00000000000r");
    if let Some(journal) = entry["journal_id"].as_str() {
        env.mask(journal, "00000000-0000-4000-8000-00000000000j");
    }

    let exported = env.run_local(&["receipts", "export", &receipt_id, "--out", "-"]);
    exported.assert_code(0);
    let document: serde_json::Value =
        serde_json::from_str(&exported.stdout).expect("the export is one JSON document");
    assert!(
        document.get("schema").is_none() && document.get("envelope").is_none(),
        "the export is the receipt itself, not an envelope"
    );
    env.snapshot("receipts_export_stdout", &exported);

    refuses_json(
        &env,
        "receipts_export_stdout",
        &["receipts", "export", &receipt_id, "--out", "-"],
    );
}

#[tokio::test]
async fn auth_token_reveal() {
    let server = CanvasServer::start().await;
    let env = E2e::new();
    env.track_origin(&server.uri());
    // `--reveal` reads the credential store, so the token has to be stored:
    // seeding the credential row alone would leave nothing to print.
    env.run_stdin_local(
        &["auth", "login", "--host", &server.uri(), "--token-stdin"],
        crate::harness::TOKEN,
    )
    .assert_code(0);
    env.track_logged_in_identity();
    let revealed = env.run_local(&["auth", "token", "--reveal"]);
    revealed.assert_code(0);
    assert_eq!(
        revealed.stdout.trim(),
        crate::harness::TOKEN,
        "--reveal prints the secret and nothing else"
    );
    env.snapshot("auth_token_reveal", &revealed);
    refuses_json(&env, "auth_token_reveal", &["auth", "token", "--reveal"]);
    let _ = &server;
}

#[tokio::test]
async fn config_edit_without_a_terminal() {
    let env = E2e::new();
    // `config edit` hands the file to `$EDITOR`; with no TTY it refuses before
    // it starts one, which is the only deterministic end-to-end outcome.
    let edited = env.run_local(&["config", "edit"]);
    edited.assert_code(2);
    env.snapshot("config_edit_no_tty", &edited);
    refuses_json(&env, "config_edit", &["config", "edit"]);
}

/// `calendar --ics -` streams an iCalendar document to stdout.
#[tokio::test]
async fn calendar_ics_to_stdout() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let stream = env.run_raw(&["calendar", "--ics", "-"]);
    stream.assert_code(0);
    let body = stream.stdout.replace("\r\n", "\n");
    assert!(
        body.starts_with("BEGIN:VCALENDAR\n") && body.trim_end().ends_with("END:VCALENDAR"),
        "the stream is one iCalendar document: {body}"
    );
    assert!(
        stream.stdout.contains("\r\n"),
        "RFC 5545 folds on CRLF, which the snapshot cannot show"
    );
    env.snapshot("calendar_ics_stdout", &stream);

    refuses_json(&env, "calendar_ics", &["calendar", "--ics", "-"]);
}

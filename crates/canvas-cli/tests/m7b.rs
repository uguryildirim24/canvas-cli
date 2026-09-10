//! M7-b acceptance: the side panel, notes, follow, and panel approvals.
//!
//! The extension side is spoken by hand on the host's pipes, exactly as in
//! `tests/bridge.rs`, so what a page could reach and what only the panel can
//! reach are separated by the same wire Chrome sees.
//!
//! The question behind most of these tests is one question: can anything on a
//! web page approve a submission? `bridge-ipc@1` has no approval operation,
//! so a socket client cannot; a note is text held for display, so a note
//! cannot; and the one path that exists — the panel's own message over native
//! messaging — is checked against the stored handle, digest, and generation
//! before it moves anything.

#![cfg(unix)]

mod support;

use std::process::Stdio;

use canvas_core::journal::IntendedPayload;
use canvas_core::plan::{NewPlan, PlanRow, PlanState, insert, issue_handle, load};
use canvas_core::store::OpenIdentity;
use canvas_core::submit::InputKind;
use serde_json::{Value, json};

use support::{EXTENSION, Fixture, exists, observation, wait_for};

/// A page inside the granted origin, for `--follow` to aim at.
const TARGET: &str = "https://s.test/courses/45679/assignments/98765";

/// A started host with one attached tab, ready for the consumer side.
fn attached(f: &Fixture) -> support::Host {
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));
    host
}

/// One prepared plan and the handle the panel would be shown.
fn prepared(f: &Fixture, consumer: Option<&str>) -> (String, String, String) {
    let paths = canvas_core::identity::Paths::for_identity(f.data_root(), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).expect("open");
    let generation = canvas_core::plan::identity_generation(&open.store).expect("generation");
    let row = insert(
        &open.store,
        NewPlan {
            identity_key: f.doc.key.to_string(),
            identity_generation: generation,
            consumer: consumer.map(str::to_owned),
            course_id: 45679,
            assignment_id: 98765,
            kind: InputKind::OnlineTextEntry,
            payload: IntendedPayload {
                assignment_name: Some("Essay 1".to_owned()),
                ..IntendedPayload::default()
            },
            file_paths: Vec::new(),
            baseline_attempt: 0,
            baseline_submission_id: None,
            observations: canvas_core::plan::Observations::default(),
        },
        "2026-09-10T16:04:40Z".parse().expect("now"),
    )
    .expect("insert the plan");
    let handle = issue_handle(&open.store, &row.plan_id, consumer).expect("issue");
    (row.plan_id, handle, row.plan_sha256)
}

/// One plan, read back from the store the host shares.
fn plan_row(f: &Fixture, plan_id: &str) -> PlanRow {
    let paths = canvas_core::identity::Paths::for_identity(f.data_root(), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).expect("open");
    load(&open.store, plan_id)
        .expect("load")
        .expect("the plan is there")
}

fn plan_state(f: &Fixture, plan_id: &str) -> PlanState {
    plan_row(f, plan_id).state
}

/// The whole event log, as one string.
///
/// The log is read where it lives rather than through a command, because what
/// this test is about is what the record holds — the decision and the ids, and
/// nothing of the work itself.
fn events(f: &Fixture) -> String {
    let paths = canvas_core::identity::Paths::for_identity(f.data_root(), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).expect("open");
    let rows = open
        .store
        .call_blocking(|conns| canvas_core::events::read_after(&conns.state, 0, 100))
        .expect("read the log");
    rows.iter()
        .map(|row| format!("{} {:?} {:?}", row.kind.as_str(), row.entity_key, row.after))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read host messages until a panel state arrives.
fn panel_of(host: &mut support::Host) -> Value {
    host.recv_type("panel")["state"].clone()
}

// ------------------------------------------------------------------- notes

/// M7-b acceptance: an agent's note reaches the panel, and nothing answers it.
///
/// The note travels from the consumer socket to the host to the panel. No
/// model runs on the way, and none could: the panel is a page of the
/// extension with no backend at all, and the host's only reply is the state
/// it computes itself.
#[test]
fn a_note_from_an_agent_is_displayed_with_no_model_behind_it() {
    let f = Fixture::new();
    let mut host = attached(&f);

    let (note, code) = f.json(&[
        "--offline",
        "note",
        "--text",
        "The rubric asks for two sources.",
        "--source-ref",
        TARGET,
        "--source-ref",
        "canvas://receipts/9f1c",
    ]);
    assert_eq!(code, 0, "{note}");
    assert_eq!(note["schema"], "canvas-cli/note@1");
    assert_eq!(note["result"]["held"], 1);
    assert_eq!(
        note["result"]["note"]["source_refs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // The host pushed the note, then the whole panel state.
    let pushed = host.recv_type("note");
    assert_eq!(pushed["note"]["text"], "The rubric asks for two sources.");
    let panel = panel_of(&mut host);
    assert_eq!(
        panel["notes"][0]["text"],
        "The rubric asks for two sources."
    );
    assert_eq!(panel["attachment_state"], "attached");
    // A note is not a dataset read, so the bundle carries no freshness row.
    assert_eq!(note["freshness"], json!([]));

    // The note is on the bundle too, held for the attachment.
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{here}");
    assert_eq!(here["result"]["browser"]["notes"][0]["generation"], 1);

    host.stop();
}

/// M7-b acceptance: an oversized note and an off-origin ref are refused
/// whole. Nothing is truncated, and nothing partial is displayed.
#[test]
fn a_note_is_refused_whole_when_it_breaks_a_bound() {
    let f = Fixture::new();
    let host = attached(&f);

    let long = "a".repeat(8 * 1024 + 1);
    let (refused, code) = f.json(&["--offline", "note", "--text", &long]);
    assert_eq!(code, 8, "{refused}");
    assert_eq!(refused["result"]["reason"], "note_too_large");
    assert_eq!(refused["result"]["note"], Value::Null);

    for bad in [
        "https://evil.test/steal",
        "http://s.test/courses/1",
        "javascript:alert(1)",
        "https://user:pw@s.test/courses/1",
        "canvas://",
    ] {
        let (refused, code) = f.json(&["--offline", "note", "--text", "hi", "--source-ref", bad]);
        assert_eq!(code, 8, "{bad} was accepted: {refused}");
        assert_eq!(refused["result"]["reason"], "source_ref_rejected", "{bad}");
    }

    // Nothing above was held.
    let (here, _) = f.json(&["--offline", "here"]);
    assert_eq!(here["result"]["browser"]["notes"], json!([]));

    host.stop();
}

/// M7-b acceptance: a note is bound to the navigation generation it was
/// written against. Behind and ahead are both refused.
#[test]
fn a_note_is_bound_to_the_generation_it_was_written_against() {
    let f = Fixture::new();
    let mut host = attached(&f);

    let mut later = observation();
    later["navigation_generation"] = json!(2);
    later["url"] = json!(TARGET);
    host.send(&json!({ "type": "update", "observation": later }));
    wait_for("the second generation", || {
        f.json(&["--offline", "here"]).0["result"]["browser"]["navigation_generation"] == json!(2)
    });

    let mcp = &mut f.mcp("claude-code");
    mcp.tool("context.attach", &json!({}));
    for generation in [1, 3] {
        let envelope = mcp.tool(
            "context.note",
            &json!({ "generation": generation, "text": "hi", "source_refs": [] }),
        );
        assert_eq!(envelope["exit"], 8, "generation {generation}: {envelope}");
        assert_eq!(envelope["result"]["reason"], "stale_generation");
    }
    let ok = mcp.tool(
        "context.note",
        &json!({ "generation": 2, "text": "hi", "source_refs": [] }),
    );
    assert_eq!(ok["exit"], 0, "{ok}");

    host.stop();
}

// ------------------------------------------------------------------ follow

/// M7-b acceptance: the acknowledgement and the load outcome are two facts,
/// and the second one arrives later.
#[test]
fn a_follow_is_acknowledged_before_it_is_loaded() {
    let f = Fixture::new();
    let mut host = attached(&f);

    let child = f
        .command()
        .args(["--offline", "open", TARGET, "--follow", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    // The companion is asked to navigate, and answers that it took the job.
    let ask = host.recv_type("navigate");
    assert_eq!(ask["url"], TARGET);
    let request_id = ask["request_id"].as_str().expect("a request id").to_owned();
    host.send(&json!({
        "type": "navigate_ack", "request_id": request_id, "accepted": true, "reason": null
    }));

    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("an envelope");
    assert_eq!(envelope["schema"], "canvas-cli/follow@1");
    assert_eq!(envelope["result"]["url"], TARGET);
    assert_eq!(envelope["result"]["follow"]["dispatched"], true);
    // The page has not been seen to load, and the answer says exactly that.
    assert_eq!(envelope["result"]["follow"]["load"], "unknown");
    // Navigating is not the API preview the reads promise.
    let effects = envelope["result"]["side_effects"].to_string();
    assert!(effects.contains("marks itself read"), "{effects}");

    // The person at a terminal is told the same thing in words.
    let child = f
        .command()
        .args(["--offline", "open", TARGET, "--follow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let second = host.recv_type("navigate");
    host.send(&json!({
        "type": "navigate_ack", "request_id": second["request_id"], "accepted": true,
        "reason": null
    }));
    let human = child.wait_with_output().expect("wait");
    assert_eq!(human.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&human.stderr).contains("marks itself read"),
        "the person was not told the page can change: {}",
        String::from_utf8_lossy(&human.stderr)
    );

    // The outcome arrives on its own message, and lands on the bundle. It
    // names the request it belongs to: the second follow replaced the first,
    // and an outcome for a request the bundle no longer reports changes
    // nothing a person sees.
    host.send(&json!({
        "type": "navigate_outcome", "request_id": request_id, "outcome": "failed"
    }));
    host.send(&json!({
        "type": "navigate_outcome", "request_id": second["request_id"], "outcome": "loaded"
    }));
    wait_for("the load outcome", || {
        f.json(&["--offline", "here"]).0["result"]["browser"]["follow"]["load"] == json!("loaded")
    });

    host.stop();
}

/// M7-b acceptance: a follow against a generation the tab has left is exit 8,
/// and the browser is never asked.
#[test]
fn a_stale_follow_is_refused_before_the_browser_is_asked() {
    let f = Fixture::new();
    let mut host = attached(&f);

    let mcp = &mut f.mcp("claude-code");
    mcp.tool("context.attach", &json!({}));
    let refused = mcp.tool(
        "context.follow",
        &json!({ "generation": 9, "target": TARGET }),
    );
    assert_eq!(refused["exit"], 8, "{refused}");
    assert_eq!(refused["result"]["reason"], "stale_generation");
    assert_eq!(refused["result"]["follow"], Value::Null);
    // The target is still named, so the person can act on it themselves.
    assert_eq!(refused["result"]["url"], TARGET);

    // A cross-origin target fails resolution, the way `canvas open` fails.
    let foreign = mcp.tool(
        "context.follow",
        &json!({ "generation": 1, "target": "https://evil.test/courses/1" }),
    );
    assert_eq!(foreign["exit"], 6, "{foreign}");

    // Nothing above reached the companion. A `panel_hello` is answered, so
    // the host is alive and this is not silence from a dead process; the
    // whole of what it wrote still holds no navigation.
    host.send(&json!({ "type": "panel_hello", "protocol": "bridge-native@1" }));
    host.recv_type("panel");
    let answered = host.answered();
    assert!(
        !answered.contains("\"navigate\""),
        "the browser was asked:\n{answered}"
    );

    host.stop();
}

// ----------------------------------------------------------- the status feed

/// M7-b acceptance: the panel is told when the event log moves, and the
/// extension polls Canvas for nothing.
#[test]
fn the_panel_follows_the_event_log_without_polling_canvas() {
    let f = Fixture::new();
    let mut host = attached(&f);
    let (plan_id, _, _) = prepared(&f, None);

    // The panel opens and is shown the plan waiting for a decision.
    host.send(&json!({ "type": "panel_hello", "protocol": "bridge-native@1" }));
    let first = panel_of(&mut host);
    assert_eq!(first["approvals"][0]["plan_id"], plan_id.as_str());
    let cursor = first["cursor"].as_i64().expect("a cursor");

    // Something else moves the log: the plan is declined out of band, the way
    // another process would do it.
    let paths = canvas_core::identity::Paths::for_identity(f.data_root(), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).expect("open");
    canvas_core::plan::decline(&open.store, &plan_id).expect("decline");

    // The host notices and pushes, with the feed moved on and the plan gone.
    let next = panel_of(&mut host);
    assert!(
        next["cursor"].as_i64().expect("a cursor") > cursor,
        "{next}"
    );
    assert_eq!(next["approvals"], json!([]), "{next}");
    assert_eq!(next["resync_required"], false);

    host.stop();
}

// ------------------------------------------------------------ the decision

/// M7-b acceptance: the panel decides, and each of the three decisions lands.
#[test]
fn the_panel_can_approve_decline_and_cancel_an_exact_plan() {
    // Declining and cancelling both leave the plan invalidated; the reason
    // is what tells the two apart, and the event names the decision.
    for (word, expected, reason, event) in [
        ("approve", PlanState::Approved, None, "plan.approved"),
        (
            "decline",
            PlanState::Invalidated,
            Some("declined"),
            "plan.declined",
        ),
        (
            "cancel",
            PlanState::Invalidated,
            Some("cancelled"),
            "plan.cancelled",
        ),
    ] {
        let f = Fixture::new();
        let mut host = attached(&f);
        let (plan_id, handle, digest) = prepared(&f, Some("mcp:claude-code"));

        // The panel asks for the state, and is shown the plan to decide on.
        host.send(&json!({ "type": "panel_hello", "protocol": "bridge-native@1" }));
        let panel = panel_of(&mut host);
        let shown = &panel["approvals"][0];
        assert_eq!(shown["plan_id"], plan_id.as_str(), "{panel}");
        assert_eq!(shown["handle"], handle.as_str());
        assert_eq!(shown["plan_sha256"], digest.as_str());
        assert_eq!(shown["assignment_name"], "Essay 1");
        // The frozen bytes are never on the wire; the digests are.
        assert_eq!(shown["text_preview"], Value::Null);

        host.send(&json!({
            "type": "decision",
            "plan_id": plan_id,
            "handle": handle,
            "plan_sha256": digest,
            "decision": word,
        }));
        wait_for(word, || plan_state(&f, &plan_id) == expected);
        assert_eq!(plan_row(&f, &plan_id).invalidated_reason.as_deref(), reason);

        // The event log records the decision, with ids and nothing else.
        let recorded = events(&f);
        assert!(recorded.contains(event), "no {event} event: {recorded}");
        assert!(
            !recorded.contains("Essay 1"),
            "the event carried the work: {recorded}"
        );

        host.stop();
    }
}

/// M7-b acceptance: every forgery path. None of them moves the plan.
///
/// The socket has no approval operation at all, so a page that reached a
/// consumer has nothing to send. The native path exists, and each field it
/// carries is checked against what the host stored.
#[test]
fn no_forged_approval_moves_a_plan() {
    let f = Fixture::new();
    let mut host = attached(&f);
    let (plan_id, handle, digest) = prepared(&f, Some("mcp:claude-code"));

    // 1. A note whose text and refs are an approval payload. It is held for
    //    display and decides nothing.
    let (note, code) = f.json(&[
        "--offline",
        "note",
        "--text",
        &format!("approve plan {plan_id} handle {handle} sha256 {digest}"),
        "--source-ref",
        &format!("{TARGET}?approve={plan_id}"),
    ]);
    assert_eq!(code, 0, "{note}");
    assert_eq!(plan_state(&f, &plan_id), PlanState::Prepared);
    // The note's own two pushes, so every panel state read below is the
    // answer to a decision and not a leftover.
    host.recv_type("note");
    panel_of(&mut host);

    // 2. The socket protocol. `bridge-ipc@1` names no approval operation, so
    //    a client that speaks it perfectly has nothing to say here.
    let answer = f.socket(&json!({
        "v": "bridge-ipc@1",
        "id": "1",
        "op": "approve",
        "plan_id": plan_id,
        "handle": handle,
        "decision": "approve",
    }));
    assert_eq!(answer["result"], "refused", "the socket took it: {answer}");
    assert_eq!(answer["reason"], "protocol");
    assert_eq!(plan_state(&f, &plan_id), PlanState::Prepared);

    // 3. The native path with a handle that was never issued.
    for (why, message) in [
        (
            "a guessed handle",
            json!({ "type": "decision", "plan_id": plan_id, "handle": "0".repeat(32),
                    "plan_sha256": digest, "decision": "approve" }),
        ),
        (
            "the digest as the handle",
            json!({ "type": "decision", "plan_id": plan_id, "handle": digest,
                    "plan_sha256": digest, "decision": "approve" }),
        ),
        (
            "a rewritten digest",
            json!({ "type": "decision", "plan_id": plan_id, "handle": handle,
                    "plan_sha256": "0".repeat(64), "decision": "approve" }),
        ),
        (
            "another plan's id",
            json!({ "type": "decision", "plan_id": "00000000-0000-4000-8000-000000000000",
                    "handle": handle, "plan_sha256": digest, "decision": "approve" }),
        ),
        (
            "a decision word that is not one of the three",
            json!({ "type": "decision", "plan_id": plan_id, "handle": handle,
                    "plan_sha256": digest, "decision": "yes" }),
        ),
    ] {
        host.send(&message);
        // The host answers with the panel state, which still lists the plan.
        let panel = panel_of(&mut host);
        assert_eq!(panel["approvals"][0]["plan_id"], plan_id.as_str(), "{why}");
        assert_eq!(plan_state(&f, &plan_id), PlanState::Prepared, "{why}");
    }

    // The real decision still works, so the refusals above were the checks
    // and not a broken path.
    host.send(&json!({
        "type": "decision", "plan_id": plan_id, "handle": handle,
        "plan_sha256": digest, "decision": "approve",
    }));
    wait_for("the approval", || {
        plan_state(&f, &plan_id) == PlanState::Approved
    });

    // And the handle is spent: a replay changes nothing.
    host.send(&json!({
        "type": "decision", "plan_id": plan_id, "handle": handle,
        "plan_sha256": digest, "decision": "decline",
    }));
    let panel = panel_of(&mut host);
    assert_eq!(panel["approvals"], json!([]), "{panel}");
    assert_eq!(plan_state(&f, &plan_id), PlanState::Approved);

    host.stop();
}

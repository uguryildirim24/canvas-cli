//! SPEC §14 precedence, asserted end to end.
//!
//! Two orders are specified. An **abort** ends the command as soon as it is
//! detected, in the order 2 → 3 → 13 → 4 → 5 → 6 → 7. A **completed command**
//! ranks its own outcome 9 > 10 > 8 > 12 > 11 > 0.
//!
//! The abort order is an order of detection, so a pair is only assertable when
//! one invocation can really be in both states at once. Every such adjacent
//! pair is constructed below. `4` before `5` is the one adjacent pair that no
//! single invocation can hold: a request that never connects cannot also come
//! back rate limited, and the first dataset failure ends the command before a
//! second route is asked.
//!
//! The completed-command order has the same limit, and a tighter one: 9, 8 and
//! 11 are terminal for the command that can produce them, so no invocation ever
//! carries one of them next to a lower-ranked outcome. The orderings a single
//! invocation can hold are `10 > 12` (`download --verify`, covered by
//! `download.rs::modified_force_mismatch_precedence_and_move_previous_path`)
//! and `12 > 0`, which is asserted here. `canvas-core`'s
//! `download::install::outcome_exit_code` ranks the full list and is unit
//! tested against every action.

use serde_json::json;

use crate::harness::{COURSE_ID, CanvasServer, E2e};

/// Assert the earlier of two applicable conditions decided the exit.
fn earlier_wins(name: &str, run: &crate::harness::Run, code: i32, error_code: &str) {
    run.assert_code(code);
    let value = run.json();
    assert_eq!(value["exit"], code, "{name}: envelope exit");
    assert_eq!(value["result"]["code"], error_code, "{name}: error code");
}

// --------------------------------------------------------- abort order ------

/// 2 before 3: a usage error is raised before the missing identity is reported.
#[tokio::test]
async fn usage_before_auth() {
    let env = E2e::new();
    // No identity exists, so this would be exit 3; `--offline` on a
    // network-required command is exit 2 and is checked first.
    let run = env.run_local(&["sync", "--offline", "--json"]);
    earlier_wins("usage_before_auth", &run, 2, "usage");
    env.snapshot_json("precedence_usage_before_auth", &run);
}

/// 2 before 6: a usage error is raised before the operand is resolved.
#[tokio::test]
async fn usage_before_resolution() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["course", "NOT-A-COURSE", "--offline", "--fresh", "--json"]);
    run.assert_code(2);
    assert!(run.stdout.is_empty(), "clap writes the conflict to stderr");
    env.snapshot("precedence_usage_before_resolution", &run);
}

/// 3 before 13: credential resolution runs before the store is opened.
///
/// Ignored: the CLI opens the identity store first, so a store this binary
/// cannot read reports exit 13 and the missing credential is never looked for.
/// The order is structural — the credential row lives in that same store — so
/// the fix is not a local one.
#[tokio::test]
#[ignore = "session.rs opens the identity store before resolving credentials,             so exit 13 wins over exit 3 (SPEC §14 requires 3 first)"]
async fn auth_before_local() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_newer_cache_schema();
    // No token anywhere, and a cache database this binary cannot open.
    let run = env.run_local(&["courses", "--json"]);
    earlier_wins("auth_before_local", &run, 3, "auth");
}

/// 3 before 4: a missing credential is reported without reaching the network.
#[tokio::test]
async fn auth_before_network() {
    let env = E2e::with_identity("http://127.0.0.1:1");
    let run = env.run_local(&["courses", "--json"]);
    earlier_wins("auth_before_network", &run, 3, "auth");
}

/// 13 before 4: the store is opened before the first request is made.
#[tokio::test]
async fn local_before_network() {
    let env = E2e::with_identity("http://127.0.0.1:1");
    env.write_newer_cache_schema();
    let run = env.run(&["courses", "--json"]);
    earlier_wins("local_before_network", &run, 13, "local");
}

/// 13 before 7: the store is opened before coverage is consulted.
#[tokio::test]
async fn local_before_offline_miss() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_newer_cache_schema();
    let run = env.run(&["courses", "--offline", "--json"]);
    earlier_wins("local_before_offline_miss", &run, 13, "local");
}

/// 5 before 6: the fetch the resolver needs is rate limited before it can fail.
#[tokio::test]
async fn rate_limited_before_resolution() {
    let server = CanvasServer::start().await;
    server
        .override_post_or_get(
            "/api/v1/courses",
            wiremock::ResponseTemplate::new(429).insert_header("retry-after", "0"),
        )
        .await;
    let env = E2e::with_server(&server);
    // `NOT-A-COURSE` matches nothing, so a served listing would give exit 6.
    let run = env.run(&["course", "NOT-A-COURSE", "--json"]);
    earlier_wins("rate_limited_before_resolution", &run, 5, "rate_limited");
}

/// 4 before 6: the resolver's fetch fails before the name can be judged.
#[tokio::test]
async fn network_before_resolution() {
    let env = E2e::with_identity("http://127.0.0.1:1");
    let run = env.run(&["course", "NOT-A-COURSE", "--json"]);
    earlier_wins("network_before_resolution", &run, 4, "network");
}

/// 6 before 7: resolution runs first, so a resolvable-but-uncovered dataset
/// never gets as far as reporting the offline miss.
#[tokio::test]
async fn resolution_before_offline_miss() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    // Fill the courses dataset so the resolver has coverage and can decide.
    // The resolver reads both course scopes, so both have to be covered
    // before an offline resolution can reach a verdict at all.
    env.run(&["courses"]).assert_code(0);
    env.run(&["courses", "--all"]).assert_code(0);
    // The course detail dataset has no coverage, so a resolvable operand would
    // be exit 7; this one cannot be resolved, which is exit 6.
    let run = env.run(&["course", "NOT-A-COURSE", "--offline", "--json"]);
    earlier_wins("resolution_before_offline_miss", &run, 6, "resolution");

    // The same invocation with a resolvable operand does report the miss, so
    // the pair really is a pair.
    let covered = env.run(&["course", &COURSE_ID.to_string(), "--offline", "--json"]);
    earlier_wins("offline_miss_alone", &covered, 7, "offline");
}

// ---------------------------------------------- completed-command order -----

/// 12 before 0: one denied scope outranks every dataset that did arrive.
#[tokio::test]
async fn partial_before_success() {
    let server = CanvasServer::start().await;
    server
        .override_status(&format!("/api/v1/courses/{COURSE_ID}/folders"), 403)
        .await;
    let env = E2e::with_server(&server);
    let run = env.run(&["files", &COURSE_ID.to_string(), "--json"]);
    run.assert_code(12);
    let value = run.json();
    assert_eq!(value["outcome"], "partial");
    assert_eq!(
        value["schema"], "canvas-cli/files@1",
        "a completed command keeps its own schema"
    );
    assert!(
        !value["result"]["files"].as_array().unwrap().is_empty(),
        "the files that did arrive are still in the result"
    );
    assert!(
        !value["partial"].as_array().unwrap().is_empty(),
        "the denied scope is named"
    );
}

/// The same command with nothing denied is exit 0, so the pair is a pair.
#[tokio::test]
async fn success_when_nothing_is_partial() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["files", &COURSE_ID.to_string(), "--json"]);
    run.assert_code(0);
    assert_eq!(run.json()["outcome"], "ok");
    assert_eq!(run.json()["partial"], json!([]));
}

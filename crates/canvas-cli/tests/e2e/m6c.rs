//! M6-c: the shared coordinator, the event stream, and `notify`.
//!
//! These tests exercise the parts of REPORT §3.6 that need a real process:
//! cross-process request permits, refresh single-flight, foreground priority,
//! and the `watch`/`notify` contracts. Everything that fits inside one process
//! — the lock-name encoding, the outbox replay, the comparison rules — lives in
//! `canvas-core::coord::tests` and `canvas-core::events::tests`.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use canvas_core::coord::{CoordConfig, Coordinator, InterestKind};
use serde_json::{Value, json};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::harness::{ASSIGNMENT_ID, COURSE_ID, CanvasServer, E2e, Fixtures, NOW, Run, TOKEN};

/// An hour after [`NOW`], so a second tick writes a new `fetched_at`.
///
/// The observation id embeds the cache row's `fetched_at`, which makes a
/// re-run idempotent. A test that wants a second observation therefore has to
/// move the frozen clock, exactly as real time would.
const LATER: &str = "2026-09-09T18:05:12Z";

/// Every TTL at zero, so each tick refreshes and the test controls the pace.
const NO_TTL: &str = "\
[cache]
ttl_courses = \"0m\"
ttl_grades = \"0m\"
ttl_assignments = \"0m\"
ttl_missing = \"0m\"
ttl_planner = \"0m\"
ttl_announcements = \"0m\"
";

/// Run `canvas watch` with the fixture token and the given extra variables.
fn watch(env: &E2e, args: &[&str], vars: &[(&str, &str)]) -> Run {
    let mut all: Vec<(&str, &str)> = vec![("CANVAS_TOKEN", TOKEN)];
    all.extend_from_slice(vars);
    env.run_env(args, &all)
}

/// The `event@1` documents and the closing `watch@1` envelope of one run.
fn stream(run: &Run) -> (Vec<Value>, Value) {
    let mut documents: Vec<Value> = run
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("stream line is not one JSON document ({e}): {line:?}"))
        })
        .collect();
    let summary = documents.pop().expect("the stream ends with its summary");
    assert_eq!(summary["schema"], "canvas-cli/watch@1");
    (documents, summary)
}

#[test]
fn watch_refuses_json_because_the_stream_is_its_own_contract() {
    let env = E2e::new();
    let run = env.run_local(&["watch", "--json"]);
    run.assert_code(2);
    assert!(
        run.stderr.contains("--json cannot be used with watch"),
        "stderr={}",
        run.stderr
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_tick_is_silent_and_the_next_one_streams_what_changed() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);

    // The first complete observation of a scope only sets the baseline, so the
    // whole stream is the closing `watch@1` document: one envelope, which the
    // schema suite then checks against the registry fixture.
    let first = watch(&env, &["watch", "--jsonl", "--once"], &[]);
    first.assert_code(0);
    env.snapshot_json("m6c_watch_once", &first);
    let (events, summary) = stream(&first);
    assert!(events.is_empty(), "the first tick emitted {events:?}");
    assert_eq!(summary["result"]["events"], json!(0));
    assert!(
        summary["result"]["datasets"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "the first tick refreshed nothing: {summary}"
    );

    // Move the due date; the next complete observation reports exactly that.
    let mut assignment = Fixtures::assignment();
    assignment["due_at"] = json!("2026-09-30T03:59:00Z");
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments"),
            200,
            json!([assignment]),
        )
        .await;

    let second = watch(
        &env,
        &["watch", "--jsonl", "--once"],
        &[("CANVAS_NOW", LATER)],
    );
    second.assert_code(0);
    let (events, summary) = stream(&second);
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or_default())
        .collect();
    assert!(
        kinds.contains(&"due.changed"),
        "the changed due date was not reported: {kinds:?}"
    );
    for event in &events {
        assert_eq!(event["schema"], "canvas-cli/event@1");
        assert!(event["cursor"].is_string(), "a cursor is a string id (§7)");
        assert!(
            event["observed_at_local"].is_string(),
            "every ts carries its local sibling (§7)"
        );
    }
    assert_eq!(
        summary["result"]["events"],
        json!(u64::try_from(events.len()).unwrap())
    );
    env.mask(
        &env.identity.as_ref().unwrap().generation.to_string(),
        "<generation>",
    );
    env.snapshot("m6c_watch_stream", &second);
    assert_eq!(
        summary["result"]["cursor"],
        events.last().unwrap()["cursor"]
    );
    assert_eq!(summary["exit"], json!(0));

    // Replay is at least once: the same cursor replays the same documents.
    let replay = watch(&env, &["watch", "--jsonl", "--once", "--since", "0"], &[]);
    replay.assert_code(0);
    let (replayed, _) = stream(&replay);
    assert!(
        replayed.len() >= events.len(),
        "replay dropped events: {} < {}",
        replayed.len(),
        events.len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_cursor_this_log_cannot_replay_asks_for_a_resync_and_exits_zero() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);

    // Nothing this log ever issued: the consumer must rebuild its baseline.
    let run = watch(
        &env,
        &["watch", "--jsonl", "--once", "--since", "999999"],
        &[],
    );
    run.assert_code(0);
    let (events, summary) = stream(&run);
    assert_eq!(events.len(), 1, "exactly one gap is reported: {events:?}");
    assert_eq!(events[0]["kind"], json!("resync_required"));
    assert_eq!(summary["result"]["resync_required"], json!(true));
    assert_eq!(summary["result"]["ticks"], json!(0));
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_outcome_never_stops_polling_but_an_active_one_does() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);

    // A terminal `outcome_unknown` journal is not in flight (§12.2): watch
    // keeps polling, which is what its readback depends on.
    seed_journal(&env, "outcome_unknown");
    let run = watch(&env, &["watch", "--jsonl", "--once"], &[]);
    run.assert_code(0);
    let (_, summary) = stream(&run);
    assert_eq!(summary["result"]["skipped"], Value::Null);
    assert!(
        summary["result"]["datasets"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "an unknown outcome stopped polling: {summary}"
    );

    // A journal that is still posting is the §10 pending hook.
    seed_journal(&env, "posting");
    let run = watch(&env, &["watch", "--jsonl", "--once"], &[]);
    run.assert_code(0);
    let (_, summary) = stream(&run);
    assert_eq!(summary["result"]["skipped"], json!("journal_in_flight"));
    assert_eq!(summary["result"]["datasets"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn foreground_interest_stops_polling_and_polling_resumes_when_it_ends() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);
    let coord = Coordinator::open(&env.paths(), CoordConfig::default().with_concurrency(1, 1))
        .expect("the coordinator opens");

    let interest = coord
        .register_interest(InterestKind::Submit, ASSIGNMENT_ID)
        .expect("interest is recorded")
        .expect("nothing else holds this assignment");
    let run = watch(&env, &["watch", "--jsonl", "--once"], &[]);
    run.assert_code(0);
    let (_, summary) = stream(&run);
    assert_eq!(summary["result"]["skipped"], json!("foreground_interest"));
    assert_eq!(
        summary["requests"]["api"],
        json!(0),
        "watch made a request while a submission waited: {summary}"
    );

    // Priority is bounded: polling resumes as soon as the interest is gone.
    drop(interest);
    let run = watch(&env, &["watch", "--jsonl", "--once"], &[]);
    run.assert_code(0);
    let (_, summary) = stream(&run);
    assert_eq!(summary["result"]["skipped"], Value::Null);
    assert!(
        summary["result"]["datasets"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "polling was starved: {summary}"
    );
    assert!(
        summary["requests"]["api"].as_u64().unwrap_or_default() > 0,
        "polling never reached the network: {summary}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn notify_posts_one_line_per_kind_group_and_never_repeats_a_cursor() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);

    watch(&env, &["watch", "--jsonl", "--once"], &[]).assert_code(0);
    let mut assignment = Fixtures::assignment();
    assignment["due_at"] = json!("2026-09-30T03:59:00Z");
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments"),
            200,
            json!([assignment]),
        )
        .await;
    watch(
        &env,
        &["watch", "--jsonl", "--once"],
        &[("CANVAS_NOW", LATER)],
    )
    .assert_code(0);

    let first = env.run_local(&["notify", "--stdout"]);
    first.assert_code(0);
    let lines: Vec<&str> = first.stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.iter().any(|line| line.starts_with("assignments:")),
        "no assignments notification: {lines:?}"
    );
    for line in &lines {
        assert!(line.contains("cursor"), "a group names its cursor: {line}");
    }

    // Deduplication is by cursor: the position is durable, so nothing repeats.
    let second = env.run_local(&["notify", "--stdout"]);
    second.assert_code(0);
    assert_eq!(
        second.stdout, "",
        "notify repeated itself: {}",
        second.stdout
    );

    // `--since` overrides the stored position for one run, and only that run.
    let replay = env.run_local(&["notify", "--stdout", "--since", "0"]);
    replay.assert_code(0);
    assert_eq!(replay.stdout, first.stdout);

    // Without a backend the command still says where the lines went.
    let plain = env.run_local(&["notify", "--since", "0"]);
    plain.assert_code(0);
    assert!(
        plain.stderr.contains("no desktop notification backend"),
        "stderr={}",
        plain.stderr
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sigint_closes_the_stream_cleanly_with_the_cursor_durable() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    env.write_config(NO_TTL);

    // Put one event in the log, so the resident run has something to replay.
    watch(&env, &["watch", "--jsonl", "--once"], &[]).assert_code(0);
    let mut assignment = Fixtures::assignment();
    assignment["due_at"] = json!("2026-09-30T03:59:00Z");
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments"),
            200,
            json!([assignment]),
        )
        .await;
    watch(
        &env,
        &["watch", "--jsonl", "--once"],
        &[("CANVAS_NOW", LATER)],
    )
    .assert_code(0);

    let out = env.scratch_path().join("watch-stream.jsonl");
    let mut command: Command = env.command();
    command
        .args(["watch", "--jsonl", "--since", "0", "--color", "never"])
        .env("CANVAS_TOKEN", TOKEN)
        .env("CANVAS_TEST_WATCH_TICK_MS", "100")
        .stdout(std::fs::File::create(&out).expect("the stream file"))
        .stderr(Stdio::null());
    let mut child = command.spawn().expect("watch starts");

    // Wait for the replay to reach the file, so the interrupt lands on a
    // running stream rather than on start-up.
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::fs::read_to_string(&out)
        .unwrap_or_default()
        .lines()
        .count()
        < 1
    {
        assert!(Instant::now() < deadline, "the replay never reached stdout");
        std::thread::sleep(Duration::from_millis(20));
    }

    let signalled = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(signalled.success(), "SIGINT was not delivered");
    let status = child.wait().expect("watch exits");
    assert_eq!(status.code(), Some(0), "SIGINT did not close cleanly");

    let text = std::fs::read_to_string(&out).expect("the stream file");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let first: Value = serde_json::from_str(lines[0]).expect("the replayed event");
    let last: Value = serde_json::from_str(lines[lines.len() - 1]).expect("the summary");
    assert_eq!(first["schema"], json!("canvas-cli/event@1"));
    assert_eq!(last["schema"], json!("canvas-cli/watch@1"));
    assert_eq!(
        last["result"]["cursor"], first["cursor"],
        "the closing summary lost the cursor it streamed"
    );
}

#[test]
fn notify_refuses_json_like_every_other_raw_output_command() {
    let env = E2e::new();
    let run = env.run_local(&["notify", "--json"]);
    run.assert_code(2);
}

// --- cross-process permits and single-flight -------------------------------

/// A Canvas that records when each request arrived and answers slowly.
///
/// Two arrivals less than the delay apart overlapped in flight, so the peak of
/// those overlaps is the server's own concurrency count: it needs no
/// cooperation from the client under test.
#[derive(Clone)]
struct Slow {
    arrivals: Arc<Mutex<Vec<(String, Instant)>>>,
    delay: Duration,
    /// The one route that answers slowly, or every route when absent.
    only: Option<String>,
}

impl Respond for Slow {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let route = request.url.path().to_owned();
        self.arrivals
            .lock()
            .expect("arrivals")
            .push((route.clone(), Instant::now()));
        let delay = match &self.only {
            Some(only) if *only != route => Duration::ZERO,
            _ => self.delay,
        };
        ResponseTemplate::new(200)
            .set_body_json(body_for(&route))
            .set_delay(delay)
    }
}

impl Slow {
    /// The largest number of requests that were in flight at one moment.
    fn peak(&self) -> usize {
        let arrivals = self.arrivals.lock().expect("arrivals");
        arrivals
            .iter()
            .map(|(_, at)| {
                arrivals
                    .iter()
                    .filter(|(_, other)| other <= at && *at < *other + self.delay)
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    /// How many requests reached one route.
    fn hits(&self, route: &str) -> usize {
        self.arrivals
            .lock()
            .expect("arrivals")
            .iter()
            .filter(|(seen, _)| seen == route)
            .count()
    }

    /// Block until `route` has been hit `count` times.
    ///
    /// A second process must start while the first still holds the
    /// single-flight lock. Waiting for the holder's own request to arrive is
    /// the exact moment it does, so the test never depends on process startup
    /// beating a sleep.
    fn wait_for_hits(&self, route: &str, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.hits(route) < count {
            assert!(
                Instant::now() < deadline,
                "{route} was hit {} times, not {count}",
                self.hits(route)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// The route the single-flight tests contend on.
const COURSES: &str = "/api/v1/courses";

/// The fixture body one route answers with; an unknown route is an empty page.
fn body_for(route: &str) -> Value {
    let assignments = format!("/api/v1/courses/{COURSE_ID}/assignments");
    let mut course = Fixtures::course();
    course["term"] = Fixtures::term();
    match route {
        "/api/v1/users/self" => Fixtures::user(),
        "/api/v1/courses" => json!([course]),
        "/api/v1/planner/items" => json!([Fixtures::planner()]),
        "/api/v1/users/self/missing_submissions" => json!([Fixtures::missing()]),
        "/api/v1/users/self/enrollments" => json!([Fixtures::enrollment()]),
        "/api/v1/announcements" => json!([Fixtures::announcement()]),
        route if route == assignments => json!([Fixtures::assignment()]),
        _ => json!([]),
    }
}

/// Start a slow Canvas and the environment bound to it.
async fn slow_canvas(delay: Duration, only: Option<&str>) -> (MockServer, Slow, E2e) {
    let server = MockServer::start().await;
    let slow = Slow {
        arrivals: Arc::new(Mutex::new(Vec::new())),
        delay,
        only: only.map(str::to_owned),
    };
    Mock::given(any())
        .respond_with(slow.clone())
        .mount(&server)
        .await;
    let origin = server.uri().trim_end_matches('/').to_owned();
    let env = E2e::with_identity(&origin);
    env.write_config(NO_TTL);
    (server, slow, env)
}

#[tokio::test(flavor = "current_thread")]
async fn one_api_slot_bounds_a_cli_command_and_a_watch_together() {
    let (_server, slow, env) = slow_canvas(Duration::from_millis(150), None).await;

    // One slot for the whole identity, held across both processes.
    let vars = [
        ("CANVAS_TEST_API_CONCURRENCY", "1"),
        ("CANVAS_TOKEN", TOKEN),
    ];
    let mut watcher = spawn(&env, &["watch", "--jsonl", "--once"], &vars);
    let mut reader = spawn(&env, &["todo", "--json", "--fresh"], &vars);
    let watcher = watcher.wait().expect("watch exits");
    let reader = reader.wait().expect("todo exits");
    assert!(watcher.success(), "watch exited {watcher:?}");
    assert!(reader.success(), "todo exited {reader:?}");

    assert!(
        slow.peak() <= 1,
        "{} requests were in flight at once with one slot",
        slow.peak()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn interest_that_arrives_during_a_tick_stops_the_rest_of_it() {
    // Only `courses` is slow, so the tick is caught with one request in flight
    // and every later refresh would be immediate if nothing stopped it.
    let (_server, slow, env) = slow_canvas(Duration::from_millis(800), Some(COURSES)).await;
    let vars = [
        ("CANVAS_TEST_API_CONCURRENCY", "1"),
        ("CANVAS_TOKEN", TOKEN),
    ];

    let watcher = spawn_piped(&env, &["watch", "--jsonl", "--once"], &vars);
    // The tick has already passed its own start-of-tick check and is waiting
    // on the network. This is the moment §3.6 is about.
    slow.wait_for_hits(COURSES, 1);
    let coord = Coordinator::open(&env.paths(), CoordConfig::default().with_concurrency(1, 1))
        .expect("the coordinator opens");
    let interest = coord
        .register_interest(InterestKind::Submit, ASSIGNMENT_ID)
        .expect("interest is recorded")
        .expect("nothing else holds this assignment");

    let output = watcher.wait_with_output().expect("watch exits");
    drop(interest);
    assert!(output.status.success(), "watch exited {:?}", output.status);
    let run = Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::new(),
    };
    let (_, summary) = stream(&run);

    assert_eq!(
        summary["result"]["skipped"],
        json!("foreground_interest"),
        "watch kept polling after a submission registered interest: {summary}"
    );
    // The request already in flight finished; nothing after it was admitted.
    let refreshed: Vec<&str> = summary["result"]["datasets"]
        .as_array()
        .expect("datasets is an array")
        .iter()
        .filter_map(|row| row["dataset"].as_str())
        .collect();
    assert!(
        refreshed.iter().all(|d| *d == "courses"),
        "watch admitted {refreshed:?} while a submission waited: {summary}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn two_processes_refreshing_one_scope_fetch_it_once() {
    let (_server, slow, env) = slow_canvas(Duration::from_secs(3), Some(COURSES)).await;

    // Four slots, so nothing but the single-flight lock can serialize these.
    let vars = [
        ("CANVAS_TEST_API_CONCURRENCY", "4"),
        ("CANVAS_TOKEN", TOKEN),
    ];
    let mut first = spawn(&env, &["courses", "--json", "--fresh"], &vars);
    slow.wait_for_hits(COURSES, 1);
    let mut second = spawn(&env, &["courses", "--json", "--fresh"], &vars);
    assert!(first.wait().expect("first exits").success());
    assert!(second.wait().expect("second exits").success());

    assert_eq!(
        slow.hits(COURSES),
        1,
        "the second process refetched a scope the first was already fetching"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_waiter_that_times_out_serves_what_the_cache_has() {
    let (_server, slow, env) = slow_canvas(Duration::from_secs(3), Some(COURSES)).await;
    let vars = [
        ("CANVAS_TEST_API_CONCURRENCY", "4"),
        ("CANVAS_TOKEN", TOKEN),
    ];

    // Give the cache one complete row to fall back to.
    env.run_env(&["courses", "--json", "--fresh"], &vars)
        .assert_code(0);
    let before = slow.hits(COURSES);

    // The waiter gives up long before the holder finishes, and answers from
    // the cache with honest §7 metadata instead of a second fetch.
    let mut holder = spawn(&env, &["courses", "--json", "--fresh"], &vars);
    slow.wait_for_hits(COURSES, before + 1);
    let waiter = env.run_env(
        &["courses", "--json", "--fresh"],
        &[
            ("CANVAS_TEST_API_CONCURRENCY", "4"),
            ("CANVAS_TOKEN", TOKEN),
            ("CANVAS_TEST_REFRESH_WAIT_MS", "50"),
        ],
    );
    waiter.assert_code(0);
    assert!(holder.wait().expect("holder exits").success());

    let envelope = waiter.json();
    assert_eq!(envelope["schema"], json!("canvas-cli/courses@1"));
    assert_eq!(
        envelope["freshness"][0]["source"],
        json!("cache"),
        "the waiter claimed a fetch it never made: {envelope}"
    );
    assert_eq!(
        slow.hits(COURSES) - before,
        1,
        "the waiter fetched the scope the holder was already fetching"
    );
}

/// Spawn `canvas` with its stdout captured, so the caller can read the stream.
fn spawn_piped(env: &E2e, args: &[&str], vars: &[(&str, &str)]) -> std::process::Child {
    let mut command: Command = env.command();
    command.args(args).args(["--color", "never"]);
    for (key, value) in vars {
        command.env(key, value);
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("canvas starts")
}

/// Spawn `canvas` without waiting for it.
fn spawn(env: &E2e, args: &[&str], vars: &[(&str, &str)]) -> std::process::Child {
    let mut command: Command = env.command();
    command.args(args).args(["--color", "never"]);
    for (key, value) in vars {
        command.env(key, value);
    }
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("canvas starts")
}

/// Write one journal row in `state`, as §12.2 records it.
fn seed_journal(env: &E2e, state: &str) {
    let open = env.open();
    let state = state.to_owned();
    open.store
        .call_blocking(move |conns| {
            conns.state.execute(
                "INSERT INTO submission_journal
                    (journal_id, identity_key, course_id, assignment_id, kind,
                     state, created_at)
                 VALUES (?1, 'k', ?2, ?3, 'online_text_entry', ?4, ?5)",
                rusqlite::params![
                    format!("journal-{state}"),
                    COURSE_ID,
                    ASSIGNMENT_ID,
                    state,
                    NOW
                ],
            )?;
            Ok(())
        })
        .expect("the journal row is written");
}

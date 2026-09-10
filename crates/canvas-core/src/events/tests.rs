//! Observation, baseline, retention, and crash-replay tests (REPORT §3.6).

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use jiff::{Span, Timestamp};
use rusqlite::params;

use super::{CursorCheck, EventKind, apply_pending, check_cursor, expire, observe_refresh};
use crate::identity::{IdentityDocument, Paths};
use crate::store::{OpenIdentity, Store};
use crate::test_scratch::Scratch;

const SCOPE: &str = "course:1";

/// One assignment row as the cache holds it.
#[derive(Clone, Copy)]
struct Row {
    id: i64,
    name: &'static str,
    due_at: Option<&'static str>,
    score: Option<f64>,
    posted_at: Option<&'static str>,
}

fn row(id: i64, name: &'static str) -> Row {
    Row {
        id,
        name,
        due_at: Some("2026-09-20T03:59:00Z"),
        score: None,
        posted_at: None,
    }
}

fn open(scratch: &Scratch) -> (Paths, Store) {
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(scratch.as_ref(), &doc.key);
    std::fs::create_dir_all(&paths.identity_dir).unwrap();
    std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    (paths, store)
}

/// Write a complete `assignments` coverage for `course:1`.
fn seed(store: &Store, rows: &[Row], fetched_at: &str) {
    let rows = rows.to_vec();
    let fetched_at = fetched_at.to_owned();
    store
        .call_blocking(move |conns| {
            let tx = conns
                .cache
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "DELETE FROM membership WHERE dataset = 'assignments' AND scope = ?1",
                [SCOPE],
            )?;
            for (position, row) in rows.iter().enumerate() {
                let data = serde_json::json!({ "posted_at": row.posted_at }).to_string();
                tx.execute(
                    "INSERT INTO assignments (id, course_id, name, due_at, score, data_json)
                     VALUES (?1, 1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name, due_at = excluded.due_at,
                        score = excluded.score, data_json = excluded.data_json",
                    params![row.id, row.name, row.due_at, row.score, data],
                )?;
                tx.execute(
                    "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                     VALUES ('assignments', ?1, 'assignment', ?2, ?3)",
                    params![SCOPE, row.id.to_string(), i64::try_from(position).unwrap()],
                )?;
            }
            tx.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale)
                 VALUES ('assignments', ?1, ?2, 1, ?3, 0)
                 ON CONFLICT(dataset, scope) DO UPDATE SET
                    fetched_at = excluded.fetched_at, complete = 1, stale = 0,
                    count = excluded.count, error = NULL",
                params![SCOPE, fetched_at, i64::try_from(rows.len()).unwrap()],
            )?;
            tx.commit()?;
            Ok(())
        })
        .unwrap();
}

/// Mark the coverage stale, as a failed page does.
fn mark_stale(store: &Store) {
    store
        .call_blocking(|conns| {
            conns.cache.execute(
                "UPDATE fetch_log SET stale = 1, complete = 0, error = 'network'
                 WHERE dataset = 'assignments' AND scope = ?1",
                [SCOPE],
            )?;
            Ok(())
        })
        .unwrap();
}

fn events(store: &Store) -> Vec<(String, Option<String>)> {
    store
        .call_blocking(|conns| {
            let mut stmt = conns
                .state
                .prepare("SELECT kind, entity_key FROM events ORDER BY cursor ASC")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

// ------------------------------------------------------------- baselines

#[test]
fn the_first_complete_observation_is_silent_and_the_next_one_reports_the_difference() {
    let scratch = Scratch::new("events-baseline");
    let (_paths, store) = open(&scratch);
    let runtime = runtime();
    seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T17:00:00Z");
    let first = runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    assert!(
        first.baseline,
        "the first observation must set the baseline"
    );
    assert_eq!(first.events, 0, "the first observation must be silent");
    assert!(events(&store).is_empty());

    // One added, one removed, one due change, one grade published.
    seed(
        &store,
        &[
            Row {
                due_at: Some("2026-09-21T03:59:00Z"),
                score: Some(9.0),
                posted_at: Some("2026-09-10T00:00:00Z"),
                ..row(1, "Problem Set 1")
            },
            row(2, "Problem Set 2"),
        ],
        "2026-09-09T18:00:00Z",
    );
    let second = runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    assert!(!second.baseline);
    let kinds = events(&store);
    assert_eq!(
        kinds,
        vec![
            (EventKind::DueChanged.to_string(), Some("1".to_owned())),
            (EventKind::GradePosted.to_string(), Some("1".to_owned())),
            (EventKind::AssignmentAdded.to_string(), Some("2".to_owned())),
        ],
        "unexpected event sequence"
    );
    assert_eq!(second.events, kinds.len());

    // Assignment 2 disappears from a complete membership: one removal.
    seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T19:00:00Z");
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    let last = events(&store).pop().unwrap();
    assert_eq!(
        last,
        (
            EventKind::AssignmentRemoved.to_string(),
            Some("2".to_owned())
        )
    );
}

#[test]
fn a_partial_page_emits_no_removal() {
    let scratch = Scratch::new("events-partial");
    let (_paths, store) = open(&scratch);
    let runtime = runtime();
    seed(
        &store,
        &[row(1, "Problem Set 1"), row(2, "Problem Set 2")],
        "2026-09-09T17:00:00Z",
    );
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    assert!(events(&store).is_empty());

    // A later page fails: the coverage is marked stale and incomplete, and the
    // membership is whatever survived. Nothing may be reported as removed.
    mark_stale(&store);
    let outcome = runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    assert_eq!(outcome.events, 0);
    assert!(!outcome.resync);
    assert!(events(&store).is_empty(), "a failed page implied a removal");
}

#[test]
fn a_grade_without_publication_evidence_is_grade_changed() {
    let scratch = Scratch::new("events-grade");
    let (_paths, store) = open(&scratch);
    let runtime = runtime();
    seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T17:00:00Z");
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    seed(
        &store,
        &[Row {
            score: Some(7.5),
            ..row(1, "Problem Set 1")
        }],
        "2026-09-09T18:00:00Z",
    );
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    assert_eq!(
        events(&store),
        vec![(EventKind::GradeChanged.to_string(), Some("1".to_owned()))]
    );
}

// --------------------------------------------------------------- cursors

#[test]
fn retention_expires_rows_without_reusing_their_cursors() {
    let scratch = Scratch::new("events-retention");
    let (_paths, store) = open(&scratch);
    let runtime = runtime();
    seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T17:00:00Z");
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    seed(
        &store,
        &[row(1, "Problem Set 1"), row(2, "Problem Set 2")],
        "2026-09-09T18:00:00Z",
    );
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    let first_cursor = store
        .call_blocking(|conns| {
            Ok(conns
                .state
                .query_row("SELECT MIN(cursor) FROM events", [], |r| r.get::<_, i64>(0))?)
        })
        .unwrap();
    assert_eq!(first_cursor, 1);

    // Thirty-one days later the row is outside the retention window.
    let removed = store
        .call_blocking(|conns| {
            expire(
                &mut conns.state,
                "2026-09-09T17:00:00Z"
                    .parse::<Timestamp>()
                    .unwrap()
                    .checked_add(Span::new().hours(31 * 24))
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(removed, 1);

    // The next event takes cursor 2, never the expired 1.
    seed(
        &store,
        &[
            row(1, "Problem Set 1"),
            row(2, "Problem Set 2"),
            row(3, "Problem Set 3"),
        ],
        "2026-09-09T19:00:00Z",
    );
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    let cursors: Vec<i64> = store
        .call_blocking(|conns| {
            let mut stmt = conns
                .state
                .prepare("SELECT cursor FROM events ORDER BY cursor")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(cursors, vec![2], "a deleted cursor was handed out again");

    // A consumer that stopped at the expired cursor must resynchronize.
    let checks = store
        .call_blocking(|conns| {
            let generation = super::identity(&conns.state)?.generation;
            Ok((
                check_cursor(&conns.state, 1, &generation)?,
                check_cursor(&conns.state, 2, &generation)?,
                check_cursor(&conns.state, 99, &generation)?,
                check_cursor(&conns.state, 0, &generation)?,
            ))
        })
        .unwrap();
    assert_eq!(checks.0, CursorCheck::Replay, "cursor 1 is still adjacent");
    assert_eq!(checks.1, CursorCheck::Replay);
    assert_eq!(checks.2, CursorCheck::Resync, "a cursor beyond the log");
    assert_eq!(checks.3, CursorCheck::Replay);
}

/// A consumer position moves forward on its own, and only a resync replaces
/// it: a cursor the log cannot replay is not a position at all.
#[test]
fn a_consumer_position_only_moves_forward_until_a_resync_replaces_it() {
    let scratch = Scratch::new("events-consumer-cursor");
    let (_paths, store) = open(&scratch);
    store
        .call_blocking(|conns| {
            assert_eq!(super::consumer_cursor(&conns.state, "notify")?, 0);
            super::set_consumer_cursor(&mut conns.state, "notify", 7)?;
            assert_eq!(super::consumer_cursor(&conns.state, "notify")?, 7);
            // Backwards is refused: an event already reported stays reported.
            super::set_consumer_cursor(&mut conns.state, "notify", 3)?;
            assert_eq!(super::consumer_cursor(&conns.state, "notify")?, 7);
            // A resync replaces it, so the same gap is reported once.
            super::reset_consumer_cursor(&mut conns.state, "notify", 3)?;
            assert_eq!(super::consumer_cursor(&conns.state, "notify")?, 3);
            // Consumers are independent.
            assert_eq!(super::consumer_cursor(&conns.state, "mcp:host")?, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_cursor_from_another_generation_asks_for_a_resync() {
    let scratch = Scratch::new("events-generation");
    let (_paths, store) = open(&scratch);
    let runtime = runtime();
    seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T17:00:00Z");
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    seed(
        &store,
        &[row(1, "Problem Set 1"), row(2, "Problem Set 2")],
        "2026-09-09T18:00:00Z",
    );
    runtime
        .block_on(observe_refresh(&store, "assignments", SCOPE))
        .unwrap();
    // The log carries a cursor this identity generation never issued.
    let checks = store
        .call_blocking(|conns| {
            conns
                .state
                .execute("UPDATE events SET generation = 'another-generation'", [])?;
            let generation = super::identity(&conns.state)?.generation;
            Ok((
                check_cursor(&conns.state, 1, &generation)?,
                check_cursor(&conns.state, 0, &generation)?,
            ))
        })
        .unwrap();
    assert_eq!(checks.0, CursorCheck::Resync, "a foreign cursor replayed");
    assert_eq!(checks.1, CursorCheck::Resync, "a foreign log replayed");
}

// ------------------------------------------------------- journal events

#[test]
fn a_journal_transition_writes_its_event_in_the_same_transaction() {
    use crate::journal::{AdmissionLock, CreateOpts, State, TransitionPatch, create, transition};
    let scratch = Scratch::new("events-journal");
    let (paths, store) = open(&scratch);
    let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 42).unwrap();
    let (jid, owner) = create(
        &store,
        &paths.identity_dir,
        &admission,
        &CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: 42,
            kind: "online_upload".into(),
            intended_payload_json: r#"{"files":[{"name":"a","size":1,"sha256":"abc"}]}"#.into(),
            baseline_attempt: Some(0),
            baseline_submission_id: None,
        },
    )
    .unwrap();
    drop(admission);
    transition(
        &store,
        &owner,
        &jid,
        State::Planned,
        State::Uploading,
        TransitionPatch::default(),
    )
    .unwrap();
    let rows = store
        .call_blocking(|conns| {
            let mut stmt = conns
                .state
                .prepare("SELECT kind, dataset, scope, [after] FROM events ORDER BY cursor ASC")?;
            let out = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            Ok(out.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (kind, dataset, scope, _) in &rows {
        assert_eq!(kind, EventKind::SubmissionState.as_str());
        assert_eq!(dataset, "submission_journal");
        assert_eq!(scope, "assignment:42");
    }
    assert!(rows[0].3.contains("planned"));
    assert!(rows[1].3.contains("uploading"));
}

// ------------------------------------------------- crash across the handoff

#[test]
fn a_kill_across_the_cache_and_state_handoff_replays_or_asks_for_a_resync() {
    for (mode, overwrite, want) in [
        // Killed after the cache commit, before the observation: the baseline
        // never moved, so the next refresh reports the same difference.
        ("after_cache", false, Expect::Replay),
        // Killed after the observation, before the comparison: the pending row
        // is applied on restart.
        ("after_observation", false, Expect::Replay),
        // The same, but the cache row was refreshed again in the meantime, so
        // the gap is real and is named.
        ("after_observation", true, Expect::Resync),
    ] {
        let scratch = Scratch::new("events-crash");
        let (paths, store) = open(&scratch);
        let runtime = runtime();
        seed(&store, &[row(1, "Problem Set 1")], "2026-09-09T17:00:00Z");
        runtime
            .block_on(observe_refresh(&store, "assignments", SCOPE))
            .unwrap();
        assert!(events(&store).is_empty());
        drop(store);

        let mut child = Process(
            helper_command(paths.data_root.as_path(), mode)
                .spawn()
                .unwrap(),
        );
        wait_for(&paths.data_root.join("ready"));
        child.0.kill().unwrap();
        child.0.wait().unwrap();

        let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
        let store = OpenIdentity::open(&paths, &doc).unwrap().store;
        if overwrite {
            seed(
                &store,
                &[row(1, "Problem Set 1"), row(3, "Problem Set 3")],
                "2026-09-09T19:00:00Z",
            );
        }
        runtime.block_on(apply_pending(&store)).unwrap();
        runtime
            .block_on(observe_refresh(&store, "assignments", SCOPE))
            .unwrap();
        let kinds: Vec<String> = events(&store).into_iter().map(|(k, _)| k).collect();
        match want {
            Expect::Replay => {
                assert!(
                    kinds.contains(&EventKind::AssignmentAdded.to_string()),
                    "{mode}/{overwrite}: the change was lost: {kinds:?}"
                );
                assert!(
                    !kinds.contains(&EventKind::ResyncRequired.to_string()),
                    "{mode}/{overwrite}: an avoidable gap was reported: {kinds:?}"
                );
            }
            Expect::Resync => {
                assert_eq!(
                    kinds
                        .iter()
                        .filter(|k| *k == EventKind::ResyncRequired.as_str())
                        .count(),
                    1,
                    "{mode}/{overwrite}: expected exactly one gap: {kinds:?}"
                );
            }
        }
        // Whatever happened, the log never stays silent about it.
        assert!(!kinds.is_empty(), "{mode}/{overwrite}: a silent gap");
    }
}

enum Expect {
    Replay,
    Resync,
}

/// The child half of the crash test: it stops at a named phase and parks.
#[test]
fn helper() {
    let Ok(root) = std::env::var("CANVAS_EVENTS_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(&root, &doc.key);
    let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    // The refresh this stands in for committed a second assignment.
    seed(
        &store,
        &[row(1, "Problem Set 1"), row(2, "Problem Set 2")],
        "2026-09-09T18:00:00Z",
    );
    if std::env::var("CANVAS_EVENTS_MODE").as_deref() == Ok("after_observation") {
        runtime()
            .block_on(super::record_observation(&store, "assignments", SCOPE))
            .unwrap()
            .expect("the observation is recorded");
    }
    publish(&root.join("ready"), "ready");
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn helper_command(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "events::tests::helper", "--nocapture"])
        .env("CANVAS_EVENTS_ROOT", root)
        .env("CANVAS_EVENTS_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn publish(path: &Path, contents: &str) {
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, contents).unwrap();
    std::fs::rename(&temporary, path).unwrap();
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

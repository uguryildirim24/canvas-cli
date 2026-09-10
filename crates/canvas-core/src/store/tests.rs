//! Store and identity integration tests (SPEC §16 store part).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use jiff::Timestamp;
use rusqlite::params;
use tempfile::TempDir;

use crate::identity::{
    IdentityDocument, IdentityError, IdentityKey, IdentityLock, Paths, RemovalCallbacks,
    remove_identity,
};
use crate::store::dataset::FieldWrite;
use crate::store::{
    Dataset, DbError, FakeEntity, FakeEntityPage, FakeEntityRow, FieldGroup, IngestOpts,
    LookupQuery, LookupResult, OpenIdentity, Store, Supplied, bump_epochs, cache_clear,
    cache_stats, field_observed_at, lookup, pending_for_assignment, read_scope_epoch,
    upsert_enrollment_grades,
};

fn ts(secs: i64) -> Timestamp {
    Timestamp::from_second(secs).unwrap()
}

fn ingest_err(e: impl std::fmt::Display) -> DbError {
    DbError::Message(e.to_string())
}

fn ingest_ok(epoch_seen: i64) -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}

fn setup_identity(dir: &TempDir) -> (Paths, IdentityDocument) {
    let data_root = dir.path().to_path_buf();
    let doc = IdentityDocument::new(
        "https://lasell.instructure.com",
        12345,
        "2026-01-01T00:00:00Z",
    );
    let paths = Paths::for_identity(&data_root, &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    (paths, doc)
}

#[test]
fn identity_key_ipv6_example() {
    let k = IdentityKey::compute("https://[::1]:8443", 7);
    assert!(
        k.as_str().starts_with("___1__8443-7-"),
        "got {}",
        k.as_str()
    );
    assert_eq!(k.as_str().len(), "___1__8443-7-".len() + 8);
}

#[test]
fn two_processes_open_same_identity() {
    if let Ok(data) = std::env::var("CANVAS_TEST_TWO_PROCESS_DATA") {
        let marker = PathBuf::from(std::env::var("CANVAS_TEST_TWO_PROCESS_MARKER").unwrap());
        let key = IdentityKey::compute("https://lasell.instructure.com", 12345);
        let paths = Paths::for_identity(data, &key);
        let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
        let _open = OpenIdentity::open(&paths, &doc).expect("child open");
        fs::write(&marker, b"ok").unwrap();
        thread::sleep(Duration::from_millis(300));
        return;
    }

    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let marker = dir.path().join("child_ok");
    // Parent opens first so migrations finish before the child attaches.
    let open = OpenIdentity::open(&paths, &doc).expect("parent open");
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(&exe)
        .args([
            "--exact",
            "--nocapture",
            "store::tests::two_processes_open_same_identity",
        ])
        .env("CANVAS_TEST_TWO_PROCESS_DATA", dir.path())
        .env("CANVAS_TEST_TWO_PROCESS_MARKER", &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    for _ in 0..200 {
        if marker.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    if !marker.exists() {
        let _ = child.kill();
        let out = child.wait_with_output().unwrap();
        panic!(
            "child did not open identity\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    drop(open);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn interrupted_multipage_refresh_leaves_old_rows() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    let ds = FakeEntity::new("course:1");

    open.store
        .call_blocking({
            let page = FakeEntityPage {
                fetched_at: ts(100),
                rows: vec![FakeEntityRow {
                    id: 1,
                    name: Supplied::Value("Old".into()),
                    due_at: Supplied::Value("2026-01-01T00:00:00Z".into()),
                    ..Default::default()
                }],
            };
            let ds = ds.clone();
            move |conns| {
                ds.ingest(&[page.to_ingest()], &ingest_ok(0), conns)
                    .map_err(ingest_err)?;
                Ok(())
            }
        })
        .unwrap();

    open.store
        .call_blocking(|conns| {
            let tx = conns
                .cache
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE fetch_log SET stale = 1, error = ?1 WHERE dataset = ?2 AND scope = ?3",
                params!["page 2 failed", "fake", "course:1"],
            )?;
            tx.commit()?;
            let name: String =
                conns
                    .cache
                    .query_row("SELECT name FROM fake_entities WHERE id = 1", [], |r| {
                        r.get(0)
                    })?;
            assert_eq!(name, "Old");
            let count: i64 = conns.cache.query_row(
                "SELECT COUNT(*) FROM membership WHERE dataset = 'fake'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn epoch_abort_at_commit() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    let ds = FakeEntity::new("course:1");

    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(100),
                        rows: vec![FakeEntityRow {
                            id: 1,
                            name: Supplied::Value("A".into()),
                            ..Default::default()
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;

                let tx = conns
                    .state
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                bump_epochs(&["course:1"], &tx)?;
                tx.commit()?;

                let err = ds
                    .ingest(
                        &[FakeEntityPage {
                            fetched_at: ts(200),
                            rows: vec![FakeEntityRow {
                                id: 1,
                                name: Supplied::Value("B".into()),
                                ..Default::default()
                            }],
                        }
                        .to_ingest()],
                        &ingest_ok(0),
                        conns,
                    )
                    .unwrap_err();
                assert!(matches!(err, crate::store::IngestError::Epoch(_)));

                let name: String = conns.cache.query_row(
                    "SELECT name FROM fake_entities WHERE id = 1",
                    [],
                    |r| r.get(0),
                )?;
                assert_eq!(name, "A");
                Ok(())
            }
        })
        .unwrap();
}

#[test]
fn newer_schema_refused() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    {
        let conn = rusqlite::Connection::open(&paths.cache_db).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
    }
    match Store::open(&paths, &doc) {
        Err(DbError::NewerSchema { found: 99, .. }) => {}
        Err(e) => panic!("expected NewerSchema(99), got {e}"),
        Ok(_) => panic!("expected NewerSchema(99), got Ok"),
    }
}

#[test]
fn cache_clear_with_concurrent_reader() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    let ds = FakeEntity::new("s");

    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(100),
                        rows: vec![FakeEntityRow {
                            id: 7,
                            name: Supplied::Value("X".into()),
                            ..Default::default()
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;
                Ok(())
            }
        })
        .unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let barrier_r = Arc::clone(&barrier);
    let store_path = paths.cache_db.clone();
    let reader = thread::spawn(move || {
        let conn = rusqlite::Connection::open(&store_path).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        barrier_r.wait();
        let _ = conn.query_row("SELECT COUNT(*) FROM fake_entities", [], |r| {
            r.get::<_, i64>(0)
        });
    });

    barrier.wait();
    open.store
        .call_blocking(|conns| {
            cache_clear(&mut conns.cache)?;
            let stats = cache_stats(&conns.cache)?;
            assert_eq!(stats.total_rows, 0);
            Ok(())
        })
        .unwrap();
    reader.join().unwrap();
    assert!(paths.cache_db.exists());
}

#[test]
fn scoped_pq_cached_p_enrollment_grades() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();

    open.store
        .call_blocking(|conns| {
            let tx = conns
                .cache
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            upsert_enrollment_grades(
                &tx,
                50,
                "P",
                ts(10),
                &[FieldWrite {
                    name: "current_score",
                    group: FieldGroup::Status,
                    value: Some("90".into()),
                }],
            )
            .map_err(ingest_err)?;
            upsert_enrollment_grades(
                &tx,
                50,
                "Q",
                ts(20),
                &[FieldWrite {
                    name: "current_score",
                    group: FieldGroup::Status,
                    value: Some("80".into()),
                }],
            )
            .map_err(ingest_err)?;
            upsert_enrollment_grades(
                &tx,
                50,
                "P",
                ts(30),
                &[FieldWrite {
                    name: "current_score",
                    group: FieldGroup::Status,
                    value: Some("95".into()),
                }],
            )
            .map_err(ingest_err)?;
            upsert_enrollment_grades(
                &tx,
                50,
                "Q",
                ts(20),
                &[FieldWrite {
                    name: "current_score",
                    group: FieldGroup::Status,
                    value: Some("70".into()),
                }],
            )
            .map_err(ingest_err)?;
            tx.commit()?;

            let p: f64 = conns.cache.query_row(
                "SELECT current_score FROM enrollment_grades WHERE enrollment_id=50 AND period='P'",
                [],
                |r| r.get(0),
            )?;
            let q: f64 = conns.cache.query_row(
                "SELECT current_score FROM enrollment_grades WHERE enrollment_id=50 AND period='Q'",
                [],
                |r| r.get(0),
            )?;
            assert!((p - 95.0).abs() < f64::EPSILON);
            assert!((q - 80.0).abs() < f64::EPSILON);

            let obs_p =
                field_observed_at(&conns.cache, "enrollment_grades", "50|P", "current_score")?
                    .unwrap();
            let obs_q =
                field_observed_at(&conns.cache, "enrollment_grades", "50|Q", "current_score")?
                    .unwrap();
            assert_eq!(obs_p, ts(30).to_string());
            assert_eq!(obs_q, ts(20).to_string());
            Ok(())
        })
        .unwrap();
}

#[test]
fn field_group_precedence_stale_full_fresh_thin() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    let ds = FakeEntity::new("course:9");

    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(10),
                        rows: vec![FakeEntityRow {
                            id: 1,
                            name: Supplied::Value("HW".into()),
                            due_at: Supplied::Value("due-old".into()),
                            description: Supplied::Value("long".into()),
                            score: Supplied::Value(10.0),
                            can_submit: Supplied::Value(true),
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;

                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(30),
                        rows: vec![FakeEntityRow {
                            id: 1,
                            name: Supplied::Value("HW".into()),
                            due_at: Supplied::Value("due-new".into()),
                            ..Default::default()
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;

                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(20),
                        rows: vec![FakeEntityRow {
                            id: 1,
                            name: Supplied::Value("HW".into()),
                            due_at: Supplied::Value("due-mid".into()),
                            description: Supplied::Value("mid".into()),
                            score: Supplied::Value(20.0),
                            can_submit: Supplied::Absent,
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;

                let (due, score, can, desc): (String, f64, i64, String) = conns.cache.query_row(
                    "SELECT due_at, score, can_submit, description FROM fake_entities WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
                assert_eq!(due, "due-new");
                assert!((score - 20.0).abs() < f64::EPSILON);
                assert_eq!(can, 1);
                assert_eq!(desc, "mid");

                assert_eq!(
                    field_observed_at(&conns.cache, "fake_entity", "1", "score")?.unwrap(),
                    ts(20).to_string()
                );
                assert_eq!(
                    field_observed_at(&conns.cache, "fake_entity", "1", "can_submit")?.unwrap(),
                    ts(10).to_string()
                );
                assert_eq!(
                    field_observed_at(&conns.cache, "fake_entity", "1", "due_at")?.unwrap(),
                    ts(30).to_string()
                );
                Ok(())
            }
        })
        .unwrap();
}

#[test]
fn cache_clear_cannot_reset_epoch() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();

    open.store
        .call_blocking(|conns| {
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            bump_epochs(&["assignments:course:1", "planner:*"], &tx)?;
            bump_epochs(&["assignments:course:1"], &tx)?;
            tx.commit()?;
            assert_eq!(read_scope_epoch(&conns.state, "assignments:course:1")?, 2);
            assert_eq!(read_scope_epoch(&conns.state, "planner:window:a")?, 1);

            cache_clear(&mut conns.cache)?;
            assert_eq!(read_scope_epoch(&conns.state, "assignments:course:1")?, 2);
            Ok(())
        })
        .unwrap();
}

#[test]
fn pending_for_assignment_with_supersession() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();

    open.store
        .call_blocking(|conns| {
            conns.state.execute(
                "INSERT INTO submission_journal (
                    journal_id, identity_key, course_id, assignment_id, kind, state, created_at
                 ) VALUES ('j1', 'k', 1, 42, 'online_upload', 'outcome_unknown', '2026-01-01T00:00:00Z')",
                [],
            )?;
            assert!(pending_for_assignment(&conns.state, 42)?);

            conns.state.execute(
                "INSERT INTO submission_journal (
                    journal_id, identity_key, course_id, assignment_id, kind, state, created_at
                 ) VALUES ('j2', 'k', 1, 42, 'online_upload', 'submitted', '2026-01-02T00:00:00Z')",
                [],
            )?;
            assert!(!pending_for_assignment(&conns.state, 42)?);
            Ok(())
        })
        .unwrap();
}

#[test]
fn hit_predicate_basic() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    let ds = FakeEntity::new("active");

    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                ds.ingest(
                    &[FakeEntityPage {
                        fetched_at: ts(1_000),
                        rows: vec![FakeEntityRow {
                            id: 1,
                            name: Supplied::Value("c".into()),
                            ..Default::default()
                        }],
                    }
                    .to_ingest()],
                    &ingest_ok(0),
                    conns,
                )
                .map_err(ingest_err)?;

                let hit = lookup(
                    conns,
                    &LookupQuery {
                        dataset: "fake",
                        scope: "active",
                        now: ts(1_000 + 60),
                        ttl: jiff::Span::new().minutes(10),
                        window: None,
                    },
                )?;
                assert!(matches!(hit, LookupResult::Hit(_)));

                let stale = lookup(
                    conns,
                    &LookupQuery {
                        dataset: "fake",
                        scope: "active",
                        now: ts(1_000 + 3600),
                        ttl: jiff::Span::new().minutes(10),
                        window: None,
                    },
                )?;
                assert!(matches!(stale, LookupResult::Stale(_)));
                Ok(())
            }
        })
        .unwrap();
}

#[test]
fn identity_removal_waiting_opener_sees_changed() {
    let dir = TempDir::new().unwrap();
    let (paths, doc) = setup_identity(&dir);

    let excl = IdentityLock::acquire_exclusive(&paths).unwrap();
    let paths_c = paths.clone();
    let doc_c = doc.clone();
    let handle = thread::spawn(move || IdentityLock::acquire_shared(&paths_c, &doc_c));

    thread::sleep(Duration::from_millis(100));
    let mut cred_ok = || Ok(());
    let mut profiles_ok = || Ok(());
    remove_identity(
        &paths,
        excl,
        &mut RemovalCallbacks {
            delete_credentials: &mut cred_ok,
            remove_profiles: &mut profiles_ok,
        },
    )
    .unwrap();

    let result = handle.join().unwrap();
    assert!(matches!(result, Err(IdentityError::Changed)));
}

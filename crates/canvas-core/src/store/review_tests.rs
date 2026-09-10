//! Adversarial regressions found during M1-a review.
use std::sync::{Arc, mpsc};
use std::time::Duration;

use fs4::fs_std::FileExt;
use tempfile::TempDir;

use super::*;
use crate::identity::{
    IdentityDocument, IdentityError, IdentityLock, Paths, RemovalCallbacks, remove_identity,
};

fn identity() -> (TempDir, Paths, IdentityDocument) {
    let dir = TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(dir.path(), &doc.key);
    doc.write(&paths.identity_json()).unwrap();
    (dir, paths, doc)
}

#[test]
fn direct_store_open_verifies_identity_and_holds_lock() {
    let (_dir, paths, doc) = identity();
    let mut changed = doc.clone();
    changed.generation = uuid::Uuid::new_v4();
    assert!(matches!(
        Store::open(&paths, &changed),
        Err(DbError::Identity(IdentityError::Changed))
    ));
    let store = Store::open(&paths, &doc).unwrap();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.lock_path)
        .unwrap();
    assert!(!FileExt::try_lock_exclusive(&file).unwrap());
    store
        .call_blocking(|conns| {
            let count: i64 = conns
                .state
                .query_row("SELECT count(*) FROM identity", [], |r| r.get(0))?;
            assert_eq!(count, 5);
            Ok(())
        })
        .unwrap();
    drop(store);
    let lock = IdentityLock::acquire_exclusive(&paths).unwrap();
    drop(lock);
}

#[test]
fn store_paths_cannot_escape_identity_or_use_another_lock() {
    let (_dir, paths, doc) = identity();
    let (_other, other_paths, _) = identity();
    let mut escaped = paths.clone();
    escaped.cache_db = other_paths.cache_db.clone();
    assert!(Store::open(&escaped, &doc).is_err());
    assert!(!other_paths.cache_db.exists());
    let lock = IdentityLock::acquire_exclusive(&paths).unwrap();
    let mut credentials = || panic!("wrong identity lock must not authorize callbacks");
    let mut profiles = || Ok(());
    assert!(
        remove_identity(
            &other_paths,
            lock,
            &mut RemovalCallbacks {
                delete_credentials: &mut credentials,
                remove_profiles: &mut profiles,
            }
        )
        .is_err()
    );
    assert!(other_paths.identity_json().exists());
}

#[test]
fn all_store_handles_use_one_worker_and_survive_panics() {
    let (_dir, paths, doc) = identity();
    let first = Store::open(&paths, &doc).unwrap();
    let second = Store::open(&paths, &doc).unwrap();
    let a = first
        .call_blocking(|_| Ok(std::thread::current().id()))
        .unwrap();
    let b = second
        .call_blocking(|_| Ok(std::thread::current().id()))
        .unwrap();
    assert_eq!(a, b);
    assert!(matches!(
        first.call_blocking::<_, ()>(|_| panic!("job panic")),
        Err(DbError::WorkerPanicked)
    ));
    assert_eq!(second.call_blocking(|_| Ok(42)).unwrap(), 42);
}

#[tokio::test(flavor = "current_thread")]
async fn saturated_queue_does_not_block_runtime() {
    let (_dir, paths, doc) = identity();
    let store = Arc::new(Store::open(&paths, &doc).unwrap());
    let (started_tx, started_rx) = mpsc::channel();
    let first_store = Arc::clone(&store);
    let first = tokio::spawn(async move {
        first_store
            .call(move |_| {
                started_tx.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(250));
                Ok(())
            })
            .await
            .unwrap();
    });
    while started_rx.try_recv().is_err() {
        tokio::task::yield_now().await;
    }
    let start = std::time::Instant::now();
    let mut calls = Vec::new();
    for _ in 0..80 {
        let store = Arc::clone(&store);
        calls.push(tokio::spawn(async move {
            store.call(|_| Ok(())).await.unwrap();
        }));
    }
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(start.elapsed() < Duration::from_millis(150));
    first.await.unwrap();
    for call in calls {
        call.await.unwrap();
    }
}

#[cfg(unix)]
#[test]
fn identity_writes_do_not_follow_predictable_temp_symlink_and_databases_are_private() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (dir, paths, doc) = identity();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "untouched").unwrap();
    symlink(&victim, paths.identity_dir.join(".identity.json.tmp")).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    assert_eq!(std::fs::read_to_string(victim).unwrap(), "untouched");
    let _store = Store::open(&paths, &doc).unwrap();
    for path in [paths.identity_json(), paths.cache_db, paths.state_db] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn simultaneous_processes_migrate_fresh_identity() {
    use std::process::Command;
    if let Ok(root) = std::env::var("CANVAS_REVIEW_MIGRATION_ROOT") {
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(root, &doc.key);
        let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
        std::fs::write(paths.data_root.join("ready"), "ready").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !paths.data_root.join("go").exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let _store = Store::open(&paths, &doc).unwrap();
        return;
    }
    let (_dir, paths, doc) = identity();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::review_tests::simultaneous_processes_migrate_fresh_identity",
        ])
        .env("CANVAS_REVIEW_MIGRATION_ROOT", &paths.data_root)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !paths.data_root.join("ready").exists() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not become ready");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(paths.data_root.join("go"), "go").unwrap();
    let store = Store::open(&paths, &doc);
    assert!(child.wait().unwrap().success());
    store
        .unwrap()
        .call_blocking(|conns| {
            for conn in [&conns.cache, &conns.state] {
                let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(version, 1);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn removal_credential_failure_preserves_directory_and_profiles() {
    let (_dir, paths, _) = identity();
    let lock = IdentityLock::acquire_exclusive(&paths).unwrap();
    let mut credentials = || {
        Err(IdentityError::Mismatch {
            reason: "credential failure".into(),
        })
    };
    let mut profiles = || panic!("profiles must survive failed credential cleanup");
    assert!(
        remove_identity(
            &paths,
            lock,
            &mut RemovalCallbacks {
                delete_credentials: &mut credentials,
                remove_profiles: &mut profiles,
            }
        )
        .is_err()
    );
    assert!(paths.identity_json().exists());
    assert!(paths.lock_path.exists());
}

fn ts(seconds: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(seconds).unwrap()
}
fn complete(epoch_seen: i64) -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}
fn page(seconds: i64, name: &str) -> IngestPage {
    FakeEntityPage {
        fetched_at: ts(seconds),
        rows: vec![FakeEntityRow {
            id: 1,
            name: Supplied::Value(name.into()),
            ..Default::default()
        }],
    }
    .to_ingest()
}
fn ingest_error(error: impl std::fmt::Display) -> DbError {
    DbError::Message(error.to_string())
}

#[test]
fn qualified_epochs_and_overlapping_prefixes_invalidate_hits() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let ds = FakeEntity::new("course:1");
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            for _ in 0..5 {
                bump_epochs(&["fake:course:1"], &tx)?;
            }
            tx.commit()?;
            ds.ingest(&[page(100, "old")], &complete(5), conns)
                .map_err(ingest_error)?;
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            bump_epochs(&["fake:*"], &tx)?;
            tx.commit()?;
            assert_eq!(read_scope_epoch(&conns.state, "fake:course:1")?, 6);
            assert!(matches!(
                lookup_dataset(conns, &ds, ts(101), None)?,
                LookupResult::Stale(_)
            ));
            assert!(matches!(
                ds.ingest(&[page(102, "new")], &complete(5), conns),
                Err(IngestError::Epoch(_))
            ));
            let name: String =
                conns
                    .cache
                    .query_row("SELECT name FROM fake_entities", [], |r| r.get(0))?;
            assert_eq!(name, "old");
            Ok(())
        })
        .unwrap();
}

#[test]
fn epoch_advanced_during_upsert_aborts_whole_refresh() {
    struct BumpingDataset {
        state_path: std::path::PathBuf,
    }
    impl Dataset for BumpingDataset {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn scope_key(&self) -> &'static str {
            "course:1"
        }
        fn ttl(&self) -> jiff::Span {
            jiff::Span::new().minutes(10)
        }
        fn entity_kind(&self) -> &'static str {
            "fake_entity"
        }
        fn upsert_entity(
            &self,
            tx: &rusqlite::Transaction<'_>,
            entity: &EntityIngest,
            at: jiff::Timestamp,
        ) -> Result<(), IngestError> {
            FakeEntity::new("course:1").upsert_entity(tx, entity, at)?;
            // Independent connection simulates another process's journal transition.
            let mut state = rusqlite::Connection::open(&self.state_path)?;
            let state_tx =
                state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            bump_epochs(&["fake:course:1"], &state_tx)?;
            state_tx.commit()?;
            Ok(())
        }
    }
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(move |conns| {
            let ds = FakeEntity::new("course:1");
            ds.ingest(&[page(100, "old")], &complete(0), conns)
                .map_err(ingest_error)?;
            let ds = BumpingDataset {
                state_path: paths.state_db,
            };
            assert!(matches!(
                ds.ingest(&[page(200, "new")], &complete(0), conns),
                Err(IngestError::Epoch(_))
            ));
            let name: String =
                conns
                    .cache
                    .query_row("SELECT name FROM fake_entities", [], |r| r.get(0))?;
            assert_eq!(name, "old");
            assert_eq!(
                load_fetch_log(&conns.cache, "fake", "course:1")?
                    .unwrap()
                    .fetched_at,
                ts(100)
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn wider_windows_are_reused_only_with_matching_context_and_containment() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let mut opts = complete(0);
            opts.window = Some((ts(10), ts(90)));
            opts.contexts = Some("hash-A");
            FakeEntity::new("window:wide")
                .ingest(&[page(100, "old")], &opts, conns)
                .map_err(ingest_error)?;
            let mut query = LookupQuery {
                dataset: "fake",
                scope: "window:narrow",
                now: ts(110),
                ttl: jiff::Span::new().minutes(10),
                window: Some(WindowQuery {
                    contexts: "hash-A",
                    start: ts(20),
                    end: ts(80),
                }),
            };
            assert!(matches!(lookup(conns, &query)?, LookupResult::Hit(_)));
            query.window.as_mut().unwrap().contexts = "hash-B";
            assert!(matches!(lookup(conns, &query)?, LookupResult::Miss));
            query.window.as_mut().unwrap().contexts = "hash-A";
            query.window.as_mut().unwrap().end = ts(91);
            assert!(matches!(lookup(conns, &query)?, LookupResult::Miss));
            query.window.as_mut().unwrap().end = ts(80);
            let tx = conns
                .state
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            bump_epochs(&["fake:*"], &tx)?;
            tx.commit()?;
            assert!(matches!(lookup(conns, &query)?, LookupResult::Stale(_)));
            Ok(())
        })
        .unwrap();
}

#[test]
fn interrupted_and_invalid_refreshes_preserve_values_membership_and_coverage() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let ds = FakeEntity::new("course:1");
            ds.ingest(&[page(100, "old")], &complete(0), conns)
                .map_err(ingest_error)?;
            let mut failed = complete(0);
            failed.complete = false;
            failed.error = Some("page 2 failed");
            ds.ingest(&[page(200, "partial")], &failed, conns)
                .map_err(ingest_error)?;
            let log = load_fetch_log(&conns.cache, "fake", "course:1")?.unwrap();
            assert!(log.complete && log.stale);
            assert_eq!(log.fetched_at, ts(100));
            assert_eq!(log.error.as_deref(), Some("page 2 failed"));
            let mut bad_page = page(300, "invalid");
            bad_page.entities[0].fields.push(FieldWrite {
                name: "score",
                group: FieldGroup::Status,
                value: Some("not-a-number".into()),
            });
            assert!(
                ds.ingest(&[page(250, "first-page"), bad_page], &complete(0), conns)
                    .is_err()
            );
            let name: String =
                conns
                    .cache
                    .query_row("SELECT name FROM fake_entities", [], |r| r.get(0))?;
            let members: i64 =
                conns
                    .cache
                    .query_row("SELECT count(*) FROM membership", [], |r| r.get(0))?;
            assert_eq!(name, "old");
            assert_eq!(members, 1);
            assert_eq!(
                field_observed_at(&conns.cache, "fake_entity", "1", "name")?.unwrap(),
                ts(100).to_string()
            );
            assert!(matches!(
                lookup_dataset(conns, &ds, ts(301), None)?,
                LookupResult::Stale(_)
            ));
            Ok(())
        })
        .unwrap();
}

#[test]
fn fractional_observations_and_explicit_null_follow_time_order() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let ds = FakeEntity::new("course:1");
            ds.ingest(&[page(100, "old")], &complete(0), conns)
                .map_err(ingest_error)?;
            let mut newer = page(100, "unused");
            newer.fetched_at = jiff::Timestamp::from_millisecond(100_500).unwrap();
            newer.entities[0].fields[0].value = None;
            ds.ingest(&[newer.clone()], &complete(0), conns)
                .map_err(ingest_error)?;
            ds.ingest(&[page(100, "stale")], &complete(0), conns)
                .map_err(ingest_error)?;
            let name: Option<String> =
                conns
                    .cache
                    .query_row("SELECT name FROM fake_entities", [], |r| r.get(0))?;
            assert!(name.is_none());
            assert_eq!(
                field_observed_at(&conns.cache, "fake_entity", "1", "name")?.unwrap(),
                newer.fetched_at.to_string()
            );
            let value: FakeEntityRow =
                serde_json::from_str(r#"{"id":1,"name":null,"can_submit":false}"#).unwrap();
            assert_eq!(value.name, Supplied::Null);
            assert_eq!(value.due_at, Supplied::Absent);
            assert_eq!(value.can_submit, Supplied::Value(false));
            Ok(())
        })
        .unwrap();
}

#[test]
fn overlapping_pages_deduplicate_membership_and_leave_other_scopes_untouched() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store.call_blocking(|conns| {
        let ds = FakeEntity::new("course:1");
        FakeEntity::new("other").ingest(&[page(50, "original")], &complete(0), conns).map_err(ingest_error)?;
        ds.ingest(&[page(100, "old"), page(200, "new")], &complete(0), conns).map_err(ingest_error)?;
        let log = load_fetch_log(&conns.cache, "fake", "course:1")?.unwrap();
        assert_eq!(log.count, 1);
        assert!(load_fetch_log(&conns.cache, "fake", "other")?.is_some());
        ds.ingest(&[IngestPage { fetched_at: ts(300), entities: vec![] }], &complete(0), conns).map_err(ingest_error)?;
        assert!(matches!(lookup_dataset(conns, &ds, ts(301), None)?, LookupResult::Hit(row) if row.count == 0));
        let entities: i64 = conns.cache.query_row("SELECT count(*) FROM fake_entities", [], |r| r.get(0))?;
        let members: i64 = conns.cache.query_row("SELECT count(*) FROM membership WHERE scope = 'other'", [], |r| r.get(0))?;
        assert_eq!((entities, members), (1, 1));
        Ok(())
    }).unwrap();
}

#[test]
fn grade_letters_are_written_with_independent_period_clocks() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let tx = conns
                .cache
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            for (period, at, grade) in [
                ("P", 30, Some("A")),
                ("Q", 20, Some("B")),
                ("P", 25, Some("C")),
                ("Q", 21, None),
            ] {
                upsert_enrollment_grades(
                    &tx,
                    7,
                    period,
                    ts(at),
                    &[FieldWrite {
                        name: "current_grade",
                        group: FieldGroup::Status,
                        value: grade.map(str::to_owned),
                    }],
                )
                .map_err(ingest_error)?;
            }
            tx.commit()?;
            let p: Option<String> = conns.cache.query_row(
                "SELECT current_grade FROM enrollment_grades WHERE period='P'",
                [],
                |r| r.get(0),
            )?;
            let q: Option<String> = conns.cache.query_row(
                "SELECT current_grade FROM enrollment_grades WHERE period='Q'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(p.as_deref(), Some("A"));
            assert!(q.is_none());
            Ok(())
        })
        .unwrap();
}

#[test]
fn journal_pending_states_acknowledgment_and_subsecond_supersession() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store.call_blocking(|conns| {
        let tx = conns.state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO submission_journal (journal_id, identity_key, course_id, assignment_id, kind, state, created_at)
            VALUES ('old', 'key', 1, 7, 'online_upload', 'planned', '2026-01-01T00:00:00Z')", [])?;
        assert!(pending_for_assignment(&tx, 7)?);
        assert!(tx.execute("INSERT INTO submission_journal (journal_id, identity_key, course_id, assignment_id, kind, state, created_at)
            VALUES ('conflict', 'key', 1, 7, 'online_upload', 'uploading', '2026-01-01T00:00:00Z')", []).is_err());
        for state in ["uploading", "uploaded", "posting", "outcome_unknown"] {
            tx.execute("UPDATE submission_journal SET state=?1", [state])?;
            assert!(pending_for_assignment(&tx, 7)?);
        }
        tx.execute("UPDATE submission_journal SET acknowledged_at='2026-01-01T00:00:01Z'", [])?;
        assert!(!pending_for_assignment(&tx, 7)?);
        tx.execute("UPDATE submission_journal SET acknowledged_at=NULL", [])?;
        tx.execute("INSERT INTO submission_journal (journal_id, identity_key, course_id, assignment_id, kind, state, created_at)
            VALUES ('later', 'key', 1, 7, 'online_upload', 'matched', '2026-01-01T00:00:00.5Z')", [])?;
        assert!(!pending_for_assignment(&tx, 7)?);
        tx.execute("UPDATE submission_journal SET state='refused' WHERE journal_id='later'", [])?;
        assert!(pending_for_assignment(&tx, 7)?);
        tx.execute("UPDATE submission_journal SET state='uploaded_not_submitted' WHERE journal_id='old'", [])?;
        assert!(!pending_for_assignment(&tx, 7)?);
        // Every failure/recovery transition keeps its own timestamp when reconciled.
        tx.execute("UPDATE submission_journal SET upload_incomplete_at=?1, uploaded_not_submitted_at=?1,
            outcome_unknown_at=?1, refused_at=?1 WHERE journal_id='old'", [ts(1).to_string()])?;
        tx.commit()?;
        Ok(())
    }).unwrap();
}

#[test]
fn schema_contains_all_cache_tables_and_refuses_newer_state() {
    let (_dir, paths, doc) = identity();
    let store = Store::open(&paths, &doc).unwrap();
    store
        .call_blocking(|conns| {
            let mut stmt = conns
                .cache
                .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")?;
            let tables = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut expected = CACHE_TABLES.to_vec();
            expected.sort_unstable();
            assert_eq!(tables, expected);
            for table in CACHE_TABLES
                .iter()
                .filter(|t| !["membership", "field_obs", "fetch_log"].contains(t))
            {
                conns.cache.prepare(&format!(
                    "SELECT observed_at_core, observed_at_detail, observed_at_status FROM {table}"
                ))?;
            }
            conns.state.execute_batch("PRAGMA user_version=99")?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        Store::open(&paths, &doc),
        Err(DbError::NewerSchema { found: 99, .. })
    ));
}

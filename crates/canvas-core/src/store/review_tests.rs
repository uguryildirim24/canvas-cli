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
            let count: i64 =
                conns
                    .state
                    .query_row("SELECT count(*) FROM identity_meta", [], |r| r.get(0))?;
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

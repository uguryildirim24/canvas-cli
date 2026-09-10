//! Real subprocess owners, killed at each publication/transaction boundary.
use super::*;
use crate::identity::{IdentityDocument, Paths};
use crate::store::{OpenIdentity, Store};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Publish a handshake file atomically.
///
/// The parent polls for the file's existence, so a plain `write` would let it
/// read a created-but-empty file and mistake a published journal for none.
fn publish(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, contents).unwrap();
    std::fs::rename(&temporary, path).unwrap();
}

pub(super) fn checkpoint(phase: &str, jid: &str) {
    if std::env::var("CANVAS_JOURNAL_PHASE").as_deref() != Ok(phase) {
        return;
    }
    publish(std::env::var("CANVAS_JOURNAL_READY").unwrap(), jid);
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

fn open(root: &Path) -> (Paths, Store, IdentityDocument) {
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(root, &doc.key);
    let stored = IdentityDocument::read(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &stored).unwrap().store;
    (paths, store, stored)
}

fn opts(doc: &IdentityDocument) -> CreateOpts {
    CreateOpts {
        identity_key: doc.key.to_string(),
        course_id: 1,
        assignment_id: 42,
        kind: "online_upload".into(),
        intended_payload_json: r#"{"files":[{"name":"a","size":1,"sha256":"abc"}]}"#.into(),
        baseline_attempt: Some(0),
        baseline_submission_id: None,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn helper() {
    let Ok(root) = std::env::var("CANVAS_JOURNAL_ROOT") else {
        return;
    };
    let (paths, store, doc) = open(Path::new(&root));
    let mode = std::env::var("CANVAS_JOURNAL_MODE").unwrap_or_default();
    if mode == "compete" {
        let result = std::env::var("CANVAS_JOURNAL_READY").unwrap();
        publish(&result, "ready");
        while !Path::new(&root).join("start").exists() {
            std::thread::sleep(Duration::from_millis(2));
        }
        let admission = match AdmissionLock::try_acquire(&paths.identity_dir, 42) {
            Ok(lock) => lock,
            Err(LockError::InProgress) => {
                publish(result, "blocked");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        match create(&store, &paths.identity_dir, &admission, &opts(&doc)) {
            Ok((_jid, _owner)) => {
                drop(admission);
                publish(result, "created");
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            Err(JournalError::InProgress) => {
                publish(result, "blocked");
                return;
            }
            Err(e) => panic!("{e}"),
        }
    }
    if matches!(mode.as_str(), "probe" | "recover") {
        let jid = std::env::var("CANVAS_JOURNAL_ID").unwrap();
        let row = get_journal(&store, &jid).unwrap().unwrap();
        if mode == "probe" {
            assert_eq!(
                owner_status_for(&paths.identity_dir, &jid, row.state).unwrap(),
                OwnerStatus::Live
            );
            assert_eq!(
                recover_if_owner_absent(&store, &paths.identity_dir, &jid).unwrap(),
                None
            );
            assert!(
                store
                    .call_blocking(|c| crate::store::pending_for_assignment(&c.state, 42))
                    .unwrap()
            );
            let rows = crate::receipts::list_journals(
                &store,
                &paths.identity_dir,
                &crate::receipts::ListFilter::default(),
            )
            .unwrap();
            assert_eq!(rows[0].owner, "live");
            let client = canvas_api::Client::new(
                "https://canvas.example".parse().unwrap(),
                canvas_api::Secret::new("test"),
                "test",
            )
            .unwrap();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = rt
                .block_on(crate::submit::reconcile(
                    &client,
                    &store,
                    &paths,
                    &jid,
                    false,
                    jiff::Timestamp::now(),
                ))
                .unwrap();
            assert_eq!(result.outcome, crate::submit::ReconcileOutcome::Recovery);
            assert_eq!(client.telemetry().api, 0);
            assert_eq!(get_journal(&store, &jid).unwrap().unwrap().state, row.state);
        } else {
            recover_if_owner_absent(&store, &paths.identity_dir, &jid).unwrap();
        }
        return;
    }
    checkpoint("before_admission", "");
    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 42).unwrap();
    checkpoint("admission_acquired", "");
    let (jid, owner) = create(&store, &paths.identity_dir, &admission, &opts(&doc)).unwrap();
    drop(admission);
    checkpoint("admission_released", &jid);
    transition(
        &store,
        &owner,
        &jid,
        State::Planned,
        State::Uploading,
        TransitionPatch::default(),
    )
    .unwrap();
    checkpoint("uploading", &jid);
    append_uploaded_file_id(&store, &owner, &jid, 0, 77).unwrap();
    checkpoint("file_recorded", &jid);
    transition(
        &store,
        &owner,
        &jid,
        State::Uploading,
        State::Uploaded,
        TransitionPatch::default(),
    )
    .unwrap();
    checkpoint("uploaded", &jid);
    mark_posting(&store, &owner, &jid).unwrap();
    checkpoint("posting", &jid);
    let raw = br#"{"id":5,"attempt":1,"attachments":[{"id":77,"display_name":"a","url":"https://storage.test/?token=secret"}]}"#;
    let posted = allowlist_from_json(
        Evidence::PostResponse,
        &serde_json::from_slice(raw).unwrap(),
        Some(raw),
    )
    .unwrap();
    let receipt = ReceiptRecord {
        receipt_id: "receipt-1".into(),
        journal_id: jid.clone(),
        attribution: "observed".into(),
        posted,
        readback: None,
    };
    commit_success(&store, &owner, &jid, 201, &receipt).unwrap();
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "journal::crash_tests::helper", "--nocapture"])
        .env("CANVAS_JOURNAL_ROOT", root)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}
fn setup() -> (tempfile::TempDir, Paths, Store) {
    let root = tempfile::tempdir().unwrap();
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(root.path(), &doc.key);
    std::fs::create_dir_all(&paths.identity_dir).unwrap();
    std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    (root, paths, store)
}

#[test]
#[allow(clippy::too_many_lines)] // One table-driven crash lifecycle, with a fresh identity per row.
fn kills_cover_publication_active_phases_and_success_atomicity() {
    for phase in [
        "before_admission",
        "admission_file_created",
        "admission_acquired",
        "owner_file_created",
        "owner_acquired",
        "inserted",
        "published",
        "admission_released",
        "uploading",
        "file_recorded",
        "uploaded",
        "posting",
        "success_before_commit",
        "success_after_commit",
    ] {
        let (root, paths, store) = setup();
        let ready = root.path().join("ready");
        let mut child = Process(
            command(root.path())
                .env("CANVAS_JOURNAL_PHASE", phase)
                .env("CANVAS_JOURNAL_READY", &ready)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "child did not reach {phase}");
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited at {phase}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let jid = std::fs::read_to_string(&ready).unwrap();
        let before = get_journal(&store, &jid).unwrap();
        if let Some(row) = &before
            && !row.state.is_terminal()
            && phase != "success_before_commit"
        {
            assert!(
                command(root.path())
                    .env("CANVAS_JOURNAL_MODE", "probe")
                    .env("CANVAS_JOURNAL_ID", &jid)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        if before.is_none() {
            assert!(
                get_journal(&store, &jid).unwrap().is_none(),
                "uncommitted row survived {phase}"
            );
            let admission = AdmissionLock::try_acquire(&paths.identity_dir, 42).unwrap();
            let (_, _, doc) = open(root.path());
            create(&store, &paths.identity_dir, &admission, &opts(&doc)).unwrap();
            continue;
        }
        // Two independent recoverers race on the actual dead owner's journal.
        let mut a = Process(
            command(root.path())
                .env("CANVAS_JOURNAL_MODE", "recover")
                .env("CANVAS_JOURNAL_ID", &jid)
                .spawn()
                .unwrap(),
        );
        let mut b = Process(
            command(root.path())
                .env("CANVAS_JOURNAL_MODE", "recover")
                .env("CANVAS_JOURNAL_ID", &jid)
                .spawn()
                .unwrap(),
        );
        assert!(a.0.wait().unwrap().success());
        assert!(b.0.wait().unwrap().success());
        let row = get_journal(&store, &jid).unwrap().unwrap();
        let expected = match phase {
            "published" | "admission_released" => State::Refused,
            "uploading" | "file_recorded" => State::UploadIncomplete,
            "uploaded" => State::UploadedNotSubmitted,
            "posting" | "success_before_commit" => State::OutcomeUnknown,
            "success_after_commit" => State::Submitted,
            _ => unreachable!(),
        };
        assert_eq!(row.state, expected, "{phase}");
        if expected == State::Submitted {
            let receipt: serde_json::Value =
                serde_json::from_str(row.receipt_record_json.as_deref().unwrap()).unwrap();
            assert_eq!(receipt["identity"]["user_id"], "7");
            assert_eq!(receipt["assignment_id"], "42");
            assert_eq!(receipt["files"][0]["name"], "a");
            assert_eq!(receipt["files"][0]["canvas_file_id"], "77");
            assert!(receipt["readback"].is_null());
            assert!(!receipt.to_string().contains("token=secret"));
            assert_eq!(row.post_status, Some(201));
            let exported = crate::receipts::export(&store, &paths, &jid, None).unwrap();
            let rebuilt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(exported.path.unwrap()).unwrap()).unwrap();
            assert_eq!(rebuilt, receipt);
        } else {
            assert!(row.receipt_record_json.is_none());
        }
        if phase == "uploaded" {
            assert_eq!(row.not_submitted_evidence.as_deref(), Some("never_sent"));
        }
        let epochs: i64 = store
            .call_blocking(|c| {
                Ok(c.state.query_row(
                    "SELECT COUNT(*) FROM scope_epoch WHERE epoch > 0",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(epochs, 7);
    }
}

#[test]
fn simultaneous_processes_publish_only_one_active_journal() {
    let (root, _paths, store) = setup();
    let a_ready = root.path().join("a");
    let b_ready = root.path().join("b");
    let _a = Process(
        command(root.path())
            .env("CANVAS_JOURNAL_MODE", "compete")
            .env("CANVAS_JOURNAL_READY", &a_ready)
            .spawn()
            .unwrap(),
    );
    let _b = Process(
        command(root.path())
            .env("CANVAS_JOURNAL_MODE", "compete")
            .env("CANVAS_JOURNAL_READY", &b_ready)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !a_ready.exists() || !b_ready.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    publish(root.path().join("start"), "go");
    loop {
        let a = std::fs::read_to_string(&a_ready).unwrap();
        let b = std::fs::read_to_string(&b_ready).unwrap();
        if (a == "created" && b == "blocked") || (a == "blocked" && b == "created") {
            break;
        }
        assert!(Instant::now() < deadline, "a={a}, b={b}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let count: i64 = store
        .call_blocking(|c| {
            Ok(c.state.query_row(
                "SELECT COUNT(*) FROM submission_journal WHERE state = 'planned'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(count, 1);
}

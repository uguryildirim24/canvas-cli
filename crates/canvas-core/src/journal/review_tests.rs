//! Adversarial state, provenance, and pending-hook regression cases.
use super::*;
use crate::identity::{IdentityDocument, Paths};
use crate::store::{OpenIdentity, Store};
use serde_json::{Value, json};

struct Fixture {
    _root: tempfile::TempDir,
    paths: Paths,
    store: Store,
    doc: IdentityDocument,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(root.path(), &doc.key);
        std::fs::create_dir_all(&paths.identity_dir).unwrap();
        std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let store = OpenIdentity::open(&paths, &doc).unwrap().store;
        Self {
            _root: root,
            paths,
            store,
            doc,
        }
    }
    fn opts(&self, assignment: i64) -> CreateOpts {
        CreateOpts { identity_key: self.doc.key.to_string(), course_id: 1, assignment_id: assignment,
            kind: "online_upload".into(), intended_payload_json: json!({"files":[
                {"name":"first","size":1,"sha256":"a".repeat(64)}, {"name":"second","size":2,"sha256":"b".repeat(64)}
            ]}).to_string(), baseline_attempt: Some(0), baseline_submission_id: None }
    }
    fn begin(&self, assignment: i64) -> (String, OwnerLock) {
        let admission = AdmissionLock::try_acquire(&self.paths.identity_dir, assignment).unwrap();
        create(
            &self.store,
            &self.paths.identity_dir,
            &admission,
            &self.opts(assignment),
        )
        .unwrap()
    }
    fn posting(&self, assignment: i64) -> (String, OwnerLock) {
        let (jid, owner) = self.begin(assignment);
        transition(
            &self.store,
            &owner,
            &jid,
            State::Planned,
            State::Uploading,
            TransitionPatch::default(),
        )
        .unwrap();
        append_uploaded_file_id(&self.store, &owner, &jid, 1, 22).unwrap();
        append_uploaded_file_id(&self.store, &owner, &jid, 0, 11).unwrap();
        transition(
            &self.store,
            &owner,
            &jid,
            State::Uploading,
            State::Uploaded,
            TransitionPatch::default(),
        )
        .unwrap();
        mark_posting(&self.store, &owner, &jid).unwrap();
        (jid, owner)
    }
    fn row(&self, jid: &str) -> JournalRow {
        get_journal(&self.store, jid).unwrap().unwrap()
    }
    fn pending(&self, assignment: i64) -> bool {
        self.store
            .call_blocking(move |c| crate::store::pending_for_assignment(&c.state, assignment))
            .unwrap()
    }
}
fn receipt(jid: &str, evidence: Evidence) -> ReceiptRecord {
    let value = json!({"id":5,"attempt":1,"submitted_at":jiff::Timestamp::now().to_string(),
        "body":"private body that must never persist", "attachments":[{"id":11,"display_name":"first","url":"https://s3.test/?token=SECRET"}, {"id":22,"display_name":"second"}]});
    let raw = serde_json::to_vec(&value).unwrap();
    let posted = allowlist_from_json(
        evidence,
        &value,
        (evidence == Evidence::PostResponse).then_some(raw.as_slice()),
    )
    .unwrap();
    ReceiptRecord {
        receipt_id: format!("r-{jid}"),
        journal_id: jid.into(),
        attribution: if evidence == Evidence::PostResponse {
            "observed"
        } else {
            "unproven"
        }
        .into(),
        readback: (evidence == Evidence::HistoryFiles).then(|| ReadbackRecord {
            submitted_at: posted.submitted_at.clone(),
            submitted_at_local: posted.submitted_at_local.clone(),
            late: posted.late,
            attachments: posted.attachments.clone(),
            body_sha256: posted.body_sha256.clone(),
        }),
        posted,
    }
}

#[test]
fn guarded_writes_epoch_rollback_and_readback_are_atomic() {
    let f = Fixture::new();
    let (jid, owner) = f.posting(42);
    let rec = receipt(&jid, Evidence::PostResponse);
    assert!(f.pending(42));
    assert!(matches!(
        transition(
            &f.store,
            &owner,
            &jid,
            State::Uploading,
            State::Uploaded,
            TransitionPatch::default()
        ),
        Err(JournalError::StateConflict)
    ));
    assert!(matches!(
        transition(
            &f.store,
            &owner,
            &jid,
            State::Posting,
            State::UploadedNotSubmitted,
            TransitionPatch::default()
        ),
        Err(JournalError::StateConflict)
    ));
    assert!(matches!(
        append_uploaded_file_id(&f.store, &owner, &jid, 0, 99),
        Err(JournalError::StateConflict)
    ));
    f.store.call_blocking(|c| {
        c.state.execute_batch("CREATE TEMP TRIGGER reject_epoch BEFORE INSERT ON scope_epoch BEGIN SELECT RAISE(ABORT, 'injected'); END;")?;
        Ok(())
    }).unwrap();
    assert!(commit_success(&f.store, &owner, &jid, 201, &rec).is_err());
    assert_eq!(f.row(&jid).state, State::Posting);
    assert!(f.row(&jid).receipt_record_json.is_none());
    f.store
        .call_blocking(|c| {
            c.state.execute_batch("DROP TRIGGER reject_epoch")?;
            Ok(())
        })
        .unwrap();
    commit_success(&f.store, &owner, &jid, 201, &rec).unwrap();
    assert!(!f.pending(42));
    let stored = f.row(&jid).receipt_record_json.unwrap();
    assert!(!stored.contains("private body"));
    assert!(!stored.contains("s3.test"));
    let parsed: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(parsed["files"][0]["canvas_file_id"], "11");
    assert_eq!(parsed["files"][1]["canvas_file_id"], "22");
    let readback = ReadbackRecord {
        submitted_at: rec.posted.submitted_at.clone(),
        submitted_at_local: rec.posted.submitted_at_local.clone(),
        late: Some(false),
        attachments: rec.posted.attachments.clone(),
        body_sha256: None,
    };
    assert!(matches!(
        enrich_readback(&f.store, &owner, &jid, 2, &readback),
        Err(JournalError::StateConflict)
    ));
    enrich_readback(&f.store, &owner, &jid, 1, &readback).unwrap();
    enrich_readback(&f.store, &owner, &jid, 1, &readback).unwrap();
    let row = f.row(&jid);
    assert_eq!(
        serde_json::from_str::<Value>(row.receipt_record_json.as_deref().unwrap()).unwrap()["readback"],
        serde_json::from_str::<Value>(row.readback_record_json.as_deref().unwrap()).unwrap()
    );
    let epoch: i64 = f
        .store
        .call_blocking(|c| {
            Ok(c.state.query_row(
                "SELECT epoch FROM scope_epoch WHERE scope = 'assignment_groups:course:1:*'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(epoch, 4); // three active transitions + success; failed success rolled back
}

#[test]
fn unknown_is_retired_only_by_acknowledgment_later_confirmation_or_explicit_assumption() {
    let f = Fixture::new();
    let (old, owner) = f.posting(42);
    transition(
        &f.store,
        &owner,
        &old,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch {
            post_status: Some(Some(504)),
            response_kind: Some(ResponseKind::CanvasError),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(f.pending(42));
    assert!(!is_superseded(&f.store, &old).unwrap());
    assert_eq!(f.row(&old).post_status, Some(504));
    assert_eq!(f.row(&old).response_kind.as_deref(), Some("canvas-error"));
    let now = jiff::Timestamp::now();
    assert!(assume_not_submitted(&f.store, &owner, &old, false, now).is_err());
    let later = jiff::Timestamp::from_second(now.as_second() + 1801).unwrap();
    assert!(assume_not_submitted(&f.store, &owner, &old, true, later).is_err());
    let (new, new_owner) = f.posting(42);
    commit_success(
        &f.store,
        &new_owner,
        &new,
        200,
        &receipt(&new, Evidence::PostResponse),
    )
    .unwrap();
    assert!(is_superseded(&f.store, &old).unwrap());
    assert!(!f.pending(42));
    assert_eq!(f.row(&old).state, State::OutcomeUnknown);
    let (ack, ack_owner) = f.posting(43);
    transition(
        &f.store,
        &ack_owner,
        &ack,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch::default(),
    )
    .unwrap();
    acknowledge(&f.store, &ack).unwrap();
    assert!(!f.pending(43));
    assert_eq!(f.row(&ack).state, State::OutcomeUnknown);
    assume_not_submitted(&f.store, &owner, &old, false, later).unwrap();
    assert_eq!(
        f.row(&old).not_submitted_evidence.as_deref(),
        Some("assumed")
    );
    assert!(matches!(
        acknowledge(&f.store, &new),
        Err(JournalError::StateConflict)
    ));
}

#[test]
fn history_files_match_has_receipt_and_unproven_attribution() {
    let f = Fixture::new();
    let (jid, owner) = f.posting(42);
    transition(
        &f.store,
        &owner,
        &jid,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch::default(),
    )
    .unwrap();
    let mut rec = receipt(&jid, Evidence::HistoryFiles);
    rec.posted.attachments.pop();
    assert!(commit_matched(&f.store, &owner, &jid, &rec).is_err());
    let rec = receipt(&jid, Evidence::HistoryFiles);
    commit_matched(&f.store, &owner, &jid, &rec).unwrap();
    assert_eq!(f.row(&jid).state, State::Matched);
    assert!(!f.pending(42));
    let stored: Value =
        serde_json::from_str(f.row(&jid).receipt_record_json.as_deref().unwrap()).unwrap();
    assert_eq!(stored["attribution"], "unproven");
    assert!(stored["posted"]["response_sha256"].is_null());
}

#[test]
fn unique_index_identity_admission_and_lock_boundaries() {
    let f = Fixture::new();
    let admission = AdmissionLock::try_acquire(&f.paths.identity_dir, 42).unwrap();
    let (jid, owner) = create(&f.store, &f.paths.identity_dir, &admission, &f.opts(42)).unwrap();
    assert!(matches!(
        create(&f.store, &f.paths.identity_dir, &admission, &f.opts(42)),
        Err(JournalError::InProgress)
    ));
    assert!(matches!(
        create(&f.store, &f.paths.identity_dir, &admission, &f.opts(43)),
        Err(JournalError::StateConflict)
    ));
    let mut bad = f.opts(42);
    bad.identity_key = "wrong".into();
    assert!(matches!(
        create(&f.store, &f.paths.identity_dir, &admission, &bad),
        Err(JournalError::Store(_))
    ));
    let other_id = uuid::Uuid::new_v4().to_string();
    let other = OwnerLock::acquire(&f.paths.identity_dir, &other_id).unwrap();
    assert!(matches!(
        transition(
            &f.store,
            &other,
            &jid,
            State::Planned,
            State::Refused,
            TransitionPatch::default()
        ),
        Err(JournalError::StateConflict)
    ));
    assert!(OwnerLock::try_acquire(&f.paths.identity_dir, "../outside").is_err());
    assert!(probe_owner(&f.paths.identity_dir, "../outside").is_err());
    let wrong_root = tempfile::tempdir().unwrap();
    let wrong_owner = OwnerLock::acquire(wrong_root.path(), &jid).unwrap();
    assert!(matches!(
        transition(
            &f.store,
            &wrong_owner,
            &jid,
            State::Planned,
            State::Refused,
            TransitionPatch::default()
        ),
        Err(JournalError::StateConflict)
    ));
    drop(owner);
}

#[cfg(unix)]
#[test]
fn lock_symlinks_are_refused_and_files_are_private() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = Fixture::new();
    let (jid, owner) = f.begin(42);
    let lock = f
        .paths
        .identity_dir
        .join("journals")
        .join(format!("{jid}.lock"));
    assert_eq!(
        std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(owner);
    let another = uuid::Uuid::new_v4().to_string();
    symlink(
        &lock,
        f.paths
            .identity_dir
            .join("journals")
            .join(format!("{another}.lock")),
    )
    .unwrap();
    assert!(OwnerLock::try_acquire(&f.paths.identity_dir, &another).is_err());
    assert!(probe_owner(&f.paths.identity_dir, &another).is_err());
    let root = tempfile::tempdir().unwrap();
    symlink(
        f.paths.identity_dir.join("journals"),
        root.path().join("journals"),
    )
    .unwrap();
    assert!(OwnerLock::try_acquire(root.path(), &jid).is_err());
}

#[test]
fn allowlist_retains_submitted_url_and_rejects_invalid_evidence() {
    let value = json!({"id":1,"attempt":1,"url":"https://assignment.test/?signature=intended", "attachments":[{"id":8,"url":"https://storage.test/?signature=SECRET"}]});
    let raw = serde_json::to_vec(&value).unwrap();
    let posted = allowlist_from_json(Evidence::PostResponse, &value, Some(&raw)).unwrap();
    assert_eq!(
        posted.url,
        Some("https://assignment.test/?signature=intended".into())
    );
    assert!(!serde_json::to_string(&posted).unwrap().contains("SECRET"));
    assert!(allowlist_from_json(Evidence::PostResponse, &json!({}), Some(b"{}")).is_err());
    assert!(allowlist_from_json(Evidence::PostResponse, &value, None).is_err());
    assert!(
        serde_json::from_value::<IntendedPayload>(json!({"upload_params":{"token":"SECRET"}}))
            .is_err()
    );
}

#[test]
fn candidate_clear_and_local_timestamps_round_trip() {
    let f = Fixture::new();
    let mut opts = f.opts(42);
    let mut intent: IntendedPayload = serde_json::from_str(&opts.intended_payload_json).unwrap();
    intent.time_zone = Some("America/New_York".into());
    opts.intended_payload_json = serde_json::to_string(&intent).unwrap();
    let admission = AdmissionLock::try_acquire(&f.paths.identity_dir, 42).unwrap();
    let (jid, owner) = create(&f.store, &f.paths.identity_dir, &admission, &opts).unwrap();
    transition(
        &f.store,
        &owner,
        &jid,
        State::Planned,
        State::Uploaded,
        TransitionPatch::default(),
    )
    .unwrap();
    mark_posting(&f.store, &owner, &jid).unwrap();
    transition(&f.store, &owner, &jid, State::Posting, State::OutcomeUnknown, TransitionPatch {
        server_match_json: Some(json!({"attempt":1,"submitted_at":"2026-09-10T03:12:44Z","attachment_ids":[],"url":"https://storage.test/?token=SECRET"}).to_string()), ..Default::default()
    }).unwrap();
    let candidate: Value =
        serde_json::from_str(f.row(&jid).server_match_json.as_deref().unwrap()).unwrap();
    assert_eq!(candidate["submitted_at_local"], "2026-09-09T23:12:44-04:00");
    assert!(candidate.get("url").is_none());
    transition(
        &f.store,
        &owner,
        &jid,
        State::OutcomeUnknown,
        State::OutcomeUnknown,
        TransitionPatch {
            server_match_json: Some("null".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(f.row(&jid).server_match_json.is_none());
    assert!(f.pending(42));
}

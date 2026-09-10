//! Adversarial M2-b lifecycle and evidence regressions.
use super::tests::{setup_identity, test_client};
use super::*;
use crate::identity::{IdentityDocument, Paths};
use crate::journal::*;
use crate::receipts::{ListFilter, list_journals, rebuild_from_journal};
use crate::store::Store;
use jiff::Timestamp;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn seed(
    paths: &Paths,
    store: &Store,
    doc: &IdentityDocument,
    files: bool,
) -> (String, OwnerLock, FrozenInput) {
    let frozen = if files {
        let path = paths.identity_dir.join("input");
        std::fs::write(&path, b"hello").unwrap();
        freeze_files(&[path], None).unwrap()
    } else {
        freeze_text(&TextSource::Bytes(b"hello"), None).unwrap()
    };
    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
    let (jid, owner) = create(
        store,
        &paths.identity_dir,
        &admission,
        &CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: 2,
            kind: frozen.kind.as_str().into(),
            intended_payload_json: serde_json::to_string(&frozen.payload).unwrap(),
            baseline_attempt: Some(0),
            baseline_submission_id: None,
        },
    )
    .unwrap();
    if files {
        transition(
            store,
            &owner,
            &jid,
            State::Planned,
            State::Uploading,
            TransitionPatch::default(),
        )
        .unwrap();
        append_uploaded_file_id(store, &owner, &jid, 0, 777).unwrap();
        transition(
            store,
            &owner,
            &jid,
            State::Uploading,
            State::Uploaded,
            TransitionPatch::default(),
        )
        .unwrap();
    } else {
        transition(
            store,
            &owner,
            &jid,
            State::Planned,
            State::Uploaded,
            TransitionPatch::default(),
        )
        .unwrap();
    }
    (jid, owner, frozen)
}
fn unknown(store: &Store, owner: &OwnerLock, jid: &str) {
    mark_posting(store, owner, jid).unwrap();
    transition(
        store,
        owner,
        jid,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch::default(),
    )
    .unwrap();
}
fn entry(attempt: i64, ids: &[i64]) -> Value {
    json!({"id":55,"attempt":attempt,"submitted_at":Timestamp::now().to_string(),"body":"<p>hello</p>",
        "attachments":ids.iter().map(|id| json!({"id":id,"display_name":"input","size":5})).collect::<Vec<_>>()})
}
async fn history(server: &MockServer, current: Value, entries: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2/submissions/self"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"attempt":current,"submission_history":entries})),
        )
        .mount(server)
        .await;
}
#[tokio::test]
async fn commit_then_error_matches_and_gateway_can_commit_later() {
    for status in [400, 500, 504] {
        let (_dir, paths, open, doc) = setup_identity();
        let server = MockServer::start().await;
        let client = test_client(&server);
        let (jid, owner, frozen) = seed(&paths, &open.store, &doc, true);
        Mock::given(method("POST"))
            .and(path("/api/v1/courses/1/assignments/2/submissions"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("x-request-id", "gateway-request")
                    .set_body_json(
                        json!({"status":"error","message":"upstream","error_report_id":1}),
                    ),
            )
            .expect(1)
            .mount(&server)
            .await;
        history(
            &server,
            json!(i32::from(status != 504)),
            if status == 504 {
                vec![]
            } else {
                vec![entry(1, &[777])]
            },
        )
        .await;
        let result = post_and_finish(&client, &open.store, &paths, &owner, &jid, &frozen)
            .await
            .unwrap();
        assert_eq!(result.post_status, Some(i64::from(status)));
        if status == 504 {
            assert_eq!(result.state, State::OutcomeUnknown);
            drop(owner);
            server.reset().await;
            history(&server, json!(1), vec![entry(1, &[777])]).await;
            assert_eq!(
                reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
                    .await
                    .unwrap()
                    .state,
                State::Matched
            );
        } else {
            assert_eq!(result.state, State::Matched);
            drop(owner);
        }
        let receipt = rebuild_from_journal(&open.store, &jid).unwrap();
        assert_eq!(receipt.attribution, "unproven");
        assert_eq!(receipt.posted.evidence, Evidence::HistoryFiles);
        assert_eq!(receipt.posted.response_sha256, None);
    }
}
#[tokio::test]
async fn file_candidates_require_exact_unique_sets() {
    for count in [0, 1, 2] {
        let (_dir, paths, open, doc) = setup_identity();
        let server = MockServer::start().await;
        let client = test_client(&server);
        let (jid, owner, _) = seed(&paths, &open.store, &doc, true);
        unknown(&open.store, &owner, &jid);
        drop(owner);
        let mut entries = vec![entry(9, &[888])];
        entries.extend((1..=count).map(|a| entry(a, &[777])));
        history(&server, json!(9), entries).await;
        let result = reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
            .await
            .unwrap();
        assert_eq!(
            result.state,
            if count == 1 {
                State::Matched
            } else {
                State::OutcomeUnknown
            }
        );
        assert_eq!(result.candidates.len(), usize::try_from(count).unwrap());
    }
}
#[tokio::test]
async fn text_evidence_clears_and_absence_precedes_filters() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);
    let (jid, owner, _) = seed(&paths, &open.store, &doc, false);
    unknown(&open.store, &owner, &jid);
    drop(owner);
    history(&server, json!(1), vec![entry(1, &[777])]).await;
    let result = reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
        .await
        .unwrap();
    assert_eq!(result.state, State::OutcomeUnknown);
    assert!(result.receipt_id.is_none());
    assert!(result.server_match.unwrap().attachment_ids.is_empty());
    server.reset().await;
    let mut old = entry(2, &[]);
    old["submitted_at"] = json!("2000-01-01T00:00:00Z");
    history(&server, json!(2), vec![old]).await;
    let later = Timestamp::now()
        .checked_add(std::time::Duration::from_secs(1801))
        .unwrap();
    let result = reconcile(&client, &open.store, &paths, &jid, false, later)
        .await
        .unwrap();
    assert!(result.server_match.is_none());
    assert!(!result.assume_available);
    assert!(!result.message.contains("no attempt is visible"));
    assert!(
        get_journal(&open.store, &jid)
            .unwrap()
            .unwrap()
            .server_match_json
            .is_none()
    );
    assert_eq!(
        reconcile(&client, &open.store, &paths, &jid, true, later)
            .await
            .unwrap()
            .outcome,
        ReconcileOutcome::Refused
    );
    server.reset().await;
    history(&server, Value::Null, vec![]).await;
    assert!(
        !reconcile(&client, &open.store, &paths, &jid, false, later)
            .await
            .unwrap()
            .assume_available
    );
    server.reset().await;
    history(&server, json!(0), vec![]).await;
    let result = reconcile(&client, &open.store, &paths, &jid, true, later)
        .await
        .unwrap();
    assert_eq!(result.state, State::UploadedNotSubmitted);
    assert_eq!(result.not_submitted_evidence.as_deref(), Some("assumed"));
}
#[tokio::test]
async fn reconcile_live_and_absent_owners_at_each_phase() {
    for phase in [
        State::Planned,
        State::Uploading,
        State::Uploaded,
        State::Posting,
        State::OutcomeUnknown,
    ] {
        let (_dir, paths, open, doc) = setup_identity();
        let server = MockServer::start().await;
        let client = test_client(&server);
        let (jid, owner, _) = seed(&paths, &open.store, &doc, false);
        // Test setup rewinds a row to each crash phase; production uses guarded transitions.
        let id = jid.clone();
        let state = phase.as_str().to_owned();
        open.store.call_blocking(move|c|{c.state.execute("UPDATE submission_journal SET state=?1, posting_started_at=?2 WHERE journal_id=?3",rusqlite::params![state,Timestamp::now().to_string(),id])?;Ok(())}).unwrap();
        let live = reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
            .await
            .unwrap();
        assert_eq!(live.outcome, ReconcileOutcome::Recovery);
        assert_eq!(live.owner, OwnerStatus::Live);
        let rows = list_journals(&open.store, &paths.identity_dir, &ListFilter::default()).unwrap();
        assert_eq!(rows[0].state, phase.as_str());
        assert!(server.received_requests().await.unwrap().is_empty());
        drop(owner);
        history(&server, json!(0), vec![]).await;
        let result = reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
            .await
            .unwrap();
        let expected = match phase {
            State::Planned => State::Refused,
            State::Uploading => State::UploadIncomplete,
            State::Uploaded => State::UploadedNotSubmitted,
            _ => State::OutcomeUnknown,
        };
        assert_eq!(result.state, expected);
        assert_eq!(
            result.outcome,
            if expected == State::OutcomeUnknown {
                ReconcileOutcome::Recovery
            } else {
                ReconcileOutcome::Refused
            }
        );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn commit_seed(store: &Store, owner: &OwnerLock, jid: &str, posted: Value) {
    mark_posting(store, owner, jid).unwrap();
    let raw = serde_json::to_vec(&posted).unwrap();
    commit_success(
        store,
        owner,
        jid,
        201,
        &ReceiptRecord {
            receipt_id: format!("r-{jid}"),
            journal_id: jid.into(),
            attribution: "observed".into(),
            posted: allowlist_from_json(Evidence::PostResponse, &posted, Some(&raw)).unwrap(),
            readback: None,
        },
    )
    .unwrap();
}
#[tokio::test]
async fn verify_binding_precedes_all_network_and_ignores_stale_exports() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);
    let (jid, owner, _) = seed(&paths, &open.store, &doc, true);
    commit_seed(&open.store, &owner, &jid, entry(1, &[777]));
    drop(owner);
    let receipt = rebuild_from_journal(&open.store, &jid).unwrap();
    for variant in 0..6 {
        let mut bad = receipt.clone();
        match variant {
            0 => bad.identity.user_id = "99".into(),
            1 => bad.journal_id = "missing".into(),
            2 => bad.files[0].canvas_file_id = None,
            3 => bad.posted.attempt = None,
            4 => bad.posted.attachments.clear(),
            _ => bad.course_id = "99".into(),
        }
        assert_eq!(
            verify(&client, &open.store, &paths, doc.key.as_str(), &bad)
                .await
                .unwrap()
                .outcome,
            VerifyOutcome::Refused
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    let export = crate::receipts::export(&open.store, &paths, &jid, None).unwrap();
    std::fs::write(export.path.unwrap(), b"corrupt stale snapshot").unwrap();
    assert_eq!(
        load_receipt_for_verify(&open.store, &paths, &receipt.receipt_id).unwrap(),
        receipt
    );
    assert!(load_receipt_for_verify(&open.store, &paths, "../../outside").is_err());
}
#[tokio::test]
async fn verify_file_hashes_sets_unavailable_and_containment() {
    use canvas_api::test_support::{TestServer, test_client};
    for scenario in [
        "match",
        "mismatch",
        "unavailable",
        "sets",
        "symlink",
        "expired",
    ] {
        let (_dir, paths, open, doc) = setup_identity();
        let server = TestServer::start().await;
        let client = test_client(&server);
        let (jid, owner, _) = seed(&paths, &open.store, &doc, true);
        commit_seed(&open.store, &owner, &jid, entry(1, &[777]));
        drop(owner);
        let receipt = rebuild_from_journal(&open.store, &jid).unwrap();
        let mut selected = entry(1, if scenario == "sets" { &[888] } else { &[777] });
        selected["attachments"][0]["url"] = json!(format!("{}/download", server.uri()));
        let storage = TestServer::start().await;
        if scenario == "expired" {
            selected["attachments"][0]["url"] = json!(format!("{}/expired", storage.uri()));
            Mock::given(path("/expired"))
                .respond_with(ResponseTemplate::new(403))
                .expect(1)
                .mount(&storage)
                .await;
            Mock::given(path("/api/v1/files/777")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":777,"display_name":"input","url":format!("{}/download",server.uri())}))).expect(1).mount(&server).await;
        }
        history(&server, json!(2), vec![selected, entry(2, &[999])]).await;
        Mock::given(path("/download"))
            .respond_with(if scenario == "unavailable" {
                ResponseTemplate::new(404)
            } else {
                ResponseTemplate::new(200).set_body_bytes(if scenario == "mismatch" {
                    b"other"
                } else {
                    b"hello"
                })
            })
            .mount(&server)
            .await;
        #[cfg(unix)]
        let outside = if scenario == "symlink" {
            let d = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(d.path(), paths.identity_dir.join("tmp")).unwrap();
            Some(d)
        } else {
            None
        };
        let result = verify(&client, &open.store, &paths, doc.key.as_str(), &receipt)
            .await
            .unwrap();
        let expected = match scenario {
            "mismatch" | "sets" => VerifyOutcome::Mismatch,
            "unavailable" => VerifyOutcome::Unavailable,
            "symlink" if cfg!(unix) => VerifyOutcome::Unavailable,
            _ => VerifyOutcome::Verified,
        };
        assert_eq!(result.outcome, expected, "{scenario}");
        #[cfg(unix)]
        if let Some(outside) = outside {
            assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        }
        if scenario != "symlink" {
            assert_eq!(
                std::fs::read_dir(paths.identity_dir.join("tmp")).map_or(0, Iterator::count),
                0
            );
        }
        assert_eq!(result.attempt, Some(1));
    }
}
#[tokio::test]
async fn text_verify_and_readback_retry_are_attempt_bound_and_idempotent() {
    for (body, expected) in [
        (Some("<p>hello</p>"), VerifyOutcome::VerifiedBody),
        (Some("changed"), VerifyOutcome::Mismatch),
        (None, VerifyOutcome::Unavailable),
    ] {
        let (_dir, paths, open, doc) = setup_identity();
        let server = MockServer::start().await;
        let client = test_client(&server);
        let (jid, owner, _) = seed(&paths, &open.store, &doc, false);
        let mut posted = entry(1, &[]);
        posted.as_object_mut().unwrap().remove("body");
        commit_seed(&open.store, &owner, &jid, posted);
        drop(owner);
        let initial = rebuild_from_journal(&open.store, &jid).unwrap();
        let mut selected = entry(1, &[]);
        selected["body"] = json!(body);
        history(&server, json!(2), vec![selected, entry(2, &[999])]).await;
        let before = verify(&client, &open.store, &paths, doc.key.as_str(), &initial)
            .await
            .unwrap();
        assert_eq!(before.outcome, VerifyOutcome::Unavailable);
        assert_eq!(
            before.reason.as_deref(),
            Some("no server body digest recorded")
        );
        reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
            .await
            .unwrap();
        let enriched = rebuild_from_journal(&open.store, &jid).unwrap();
        let count = server.received_requests().await.unwrap().len();
        reconcile(&client, &open.store, &paths, &jid, false, Timestamp::now())
            .await
            .unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), count);
        let mut reference = enriched.clone();
        reference.text.as_mut().unwrap().server_body_sha256 =
            Some(super::freeze::hex_sha256(b"<p>hello</p>"));
        assert_eq!(
            verify(&client, &open.store, &paths, doc.key.as_str(), &reference)
                .await
                .unwrap()
                .outcome,
            expected
        );
        assert!(enriched.readback.unwrap().attachments.is_empty());
    }
}

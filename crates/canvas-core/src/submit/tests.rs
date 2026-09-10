//! Wiremock integration tests for submit / reconcile.

use canvas_api::{Client, GovernorConfig, Secret};
use jiff::Timestamp;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::identity::{IdentityDocument, Paths};
use crate::journal::{
    AdmissionLock, CreateOpts, ResponseKind, State, TransitionPatch, append_uploaded_file_id,
    create, get_journal, transition,
};
use crate::store::OpenIdentity;
use crate::submit::freeze::{TextSource, freeze_files, freeze_text, validate_comment};
use crate::submit::preflight::{create_from_plan, preflight};
use crate::submit::reconcile::{ReconcileOutcome, reconcile};
use crate::submit::{InputKind, execute, post_and_finish};

fn setup_identity() -> (tempfile::TempDir, Paths, OpenIdentity, IdentityDocument) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(dir.path(), &doc.key);
    std::fs::create_dir_all(&paths.identity_dir).unwrap();
    std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    (dir, paths, open, doc)
}

fn test_client(server: &MockServer) -> Client {
    let origin = server.uri().parse().expect("mock uri");
    Client::with_governor(
        origin,
        Secret::new("tok"),
        "canvas-cli/test",
        GovernorConfig {
            jitter: false,
            ..Default::default()
        },
    )
    .unwrap()
}

fn assignment_json() -> serde_json::Value {
    json!({
        "id": 2,
        "name": "HW",
        "submission_types": ["online_upload", "online_text_entry", "online_url"],
        "can_submit": true,
        "submission": { "attempt": 0 },
        "allowed_attempts": -1
    })
}

#[test]
fn comment_too_long_is_validation() {
    let long = "x".repeat(65_536);
    assert!(validate_comment(Some(&long)).is_err());
}

#[tokio::test]
async fn assume_refused_before_30_minutes() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("GET"))
        .and(path_regex(
            r"/api/v1/courses/1/assignments/2/submissions/self",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 9,
            "attempt": 0,
            "submission_history": []
        })))
        .mount(&server)
        .await;

    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
    let (jid, owner) = create(
        &open.store,
        &paths.identity_dir,
        &admission,
        &CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: 2,
            kind: "online_upload".into(),
            intended_payload_json: json!({
                "files": [{"name":"a.pdf","size":1,"sha256":"aa","canvas_file_id":"7"}]
            })
            .to_string(),
            baseline_attempt: Some(0),
            baseline_submission_id: None,
        },
    )
    .unwrap();
    drop(admission);
    transition(
        &open.store,
        &owner,
        &jid,
        State::Planned,
        State::Uploaded,
        TransitionPatch::default(),
    )
    .unwrap();
    crate::journal::mark_posting(&open.store, &owner, &jid).unwrap();
    transition(
        &open.store,
        &owner,
        &jid,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch {
            response_kind: Some(ResponseKind::Other),
            post_status: Some(Some(500)),
            ..TransitionPatch::default()
        },
    )
    .unwrap();
    drop(owner);

    let now = Timestamp::now();
    let result = reconcile(&client, &open.store, &paths, &jid, true, now)
        .await
        .unwrap();
    assert_eq!(result.outcome, ReconcileOutcome::Refused);
    assert!(result.message.contains("30 minutes") || result.message.contains("cannot assume"));
}

#[tokio::test]
async fn outcome_unknown_on_500() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(assignment_json()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "errors": [{"message": "internal"}]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(
            r"/api/v1/courses/1/assignments/2/submissions/self",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 9,
            "attempt": 0,
            "submission_history": []
        })))
        .mount(&server)
        .await;

    let frozen = freeze_text(&TextSource::Bytes(b"hello"), None).unwrap();
    let now = Timestamp::now();
    let outcome = preflight(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        1,
        2,
        frozen.clone(),
        now,
    )
    .await
    .unwrap();
    let (jid, owner) = create_from_plan(
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &outcome.admission,
        &outcome.plan,
    )
    .unwrap();
    drop(outcome.admission);

    let out = execute(&client, &open.store, &paths, &owner, &jid, &frozen)
        .await
        .unwrap();
    assert_eq!(out.state, State::OutcomeUnknown);
    assert_eq!(out.response_kind, Some(ResponseKind::CanvasError));
    assert_eq!(out.post_status, Some(500));
}

#[tokio::test]
async fn successful_text_submit_path() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(assignment_json()))
        .mount(&server)
        .await;

    let body_html = "<p>hello</p>";
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 55,
            "attempt": 1,
            "submitted_at": "2026-01-01T00:00:00Z",
            "workflow_state": "submitted",
            "late": false,
            "missing": false,
            "submission_type": "online_text_entry",
            "body": body_html,
            "attachments": []
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(
            r"/api/v1/courses/1/assignments/2/submissions/self",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 55,
            "attempt": 1,
            "submission_history": [{
                "id": 55,
                "attempt": 1,
                "submitted_at": "2026-01-01T00:00:00Z",
                "late": false,
                "body": body_html,
                "attachments": []
            }]
        })))
        .mount(&server)
        .await;

    let frozen = freeze_text(&TextSource::Bytes(b"hello"), None).unwrap();
    let now = Timestamp::now();
    let outcome = preflight(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        1,
        2,
        frozen.clone(),
        now,
    )
    .await
    .unwrap();
    let (jid, owner) = create_from_plan(
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &outcome.admission,
        &outcome.plan,
    )
    .unwrap();
    drop(outcome.admission);

    let out = execute(&client, &open.store, &paths, &owner, &jid, &frozen)
        .await
        .unwrap();
    assert_eq!(out.state, State::Submitted);
    assert_eq!(out.attribution.as_deref(), Some("observed"));
    assert!(out.receipt_id.is_some());
    let row = get_journal(&open.store, &jid).unwrap().unwrap();
    assert!(row.readback_record_json.is_some());
    assert!(row.receipt_record_json.is_some());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn successful_file_submit_path() {
    let (_dir, paths, open, doc) = setup_identity();
    let server = MockServer::start().await;
    let client = test_client(&server);

    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(assignment_json()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 55,
            "attempt": 1,
            "submitted_at": "2026-01-01T00:00:00Z",
            "workflow_state": "submitted",
            "late": false,
            "missing": false,
            "submission_type": "online_upload",
            "attachments": [{
                "id": 777,
                "display_name": "hw.pdf",
                "size": 9,
                "content_type": "application/pdf"
            }]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(
            r"/api/v1/courses/1/assignments/2/submissions/self",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 55,
            "attempt": 1,
            "submission_history": [{
                "id": 55,
                "attempt": 1,
                "submitted_at": "2026-01-01T00:00:00Z",
                "late": false,
                "attachments": [{
                    "id": 777,
                    "display_name": "hw.pdf",
                    "size": 9,
                    "content_type": "application/pdf"
                }]
            }]
        })))
        .mount(&server)
        .await;

    let tmp = paths.identity_dir.join("hw.pdf");
    std::fs::write(&tmp, b"pdf-bytes").unwrap();
    let frozen = freeze_files(&[tmp], None).unwrap();
    let now = Timestamp::now();
    let outcome = preflight(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        1,
        2,
        frozen.clone(),
        now,
    )
    .await
    .unwrap();
    assert_eq!(outcome.plan.kind, InputKind::OnlineUpload);
    let (jid, owner) = create_from_plan(
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &outcome.admission,
        &outcome.plan,
    )
    .unwrap();
    drop(outcome.admission);

    // Seed a successful upload without HTTPS storage: planned → uploading → append → uploaded.
    transition(
        &open.store,
        &owner,
        &jid,
        State::Planned,
        State::Uploading,
        TransitionPatch::default(),
    )
    .unwrap();
    append_uploaded_file_id(&open.store, &owner, &jid, 0, 777).unwrap();
    transition(
        &open.store,
        &owner,
        &jid,
        State::Uploading,
        State::Uploaded,
        TransitionPatch::default(),
    )
    .unwrap();

    let out = post_and_finish(&client, &open.store, &paths, &owner, &jid, &frozen)
        .await
        .unwrap();
    assert_eq!(out.state, State::Submitted);
    assert_eq!(out.attribution.as_deref(), Some("observed"));
    assert!(out.receipt_id.is_some());
    let row = get_journal(&open.store, &jid).unwrap().unwrap();
    assert!(row.receipt_record_json.is_some());
}

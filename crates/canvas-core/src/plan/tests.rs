//! Plan, approval, and execute acceptance tests (REPORT §3.5).

use canvas_api::{Client, GovernorConfig, Secret};
use jiff::Timestamp;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::identity::{IdentityDocument, Paths};
use crate::journal::{
    AdmissionLock, CreateOpts, ResponseKind, State, TransitionPatch, create, get_journal,
    mark_posting, transition,
};
use crate::store::OpenIdentity;
use crate::submit::{FreezeError, FrozenInput, TextSource, freeze_files, freeze_text};

use super::record::PlanState;
use super::{
    Admission, ApprovalChannel, HandleRefusal, PlanError, PrepareRequest, approve, execute,
    invalidate, issue_handle, prepare, require,
};

pub(super) fn setup() -> (tempfile::TempDir, Paths, OpenIdentity, IdentityDocument) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(dir.path(), &doc.key);
    std::fs::create_dir_all(&paths.identity_dir).unwrap();
    std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    (dir, paths, open, doc)
}

pub(super) fn test_client(origin: &str) -> Client {
    Client::with_governor(
        origin.parse().expect("origin"),
        Secret::new("tok"),
        "canvas-cli/test",
        GovernorConfig {
            jitter: false,
            ..Default::default()
        },
    )
    .unwrap()
}

/// The assignment every plan here is prepared against.
fn assignment_json() -> Value {
    json!({
        "id": 2,
        "name": "HW",
        "submission_types": ["online_text_entry", "online_upload"],
        "can_submit": true,
        "submission": { "attempt": 0 },
        "allowed_attempts": -1
    })
}

async fn mount_assignment(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn request<'a>(paths: &'a Paths, doc: &'a IdentityDocument) -> PrepareRequest<'a> {
    PrepareRequest {
        identity_dir: &paths.identity_dir,
        identity_key: doc.key.as_str(),
        consumer: None,
        course_id: 1,
        assignment_id: 2,
        course_code: None,
        kind: crate::submit::InputKind::OnlineTextEntry,
    }
}

fn text_input() -> impl FnOnce() -> Result<FrozenInput, FreezeError> + Send + 'static {
    move || {
        let bytes = b"hello".to_vec();
        freeze_text(&TextSource::Bytes(&bytes), None)
    }
}

/// Whether the mock server saw anything that could change Canvas.
async fn saw_a_write(server: &MockServer) -> bool {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|r| r.method != wiremock::http::Method::GET)
}

#[tokio::test]
async fn approval_channel_and_digest_reach_the_journal_and_the_receipt() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let body_html = "<p>hello</p>";
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/2/submissions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 55, "attempt": 1, "submitted_at": "2026-01-01T00:00:00Z",
            "workflow_state": "submitted", "late": false, "missing": false,
            "submission_type": "online_text_entry", "body": body_html, "attachments": []
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"/api/v1/courses/1/assignments/2/submissions/self",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 55, "attempt": 1,
            "submission_history": [{
                "id": 55, "attempt": 1, "submitted_at": "2026-01-01T00:00:00Z",
                "late": false, "body": body_html, "attachments": []
            }]
        })))
        .mount(&server)
        .await;

    let now = Timestamp::now();
    let prepared = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap();
    let plan_id = prepared.plan.plan_id.clone();
    let handle = issue_handle(&open.store, &plan_id, None).unwrap();
    let approved = approve(
        &open.store,
        &plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .unwrap();
    assert_eq!(approved.state, PlanState::Approved);
    assert_eq!(
        approved.approval.as_ref().unwrap().plan_sha256,
        prepared.plan.plan_sha256
    );

    let Admission::Created {
        journal_id,
        owner,
        frozen,
        ..
    } = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    )
    .await
    .unwrap()
    else {
        panic!("a fresh plan must create a journal");
    };

    // The journal row carries the audit before anything is uploaded.
    let row = get_journal(&open.store, &journal_id).unwrap().unwrap();
    assert_eq!(row.plan_id.as_deref(), Some(plan_id.as_str()));
    let audit: Value = serde_json::from_str(row.approval_json.as_deref().unwrap()).unwrap();
    assert_eq!(audit["channel"], "tty");
    assert_eq!(audit["plan_sha256"], prepared.plan.plan_sha256);
    assert_eq!(
        require(&open.store, &plan_id).unwrap().state,
        PlanState::Executed
    );

    let out = crate::submit::execute(&client, &open.store, &paths, &owner, &journal_id, &frozen)
        .await
        .unwrap();
    assert_eq!(out.state, State::Submitted);

    // And so does the exported receipt.
    let exported = crate::receipts::export(&open.store, &paths, &journal_id, None).unwrap();
    let doc_json: Value =
        serde_json::from_slice(&std::fs::read(exported.path.unwrap()).unwrap()).unwrap();
    assert_eq!(doc_json["plan_id"], plan_id.as_str());
    assert_eq!(doc_json["approval"]["channel"], "tty");
    assert_eq!(
        doc_json["approval"]["plan_sha256"],
        prepared.plan.plan_sha256
    );
}

#[tokio::test]
async fn expired_invalidated_and_unapproved_plans_refuse_before_any_write() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let now = Timestamp::now();

    let refuse = |plan_id: String, at: Timestamp| {
        let client = &client;
        let store = &open.store;
        let dir = &paths.identity_dir;
        let key = doc.key.to_string();
        async move {
            let error = execute(client, store, dir, &key, &plan_id, at)
                .await
                .expect_err("must refuse");
            error.refusal_reason().unwrap().to_owned()
        }
    };

    // Never approved.
    let unapproved = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap();
    assert_eq!(
        refuse(unapproved.plan.plan_id.clone(), now).await,
        "approval_required"
    );

    // Approved, then left past its admission deadline.
    let stale = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap();
    let handle = issue_handle(&open.store, &stale.plan.plan_id, None).unwrap();
    approve(
        &open.store,
        &stale.plan.plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .unwrap();
    assert_eq!(
        refuse(stale.plan.plan_id.clone(), now + super::EXPIRY).await,
        "expired"
    );

    // Approved, then invalidated (a decline or a cancel).
    let cancelled = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap();
    let handle = issue_handle(&open.store, &cancelled.plan.plan_id, None).unwrap();
    approve(
        &open.store,
        &cancelled.plan.plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .unwrap();
    invalidate(&open.store, &cancelled.plan.plan_id, "declined").unwrap();
    assert_eq!(
        refuse(cancelled.plan.plan_id.clone(), now).await,
        "invalidated"
    );

    assert!(!saw_a_write(&server).await, "no upload and no post");
    let journals: i64 = open
        .store
        .call_blocking(|c| {
            Ok(c.state
                .query_row("SELECT COUNT(*) FROM submission_journal", [], |r| r.get(0))?)
        })
        .unwrap();
    assert_eq!(journals, 0, "a refusal never creates a journal");
}

/// Approve a fresh plan and return its id.
async fn approved_plan(
    client: &Client,
    open: &OpenIdentity,
    paths: &Paths,
    doc: &IdentityDocument,
    now: Timestamp,
) -> String {
    let prepared = prepare(client, &open.store, &request(paths, doc), text_input(), now)
        .await
        .unwrap();
    let plan_id = prepared.plan.plan_id.clone();
    let handle = issue_handle(&open.store, &plan_id, None).unwrap();
    approve(
        &open.store,
        &plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .unwrap();
    plan_id
}

#[tokio::test]
async fn every_changed_observation_invalidates_the_plan() {
    // Each row changes exactly one observed fact between prepare and execute.
    let changes: [(&str, Value); 10] = [
        ("group_category_id", json!({"group_category_id": 9})),
        (
            "submission_types",
            json!({"submission_types": ["online_text_entry", "online_upload", "online_url"]}),
        ),
        ("allowed_extensions", json!({"allowed_extensions": ["pdf"]})),
        ("can_submit", json!({"can_submit": false})),
        ("locked_for_user", json!({"locked_for_user": true})),
        ("allowed_attempts", json!({"allowed_attempts": 3})),
        (
            "extra_attempts",
            json!({"submission": {"attempt": 0, "extra_attempts": 1}}),
        ),
        ("unlock_at", json!({"unlock_at": "2020-01-01T00:00:00Z"})),
        ("lock_at", json!({"lock_at": "2099-01-01T00:00:00Z"})),
        ("due_at", json!({"due_at": "2099-06-01T00:00:00Z"})),
    ];
    for (field, change) in changes {
        let (_dir, paths, open, doc) = setup();
        let server = MockServer::start().await;
        let client = test_client(&server.uri());
        mount_assignment(&server, assignment_json()).await;
        let now = Timestamp::now();
        let plan_id = approved_plan(&client, &open, &paths, &doc, now).await;

        let mut fresh = assignment_json();
        for (key, value) in change.as_object().unwrap() {
            fresh[key] = value.clone();
        }
        server.reset().await;
        mount_assignment(&server, fresh).await;

        let error = execute(
            &client,
            &open.store,
            &paths.identity_dir,
            doc.key.as_str(),
            &plan_id,
            now,
        )
        .await
        .expect_err("a changed fact must be refused");
        assert_eq!(error.refusal_reason(), Some("invalidated"), "{field}");
        assert!(error.to_string().contains(field), "{field}: {error}");
        assert_eq!(
            require(&open.store, &plan_id).unwrap().state,
            PlanState::Invalidated,
            "{field}"
        );
        assert!(!saw_a_write(&server).await, "{field}");
    }
}

#[tokio::test]
async fn changed_bytes_and_a_changed_generation_invalidate_the_plan() {
    // Changed file bytes.
    let (dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let now = Timestamp::now();
    let file = dir.path().join("essay.txt");
    std::fs::write(&file, b"first draft").unwrap();
    let upload = PrepareRequest {
        kind: crate::submit::InputKind::OnlineUpload,
        ..request(&paths, &doc)
    };
    let path_for_freeze = file.clone();
    let prepared = prepare(
        &client,
        &open.store,
        &upload,
        move || freeze_files(&[path_for_freeze], None),
        now,
    )
    .await
    .unwrap();
    let plan_id = prepared.plan.plan_id.clone();
    let handle = issue_handle(&open.store, &plan_id, None).unwrap();
    approve(
        &open.store,
        &plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .unwrap();
    std::fs::write(&file, b"a different essay entirely").unwrap();
    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    )
    .await
    .expect_err("changed bytes must be refused");
    assert_eq!(error.refusal_reason(), Some("invalidated"));
    assert!(error.to_string().contains("essay.txt"), "{error}");
    assert!(!saw_a_write(&server).await);

    // Changed identity generation.
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let plan_id = approved_plan(&client, &open, &paths, &doc, now).await;
    open.store
        .call_blocking(|c| {
            c.state.execute(
                "UPDATE identity SET value = ?1 WHERE key = 'generation'",
                [uuid::Uuid::new_v4().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    )
    .await
    .expect_err("a replaced identity must be refused");
    assert_eq!(error.refusal_reason(), Some("invalidated"));
    assert!(error.to_string().contains("identity generation"), "{error}");
    assert!(!saw_a_write(&server).await);
}

#[tokio::test]
async fn an_unknown_outcome_is_never_reposted_by_the_plan_path() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;

    // An earlier attempt whose POST outcome was never observed.
    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
    let (unknown, owner) = create(
        &open.store,
        &paths.identity_dir,
        &admission,
        &CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: 2,
            kind: "online_text_entry".into(),
            intended_payload_json: json!({"text": {
                "input_sha256": "aa", "transform": "html", "sent_sha256": "bb",
                "outbound_bytes": "<p>hello</p>"
            }})
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
        &unknown,
        State::Planned,
        State::Uploaded,
        TransitionPatch::default(),
    )
    .unwrap();
    mark_posting(&open.store, &owner, &unknown).unwrap();
    transition(
        &open.store,
        &owner,
        &unknown,
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
    let before = get_journal(&open.store, &unknown).unwrap().unwrap();

    let now = Timestamp::now();
    let plan_id = approved_plan(&client, &open, &paths, &doc, now).await;
    let Admission::Created { journal_id, .. } = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    )
    .await
    .unwrap() else {
        panic!("a fresh plan must create its own journal");
    };

    assert_ne!(journal_id, unknown, "the unknown journal is never reused");
    let after = get_journal(&open.store, &unknown).unwrap().unwrap();
    assert_eq!(after.state, State::OutcomeUnknown);
    assert_eq!(after.post_status, before.post_status);
    assert_eq!(after.plan_id, None, "a legacy journal keeps a null plan_id");
    assert!(!saw_a_write(&server).await, "execute posts nothing itself");

    // The replayed approval returns the journal it already made.
    let replay = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    )
    .await
    .unwrap();
    assert!(matches!(replay, Admission::Existing { journal_id: id } if id == journal_id));
}

#[tokio::test]
async fn wrong_reused_and_foreign_handles_are_refused() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let now = Timestamp::now();

    let a = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap()
    .plan;
    let b = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap()
    .plan;

    let refusal = |plan: &str, handle: &str, consumer: Option<&str>| match approve(
        &open.store,
        plan,
        handle,
        ApprovalChannel::Elicitation,
        consumer,
        now,
    ) {
        Err(PlanError::Handle(refusal)) => refusal,
        other => panic!("expected a handle refusal, got {other:?}"),
    };

    // A handle nobody issued.
    assert_eq!(
        refusal(&a.plan_id, "0123456789abcdef0123456789abcdef", None),
        HandleRefusal::Unknown
    );
    // A handle issued for another plan.
    let for_b = issue_handle(&open.store, &b.plan_id, None).unwrap();
    assert_eq!(refusal(&a.plan_id, &for_b, None), HandleRefusal::OtherPlan);
    // A handle issued to another consumer.
    let for_consumer = issue_handle(&open.store, &a.plan_id, Some("mcp")).unwrap();
    assert_eq!(
        refusal(&a.plan_id, &for_consumer, None),
        HandleRefusal::WrongConsumer
    );
    // A handle already spent; the losing side of a race sees exactly this.
    let spent = issue_handle(&open.store, &a.plan_id, None).unwrap();
    let used = spent.clone();
    open.store
        .call_blocking(move |c| {
            c.state.execute(
                "UPDATE approval_handles SET used_at = '2026-01-01T00:00:00Z' WHERE handle = ?1",
                [&used],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        refusal(&a.plan_id, &spent, None),
        HandleRefusal::AlreadyUsed
    );
    // A handle past its deadline.
    let stale = issue_handle(&open.store, &a.plan_id, None).unwrap();
    let expired = stale.clone();
    open.store
        .call_blocking(move |c| {
            c.state.execute(
                "UPDATE approval_handles SET expires_at = '2020-01-01T00:00:00Z' WHERE handle = ?1",
                [&expired],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(refusal(&a.plan_id, &stale, None), HandleRefusal::Expired);

    // None of that approved anything.
    assert_eq!(
        require(&open.store, &a.plan_id).unwrap().state,
        PlanState::Prepared
    );
    assert!(require(&open.store, &a.plan_id).unwrap().approval.is_none());

    // The right handle still works, once.
    let good = issue_handle(&open.store, &a.plan_id, None).unwrap();
    let approved = approve(
        &open.store,
        &a.plan_id,
        &good,
        ApprovalChannel::Panel,
        None,
        now,
    )
    .unwrap();
    assert_eq!(approved.approval.unwrap().channel, ApprovalChannel::Panel);
}

#[tokio::test]
async fn declining_and_cancelling_spend_the_handles_and_refuse_execute() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    mount_assignment(&server, assignment_json()).await;
    let now = Timestamp::now();

    let declined = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap()
    .plan;
    let handle = issue_handle(&open.store, &declined.plan_id, None).unwrap();
    let after = super::decline(&open.store, &declined.plan_id).unwrap();
    assert_eq!(after.state, PlanState::Invalidated);
    assert_eq!(after.invalidated_reason.as_deref(), Some("declined"));

    // The handle issued for it is spent, and the plan cannot be approved.
    let error = approve(
        &open.store,
        &declined.plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        now,
    )
    .expect_err("a declined plan cannot be approved");
    assert_eq!(error.refusal_reason(), Some("invalidated"));
    let used: Option<String> = open
        .store
        .call_blocking(move |c| {
            Ok(c.state.query_row(
                "SELECT used_at FROM approval_handles WHERE handle = ?1",
                [&handle],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(used.is_some(), "declining spends the handle");

    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &declined.plan_id,
        now,
    )
    .await
    .expect_err("a declined plan is never executed");
    assert_eq!(error.refusal_reason(), Some("invalidated"));

    let cancelled = prepare(
        &client,
        &open.store,
        &request(&paths, &doc),
        text_input(),
        now,
    )
    .await
    .unwrap()
    .plan;
    let after = super::cancel(&open.store, &cancelled.plan_id).unwrap();
    assert_eq!(after.invalidated_reason.as_deref(), Some("cancelled"));
    assert!(!saw_a_write(&server).await);
}

#[tokio::test]
async fn a_decline_during_the_revalidation_read_is_named_as_the_decline() {
    // The plan is read once before the revalidation `GET` and again under
    // admission. A decline that lands in between must be reported as what it
    // is, and must not reach the guarded link and read as a lost race.
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(assignment_json())
                .set_delay(std::time::Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    let now = Timestamp::now();
    let plan_id = approved_plan(&client, &open, &paths, &doc, now).await;

    let running = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &plan_id,
        now,
    );
    let declining = async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        super::decline(&open.store, &plan_id).unwrap();
    };
    let (answer, ()) = tokio::join!(running, declining);

    let error = answer.expect_err("a declined plan is never executed");
    assert_eq!(error.refusal_reason(), Some("invalidated"));
    assert_eq!(error.to_string(), "declined");
    let journals: i64 = open
        .store
        .call_blocking(|c| {
            Ok(c.state
                .query_row("SELECT COUNT(*) FROM submission_journal", [], |r| r.get(0))?)
        })
        .unwrap();
    assert_eq!(journals, 0, "a refusal never creates a journal");
    assert!(!saw_a_write(&server).await);
}

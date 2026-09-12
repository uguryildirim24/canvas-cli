//! Operation acceptance tests (REPORT §4 M8-b).
//!
//! Every case here is one row of the acceptance column: what a plan freezes,
//! which prepares are refused before anything is sent, what a journal may
//! claim, and what an interrupted write recovers to.

use canvas_api::{Client, GovernorConfig, Secret};
use jiff::Timestamp;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::identity::{IdentityDocument, Paths};
use crate::journal::OwnerLock;
use crate::plan::{ApprovalChannel, PlanKind, PlanState, approve, issue_handle, require};
use crate::store::{OpenIdentity, PendingTarget, Store, pending_operations};

use super::{
    Admitted, Attribution, DiscussionReplyRequest, InboxSendRequest, NotPostedEvidence, OpState,
    OperationError, OperationTarget, PreparedOperation, QuizSubmitRequest, Verdict, execute, post,
    prepare_discussion_reply, prepare_inbox_send, prepare_quiz_submit, reconcile, status,
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

/// A topic anyone enrolled may reply to.
pub(super) fn open_topic() -> Value {
    json!({
        "id": 55,
        "title": "Week 3 reading",
        "locked": false,
        "locked_for_user": false,
        "require_initial_post": false,
        "user_can_see_posts": true,
        "group_category_id": null,
        "group_topic_children": []
    })
}

fn entry(id: i64) -> Value {
    json!({
        "id": id,
        "user_id": 7,
        "message": "<p>Existing.</p>",
        "created_at": "2026-09-09T12:00:00Z"
    })
}

async fn mount_get(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route.to_owned()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// The reads every discussion prepare makes.
pub(super) async fn mount_topic(server: &MockServer, topic: Value) {
    mount_get(server, "/api/v1/courses/5/discussion_topics/55", topic).await;
    mount_get(
        server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([entry(900)]),
    )
    .await;
}

/// The reads every inbox prepare makes.
pub(super) async fn mount_inbox(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/search/recipients"))
        .and(query_param("user_id", "31"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id": 31, "name": "Alex Kim"}])),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search/recipients"))
        .and(query_param("user_id", "999"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
    mount_get(
        server,
        "/api/v1/conversations/700",
        json!({
            "id": 700,
            "subject": "Lab partner",
            "participants": [{"id": 7, "name": "You"}, {"id": 31, "name": "Alex Kim"}],
            "messages": []
        }),
    )
    .await;
}

pub(super) fn reply_request<'a>(
    paths: &'a Paths,
    doc: &'a IdentityDocument,
    body: &'a str,
) -> DiscussionReplyRequest<'a> {
    DiscussionReplyRequest {
        identity_dir: &paths.identity_dir,
        identity_key: doc.key.as_str(),
        consumer: None,
        course_id: 5,
        course_code: Some("CHEM-101"),
        topic_id: 55,
        parent_entry_id: None,
        body,
        attachments: &[],
    }
}

fn send_request<'a>(
    paths: &'a Paths,
    doc: &'a IdentityDocument,
    recipients: &'a [String],
    body: &'a str,
) -> InboxSendRequest<'a> {
    InboxSendRequest {
        identity_dir: &paths.identity_dir,
        identity_key: doc.key.as_str(),
        consumer: None,
        recipients,
        subject: Some("Lab partner"),
        body,
        attachments: &[],
    }
}

/// Prepare one discussion reply and approve it.
/// Approve a prepared plan through a freshly issued handle.
pub(super) fn approve_plan(store: &Store, plan_id: &str) {
    let handle = issue_handle(store, plan_id, None).unwrap();
    approve(
        store,
        plan_id,
        &handle,
        ApprovalChannel::YesFlag,
        None,
        Timestamp::now(),
    )
    .unwrap();
}

pub(super) async fn approved_reply(
    client: &Client,
    store: &Store,
    paths: &Paths,
    doc: &IdentityDocument,
) -> PreparedOperation {
    let prepared = prepare_discussion_reply(
        client,
        store,
        &reply_request(paths, doc, "My reply."),
        Timestamp::now(),
    )
    .await
    .unwrap();
    let handle = issue_handle(store, &prepared.plan.plan_id, None).unwrap();
    approve(
        store,
        &prepared.plan.plan_id,
        &handle,
        ApprovalChannel::YesFlag,
        None,
        Timestamp::now(),
    )
    .unwrap();
    prepared
}

fn reason(error: &OperationError) -> &str {
    error.refusal_reason().unwrap_or("<not a refusal>")
}

/// A quiz this identity may answer: untimed, two attempts, going back allowed.
pub(super) fn answerable_quiz() -> Value {
    json!({
        "id": 77,
        "title": "Week 3 Reading Quiz",
        "description": "<p>Chapters 4 and 5.</p>",
        "quiz_type": "assignment",
        "time_limit": null,
        "allowed_attempts": 2,
        "question_count": 2,
        "points_possible": 6.0,
        "cant_go_back": false,
        "one_question_at_a_time": false,
        "require_lockdown_browser": false,
        "published": true,
        "unlocked_for_user": true,
        "locked_for_user": false
    })
}

/// A live session on attempt 1, holding the token answering needs.
pub(super) fn live_quiz_submission() -> Value {
    json!({
        "quiz_submissions": [{
            "id": 601,
            "quiz_id": 77,
            "user_id": 7,
            "submission_id": 46001,
            "started_at": "2026-09-11T14:00:00Z",
            "finished_at": null,
            "end_at": null,
            "attempt": 1,
            "validation_token": "tok-1",
            "workflow_state": "untaken",
            "attempts_left": 2
        }]
    })
}

/// Two censored questions, unanswered.
pub(super) fn quiz_questions() -> Value {
    json!({
        "quiz_submission_questions": [
            {
                "id": 901,
                "flagged": false,
                "answer": null,
                "position": 1,
                "question_name": "Question 1",
                "question_type": "multiple_choice_question",
                "question_text": "<p>Which reagent?</p>",
                "points_possible": 2.0,
                "answers": [
                    {"id": 4811, "text": "Phenolphthalein"},
                    {"id": 4812, "text": "Litmus"}
                ]
            },
            {
                "id": 902,
                "flagged": false,
                "answer": null,
                "position": 2,
                "question_name": "Question 2",
                "question_type": "essay_question",
                "question_text": "<p>Name two sources of error.</p>",
                "points_possible": 4.0,
                "answers": null
            }
        ]
    })
}

/// The reads every quiz prepare makes.
pub(super) async fn mount_quiz(server: &MockServer) {
    mount_get(server, "/api/v1/courses/5/quizzes/77", answerable_quiz()).await;
    mount_get(
        server,
        "/api/v1/courses/5/quizzes/77/submission",
        live_quiz_submission(),
    )
    .await;
    mount_get(
        server,
        "/api/v1/quiz_submissions/601/questions",
        quiz_questions(),
    )
    .await;
}

/// The answers half of the write: the session questions with each recorded
/// answer echoed back.
pub(super) async fn mount_quiz_answers(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/v1/quiz_submissions/601/questions"))
        .respond_with(ResponseTemplate::new(200).set_body_json({
            let mut questions = quiz_questions();
            questions["quiz_submission_questions"][0]["answer"] = json!(4811);
            questions["quiz_submission_questions"][1]["answer"] =
                json!("A wet flask dilutes the titrant.");
            questions
        }))
        .mount(server)
        .await;
}

/// The completion half of the write: the session graded and closed.
pub(super) async fn mount_quiz_complete(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/5/quizzes/77/submissions/601/complete",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quiz_submissions": [{
                "id": 601,
                "quiz_id": 77,
                "user_id": 7,
                "submission_id": 46001,
                "started_at": "2026-09-11T14:00:00Z",
                "finished_at": "2026-09-11T14:21:00Z",
                "attempt": 1,
                "workflow_state": "complete",
                "score": 6.0,
                "kept_score": 6.0
            }]
        })))
        .mount(server)
        .await;
}

pub(super) fn quiz_request<'a>(
    paths: &'a Paths,
    doc: &'a IdentityDocument,
) -> QuizSubmitRequest<'a> {
    // A static answer set is enough: the request borrows it, and the two
    // questions below are the whole session.
    static ANSWERS: std::sync::OnceLock<Vec<canvas_api::models::QuizAnswer>> =
        std::sync::OnceLock::new();
    let answers = ANSWERS.get_or_init(|| {
        vec![
            canvas_api::models::QuizAnswer {
                id: 901,
                answer: json!(4811),
            },
            canvas_api::models::QuizAnswer {
                id: 902,
                answer: json!("A wet flask dilutes the titrant."),
            },
        ]
    });
    QuizSubmitRequest {
        identity_dir: &paths.identity_dir,
        identity_key: doc.key.as_str(),
        consumer: None,
        course_id: 5,
        course_code: Some("CHEM-101"),
        quiz_id: 77,
        access_code: None,
        answers,
    }
}

pub(super) async fn approved_quiz(
    client: &Client,
    store: &Store,
    paths: &Paths,
    doc: &IdentityDocument,
) -> PreparedOperation {
    let prepared = prepare_quiz_submit(client, store, &quiz_request(paths, doc), Timestamp::now())
        .await
        .unwrap();
    let handle = issue_handle(store, &prepared.plan.plan_id, None).unwrap();
    approve(
        store,
        &prepared.plan.plan_id,
        &handle,
        ApprovalChannel::YesFlag,
        None,
        Timestamp::now(),
    )
    .unwrap();
    prepared
}

// --------------------------------------------------------- what a plan freezes

#[tokio::test]
async fn a_plan_freezes_the_thread_the_body_and_nothing_else() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    let prepared = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "Hello, everyone."),
        Timestamp::now(),
    )
    .await
    .unwrap();

    assert_eq!(prepared.plan.kind, PlanKind::DiscussionReply);
    assert_eq!(prepared.plan.state, PlanState::Prepared);
    assert_eq!(
        prepared.operation.target,
        OperationTarget::DiscussionReply {
            course_id: 5,
            topic_id: 55,
            parent_entry_id: None
        }
    );
    // A discussion body is sent as HTML; the two digests are of different
    // bytes, and both are in the plan.
    assert_eq!(prepared.operation.body.transform, "text-to-html");
    assert_ne!(
        prepared.operation.body.input_sha256,
        prepared.operation.body.sent_sha256
    );
    assert_eq!(
        prepared.operation.labels.topic_title.as_deref(),
        Some("Week 3 reading")
    );

    // The plan digest covers the frozen operation, so the stored row still
    // matches the digest that an approval would bind.
    let stored = require(&open.store, &prepared.plan.plan_id).unwrap();
    assert_eq!(stored.digest(), stored.plan_sha256);
    assert_eq!(stored.operation.as_ref().unwrap(), &prepared.operation);
}

#[tokio::test]
async fn a_send_freezes_exact_recipients_and_drops_a_repeat() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    let client = test_client(&server.uri());

    let recipients = vec!["31".to_owned(), " 31 ".to_owned()];
    let prepared = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "Are you free Tuesday?"),
        Timestamp::now(),
    )
    .await
    .unwrap();

    assert_eq!(
        prepared.operation.target,
        OperationTarget::InboxSend {
            recipients: vec!["31".to_owned()]
        }
    );
    // A conversation body is sent verbatim: Canvas renders it itself.
    assert_eq!(prepared.operation.body.transform, "plain");
    assert_eq!(
        prepared.operation.body.input_sha256,
        prepared.operation.body.sent_sha256
    );
    assert_eq!(prepared.operation.labels.recipients, ["Alex Kim"]);
    assert_eq!(prepared.operation.subject.as_deref(), Some("Lab partner"));
}

#[tokio::test]
async fn an_attachment_is_frozen_by_its_bytes_and_changed_bytes_invalidate_the_plan() {
    let (dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    let client = test_client(&server.uri());

    let file = dir.path().join("note.txt");
    std::fs::write(&file, b"first").unwrap();
    let recipients = vec!["31".to_owned()];
    let attachments = vec![file.clone()];
    let mut request = send_request(&paths, &doc, &recipients, "See the note.");
    request.attachments = &attachments;
    let prepared = prepare_inbox_send(&client, &open.store, &request, Timestamp::now())
        .await
        .unwrap();
    assert_eq!(prepared.operation.attachments.len(), 1);
    assert_eq!(prepared.operation.attachments[0].size, 5);

    let handle = issue_handle(&open.store, &prepared.plan.plan_id, None).unwrap();
    approve(
        &open.store,
        &prepared.plan.plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        Timestamp::now(),
    )
    .unwrap();

    // The bytes change after the approval. Execute re-hashes from disk.
    std::fs::write(&file, b"second").unwrap();
    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .expect_err("changed bytes must invalidate the plan");
    assert_eq!(reason(&error), "invalidated");
    // And nothing was created: no journal, and the plan can never run again.
    assert!(
        super::for_plan(&open.store, &prepared.plan.plan_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        require(&open.store, &prepared.plan.plan_id).unwrap().state,
        PlanState::Invalidated
    );
}

// --------------------------------------------------------- prepare refusals

#[tokio::test]
async fn a_group_discussion_is_refused_before_anything_is_sent() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let mut topic = open_topic();
    topic["group_category_id"] = json!(770);
    mount_topic(&server, topic).await;
    let client = test_client(&server.uri());

    let error = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "Hello."),
        Timestamp::now(),
    )
    .await
    .expect_err("a group discussion is refused");
    assert_eq!(reason(&error), "group_write");
    assert!(no_post_was_made(&server).await);
}

#[tokio::test]
async fn a_locked_topic_is_refused() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let mut topic = open_topic();
    topic["locked_for_user"] = json!(true);
    mount_topic(&server, topic).await;
    let client = test_client(&server.uri());

    let error = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "Hello."),
        Timestamp::now(),
    )
    .await
    .expect_err("a locked topic is refused");
    assert_eq!(reason(&error), "locked");
    assert!(no_post_was_made(&server).await);
}

/// The gate is never opened by a placeholder: no `POST` is made at all.
#[tokio::test]
async fn an_initial_post_gate_is_refused_and_nothing_is_posted() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let mut topic = open_topic();
    topic["require_initial_post"] = json!(true);
    topic["user_can_see_posts"] = json!(false);
    mount_topic(&server, topic).await;
    let client = test_client(&server.uri());

    let error = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "Hello."),
        Timestamp::now(),
    )
    .await
    .expect_err("a gated topic is refused");
    assert_eq!(reason(&error), "initial_post_required");
    assert!(no_post_was_made(&server).await);
}

#[tokio::test]
async fn an_entry_outside_the_topic_is_unresolved() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    let mut request = reply_request(&paths, &doc, "Hello.");
    request.parent_entry_id = Some(4242);
    let error = prepare_discussion_reply(&client, &open.store, &request, Timestamp::now())
        .await
        .expect_err("an entry outside the topic is refused");
    assert_eq!(reason(&error), "unresolved");
}

#[tokio::test]
async fn an_empty_body_is_refused() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    let error = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "   \n  "),
        Timestamp::now(),
    )
    .await
    .expect_err("an empty body is refused");
    assert_eq!(reason(&error), "empty_body");
}

#[tokio::test]
async fn an_unknown_recipient_is_unresolved() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    let client = test_client(&server.uri());

    let recipients = vec!["999".to_owned()];
    let error = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "Hello."),
        Timestamp::now(),
    )
    .await
    .expect_err("an unknown recipient is refused");
    assert_eq!(reason(&error), "unresolved");
}

#[tokio::test]
async fn a_topic_this_identity_cannot_see_is_denied() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/discussion_topics/55"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"errors": []})))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());

    let error = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "Hello."),
        Timestamp::now(),
    )
    .await
    .expect_err("a topic this identity cannot see is refused");
    assert_eq!(reason(&error), "denied");
}

#[tokio::test]
async fn a_discussion_attachment_is_refused_as_unsupported() {
    let (dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    let file = dir.path().join("note.txt");
    std::fs::write(&file, b"x").unwrap();
    let attachments = vec![file];
    let mut request = reply_request(&paths, &doc, "Hello.");
    request.attachments = &attachments;
    let error = prepare_discussion_reply(&client, &open.store, &request, Timestamp::now())
        .await
        .expect_err("a discussion attachment is refused");
    assert_eq!(reason(&error), "unsupported");
}

// ------------------------------------------------------------- the dispatch

/// Whether the mock server ever saw a `POST`.
async fn no_post_was_made(server: &MockServer) -> bool {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .all(|request| request.method.as_str() != "POST")
}

async fn mount_reply_post(server: &MockServer, status: u16, body: Value) {
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/5/discussion_topics/55/entries"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

fn posted_entry() -> Value {
    json!({
        "id": 5003,
        "user_id": 7,
        "created_at": "2026-09-10T14:02:11Z",
        "message": "<p>My reply.</p>"
    })
}

#[tokio::test]
async fn nothing_is_dispatched_without_a_recorded_approval() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    let client = test_client(&server.uri());

    let prepared = prepare_discussion_reply(
        &client,
        &open.store,
        &reply_request(&paths, &doc, "My reply."),
        Timestamp::now(),
    )
    .await
    .unwrap();

    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .expect_err("a prepared plan cannot execute");
    assert_eq!(reason(&error), "approval_required");
    assert!(no_post_was_made(&server).await);
}

#[tokio::test]
async fn a_yes_flag_approval_is_recorded_as_itself() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let approval = require(&open.store, &prepared.plan.plan_id)
        .unwrap()
        .approval
        .expect("an approval");
    assert_eq!(approval.channel, ApprovalChannel::YesFlag);
    assert_eq!(approval.plan_sha256, prepared.plan.plan_sha256);
}

#[tokio::test]
async fn a_two_hundred_that_names_an_entry_is_accepted_and_never_more() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let posted = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    assert_eq!(posted.state, OpState::Posted);
    assert_eq!(posted.attribution, Attribution::Accepted);
    assert_eq!(posted.delivery(), "observable");
    assert_eq!(
        posted.response.as_ref().and_then(|r| r.id.clone()),
        Some("5003".to_owned())
    );
    // The body itself never reaches the journal; only its digest does.
    let stored = serde_json::to_string(&posted.response).unwrap();
    assert!(!stored.contains("My reply"));
    // And the receipt says what it can claim.
    let receipt = posted.receipt.as_ref().expect("a receipt");
    assert_eq!(receipt["operation"]["delivery"], "observable");
    assert_eq!(receipt["attribution"], "accepted");
}

#[tokio::test]
async fn an_acceptance_of_a_conversation_is_never_reported_as_delivery() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/conversations"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!([{
            "id": 700,
            "subject": "Lab partner",
            "messages": [{"id": 8100, "author_id": 7, "created_at": "2026-09-10T14:02:11Z"}]
        }])))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());

    let recipients = vec!["31".to_owned()];
    let prepared = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "Are you free Tuesday?"),
        Timestamp::now(),
    )
    .await
    .unwrap();
    let handle = issue_handle(&open.store, &prepared.plan.plan_id, None).unwrap();
    approve(
        &open.store,
        &prepared.plan.plan_id,
        &handle,
        ApprovalChannel::Elicitation,
        None,
        Timestamp::now(),
    )
    .unwrap();
    let posted = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    assert_eq!(posted.state, OpState::Posted);
    assert_eq!(posted.attribution, Attribution::Accepted);
    // Canvas accepted a conversation. It never says a person received it.
    assert_eq!(posted.delivery(), "not_observable");
    let receipt = posted.receipt.as_ref().expect("a receipt");
    assert_eq!(receipt["operation"]["delivery"], "not_observable");
}

#[tokio::test]
async fn a_non_two_hundred_leaves_the_journal_failed_and_nothing_posted() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 422, json!({"errors": [{"message": "no"}]})).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let posted = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    assert_eq!(posted.state, OpState::Failed);
    assert_eq!(posted.attribution, Attribution::None);
    assert_eq!(posted.post_status, Some(422));
}

#[tokio::test]
async fn a_second_execute_returns_the_same_journal_and_posts_once() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let first = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    let again = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    match again {
        Admitted::Existing { journal_id } => assert_eq!(journal_id, first.journal_id),
        Admitted::Created { .. } => panic!("a plan admitted a second journal"),
    }
    assert_eq!(posts_seen(&server).await, 1);
}

/// Expiry gates first admission only, never a plan that already has a journal.
#[tokio::test]
async fn expiry_is_checked_at_admission_only() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let first = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    // An hour later the plan is long expired, and the journal still answers.
    let later = Timestamp::now() + jiff::SignedDuration::from_hours(1);
    let again = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        later,
    )
    .await
    .unwrap();
    match again {
        Admitted::Existing { journal_id } => assert_eq!(journal_id, first.journal_id),
        Admitted::Created { .. } => panic!("an expired plan admitted a journal"),
    }

    // A plan that never admitted one is refused at that same moment.
    let fresh = approved_reply(&client, &open.store, &paths, &doc).await;
    let error = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &fresh.plan.plan_id,
        later,
    )
    .await
    .expect_err("an expired plan is refused at admission");
    assert_eq!(reason(&error), "expired");
}

async fn posts_seen(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.method.as_str() == "POST")
        .count()
}

/// Admit and post one approved plan, and return the journal row.
pub(super) async fn run(
    client: &Client,
    store: &Store,
    paths: &Paths,
    doc: &IdentityDocument,
    plan_id: &str,
) -> super::OperationRow {
    let admitted = execute(
        client,
        store,
        &paths.identity_dir,
        doc.key.as_str(),
        plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    let Admitted::Created { journal_id, owner } = admitted else {
        panic!("the plan admitted no journal");
    };
    let posted = post(client, store, &owner, &journal_id).await.unwrap();
    *posted.row
}

// -------------------------------------------------- the unknown outcome

/// A response that never arrives leaves `outcome_unknown`, and nothing resends.
#[tokio::test]
async fn an_ambiguous_timeout_stays_unknown_and_is_never_resent() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    // A Canvas-shaped 500 proves nothing: SPEC §12.2 records that Canvas can
    // answer one after it has already committed.
    mount_reply_post(&server, 500, json!({"errors": [{"message": "internal"}]})).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let unknown = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(unknown.state, OpState::OutcomeUnknown);
    assert_eq!(unknown.attribution, Attribution::None);

    // The pending hook reports it, and a second execute makes no request.
    let pending = open
        .store
        .call_blocking(move |conns| pending_operations(&conns.state, PendingTarget::Topic(55)))
        .unwrap();
    assert_eq!(pending, std::slice::from_ref(&unknown.journal_id));

    let before = posts_seen(&server).await;
    let again = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    match again {
        Admitted::Existing { journal_id } => assert_eq!(journal_id, unknown.journal_id),
        Admitted::Created { .. } => panic!("an unknown outcome admitted a second journal"),
    }
    assert_eq!(posts_seen(&server).await, before);
}

/// A later write never retires an earlier unknown one (M8-b review).
///
/// A submission is superseded by a later attempt on the same assignment. A
/// reply and a message are not: a second reply is a second post, and a second
/// conversation is a second conversation. A rule keyed on the target columns
/// made every accepted `inbox_send` supersede every other unresolved send,
/// because a send has no target column at all until Canvas answers.
#[tokio::test]
async fn a_later_write_never_supersedes_an_unknown_one() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    // The first send is answered by a 500, which proves nothing.
    Mock::given(method("POST"))
        .and(path("/api/v1/conversations"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"errors": []})))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let client = test_client(&server.uri());
    let recipients = vec!["31".to_owned()];
    let first = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "Are you free Tuesday?"),
        Timestamp::now(),
    )
    .await
    .unwrap();
    approve_plan(&open.store, &first.plan.plan_id);
    let unknown = run(&client, &open.store, &paths, &doc, &first.plan.plan_id).await;
    assert_eq!(unknown.state, OpState::OutcomeUnknown);

    // A second, unrelated send to the same person is accepted.
    Mock::given(method("POST"))
        .and(path("/api/v1/conversations"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!([{
            "id": 701,
            "messages": [{"id": 8200, "author_id": 7, "created_at": "2026-09-10T15:00:00Z"}]
        }])))
        .mount(&server)
        .await;
    let second = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "One more thing."),
        Timestamp::now(),
    )
    .await
    .unwrap();
    approve_plan(&open.store, &second.plan.plan_id);
    let posted = run(&client, &open.store, &paths, &doc, &second.plan.plan_id).await;
    assert_eq!(posted.state, OpState::Posted);

    // The first one is still unresolved, on every hook that names it.
    let unknown_id = unknown.journal_id.clone();
    let pending = open
        .store
        .call_blocking(move |conns| pending_operations(&conns.state, PendingTarget::Inbox))
        .unwrap();
    assert!(
        pending.contains(&unknown_id),
        "an accepted send retired an unknown one: {pending:?}"
    );
    assert_eq!(
        super::ops::pending(&open.store).unwrap()[0],
        unknown.journal_id
    );
    assert!(!super::ops::is_superseded());

    // Acknowledging it is the only thing that clears it.
    super::ops::acknowledge(&open.store, &unknown.journal_id).unwrap();
    let cleared = open
        .store
        .call_blocking(move |conns| pending_operations(&conns.state, PendingTarget::Inbox))
        .unwrap();
    assert!(
        cleared.is_empty(),
        "an acknowledged send is the only one that clears: {cleared:?}"
    );
}

/// A readback that finds the accepted object upgrades `accepted` to `observed`.
#[tokio::test]
async fn a_readback_that_finds_the_entry_makes_the_claim_observed() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let posted = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(posted.attribution, Attribution::Accepted);

    // The thread now shows the entry Canvas named.
    server.reset().await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([entry(900), posted_entry()]),
    )
    .await;
    let read = status(
        &client,
        &open.store,
        &paths.identity_dir,
        &posted.journal_id,
    )
    .await
    .unwrap();
    assert_eq!(read.verdict, Verdict::Observed);
    assert_eq!(read.row.attribution, Attribution::Observed);
    assert_eq!(read.row.state, OpState::Posted);
}

/// A digest match with no id link is `unproven`, and its state is `matched`.
#[tokio::test]
async fn a_digest_match_with_no_id_link_is_unproven() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    // A Canvas-shaped 500 proves nothing: SPEC §12.2 records that Canvas can
    // answer one after it has already committed.
    mount_reply_post(&server, 500, json!({"errors": [{"message": "internal"}]})).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let unknown = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(unknown.state, OpState::OutcomeUnknown);

    // The thread holds an entry by this identity whose body digest matches,
    // and nothing links it to the request this process made.
    let sent = prepared.operation.body.sent_sha256.clone();
    server.reset().await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([{
            "id": 6100,
            "user_id": 7,
            "created_at": "2026-09-10T14:03:00Z",
            "message": "<p>My reply.</p>"
        }]),
    )
    .await;
    let resolved = reconcile(
        &test_client(&server.uri()),
        &open.store,
        &paths.identity_dir,
        &unknown.journal_id,
        false,
        Timestamp::now(),
    )
    .await
    .unwrap();

    assert_eq!(resolved.verdict, Verdict::Matched);
    assert_eq!(resolved.row.state, OpState::Matched);
    assert_eq!(resolved.row.attribution, Attribution::Unproven);
    let found = resolved.row.server_match.as_ref().expect("a server match");
    assert_eq!(found.id, "6100");
    assert_eq!(found.body_sha256, sent);
}

/// A threaded reply is read back where Canvas puts it (M8-b review).
///
/// `--to` posts to `…/entries/:eid/replies`, and Canvas lists only top-level
/// entries at `…/entries`. Reading the topic listing reported every threaded
/// reply as absent, so `accepted` could never become `observed` and an
/// unknown outcome could be asserted away on evidence that never covered it.
#[tokio::test]
async fn a_threaded_reply_is_read_back_under_its_parent_entry() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 200, posted_entry()).await;
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/5/discussion_topics/55/entries/900/replies",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(posted_entry()))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());

    let mut request = reply_request(&paths, &doc, "My reply.");
    request.parent_entry_id = Some(900);
    let prepared = prepare_discussion_reply(&client, &open.store, &request, Timestamp::now())
        .await
        .unwrap();
    approve_plan(&open.store, &prepared.plan.plan_id);
    let posted = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(posted.state, OpState::Posted);
    assert_eq!(posted.attribution, Attribution::Accepted);

    // The topic's own entry listing does not hold the nested reply; the
    // parent entry's replies do, and that is what the readback reads.
    server.reset().await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([entry(900)]),
    )
    .await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries/900/replies",
        json!([posted_entry()]),
    )
    .await;
    let seen = status(
        &test_client(&server.uri()),
        &open.store,
        &paths.identity_dir,
        &posted.journal_id,
    )
    .await
    .unwrap();
    assert_eq!(seen.verdict, Verdict::Observed);
    assert_eq!(seen.row.attribution, Attribution::Observed);
}

/// A readback that covered nothing cannot support "never posted" (review).
///
/// The exposed case is a send Canvas never named a conversation for: there is
/// no thread to read, so `complete` is false and the assertion is refused
/// however old the journal is (`docs/writes-v2.md` choice 8).
#[tokio::test]
async fn an_assumption_needs_a_readback_that_covered_the_thread() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_inbox(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/conversations"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"errors": []})))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());
    let recipients = vec!["31".to_owned()];
    let prepared = prepare_inbox_send(
        &client,
        &open.store,
        &send_request(&paths, &doc, &recipients, "Are you free Tuesday?"),
        Timestamp::now(),
    )
    .await
    .unwrap();
    approve_plan(&open.store, &prepared.plan.plan_id);
    let unknown = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(unknown.state, OpState::OutcomeUnknown);
    assert!(unknown.response.is_none(), "Canvas named no conversation");

    let late = reconcile(
        &client,
        &open.store,
        &paths.identity_dir,
        &unknown.journal_id,
        true,
        Timestamp::now() + super::ASSUME_AFTER + jiff::SignedDuration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(
        late.row.state,
        OpState::OutcomeUnknown,
        "an unread thread was accepted as proof of absence"
    );
    assert_ne!(late.verdict, Verdict::AssumedNotPosted);
    assert!(late.row.not_posted_evidence.is_none());
    assert!(
        late.warning
            .as_deref()
            .is_some_and(|w| w.contains("did not cover")),
        "{:?}",
        late.warning
    );
}

/// A digest another person wrote is not this journal's post (M8-b review).
///
/// `Attribution::Unproven` says the readback shows "a message whose digest
/// matches and whose author is this identity". The author half was not
/// checked, so a classmate who typed the same sentence resolved an unknown
/// outcome to `matched`, exit 0, with a receipt.
#[tokio::test]
async fn a_digest_written_by_somebody_else_resolves_nothing() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    mount_reply_post(&server, 500, json!({"errors": [{"message": "internal"}]})).await;
    let client = test_client(&server.uri());

    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let unknown = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;
    assert_eq!(unknown.state, OpState::OutcomeUnknown);

    // The same sentence is in the thread, under somebody else's name.
    server.reset().await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([{
            "id": 6100,
            "user_id": 31,
            "created_at": "2026-09-10T14:03:00Z",
            "message": "<p>My reply.</p>"
        }]),
    )
    .await;
    let resolved = reconcile(
        &test_client(&server.uri()),
        &open.store,
        &paths.identity_dir,
        &unknown.journal_id,
        false,
        Timestamp::now(),
    )
    .await
    .unwrap();

    assert_eq!(resolved.verdict, Verdict::NotFound);
    assert_eq!(resolved.row.state, OpState::OutcomeUnknown);
    assert_eq!(resolved.row.attribution, Attribution::None);
    assert!(resolved.row.server_match.is_none());
    assert!(resolved.row.receipt.is_none());
}

/// `--assume-not-posted` is refused before the wait, and honest after it.
#[tokio::test]
async fn assume_not_posted_is_refused_early_and_recorded_late() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    // A Canvas-shaped 500 proves nothing: SPEC §12.2 records that Canvas can
    // answer one after it has already committed.
    mount_reply_post(&server, 500, json!({"errors": [{"message": "internal"}]})).await;
    let client = test_client(&server.uri());
    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let unknown = run(&client, &open.store, &paths, &doc, &prepared.plan.plan_id).await;

    server.reset().await;
    mount_get(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([entry(900)]),
    )
    .await;
    let reader = test_client(&server.uri());

    // Too young: the journal does not move, and the caller is told why.
    let early = reconcile(
        &reader,
        &open.store,
        &paths.identity_dir,
        &unknown.journal_id,
        true,
        Timestamp::now(),
    )
    .await
    .unwrap();
    assert_eq!(early.row.state, OpState::OutcomeUnknown);
    assert!(early.warning.is_some());

    // After the wait, with a clean readback, it is recorded as never posted.
    let late = reconcile(
        &reader,
        &open.store,
        &paths.identity_dir,
        &unknown.journal_id,
        true,
        Timestamp::now() + super::ASSUME_AFTER + jiff::SignedDuration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(late.verdict, Verdict::AssumedNotPosted);
    assert_eq!(late.row.state, OpState::Refused);
    assert_eq!(
        late.row.not_posted_evidence,
        Some(NotPostedEvidence::Assumed)
    );
    // SPEC §12.2: the residual risk is stated, never left to be inferred.
    assert!(
        late.warning
            .as_deref()
            .is_some_and(|w| w.contains("two messages")),
        "{:?}",
        late.warning
    );
}

/// `operation status` recovers nothing (M8-b review).
///
/// SPEC §12.2 names the recoverers, and a readback is not one.
/// `docs/writes-v2.md` choice 9, the module doc, and the MCP tool description
/// all say `status` never changes state; it applied the owner-absent recovery
/// table anyway. `reconcile` still does.
#[tokio::test]
async fn status_leaves_an_abandoned_journal_where_it_found_it() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_topic(&server, open_topic()).await;
    let client = test_client(&server.uri());

    // A journal whose owner is gone, still `planned`: nothing left this
    // machine, but only a recoverer may say so.
    let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
    let Admitted::Created { journal_id, owner } = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap() else {
        panic!("the plan admitted no journal");
    };
    drop(owner);

    let seen = status(&client, &open.store, &paths.identity_dir, &journal_id)
        .await
        .unwrap();
    assert_eq!(
        seen.row.state,
        OpState::Planned,
        "status recovered a journal"
    );
    assert_eq!(
        super::ops::require(&open.store, &journal_id).unwrap().state,
        OpState::Planned
    );

    // Reconcile is the recoverer, and it says the honest thing.
    let resolved = reconcile(
        &client,
        &open.store,
        &paths.identity_dir,
        &journal_id,
        false,
        Timestamp::now(),
    )
    .await
    .unwrap();
    assert_eq!(resolved.row.state, OpState::Refused);
    assert_eq!(
        resolved.row.not_posted_evidence,
        Some(NotPostedEvidence::NeverSent)
    );
}

// ------------------------------------------------------- owner-absent recovery

/// Every non-terminal state recovers to the only answer it can honestly have.
#[test]
fn owner_absent_recovery_gives_each_state_its_honest_answer() {
    for (from, to, evidence) in [
        (
            OpState::Planned,
            OpState::Refused,
            Some(NotPostedEvidence::NeverSent),
        ),
        (OpState::Posting, OpState::OutcomeUnknown, None),
    ] {
        let (_dir, paths, open, doc) = setup();
        let journal_id = seeded(&open.store, &paths, &doc, from);

        // The owner is gone: no process holds the lock.
        let recovered =
            super::recover_if_owner_absent(&open.store, &paths.identity_dir, &journal_id)
                .unwrap()
                .expect("an abandoned journal recovers");
        assert_eq!(recovered, to, "{from} recovers to {to}");
        let row = super::require(&open.store, &journal_id).unwrap();
        assert_eq!(row.state, to);
        assert_eq!(row.not_posted_evidence, evidence);
    }
}

/// A live owner is never recovered under.
#[test]
fn a_live_owner_stops_recovery() {
    let (_dir, paths, open, doc) = setup();
    let journal_id = seeded(&open.store, &paths, &doc, OpState::Posting);
    let _owner = OwnerLock::try_acquire(&paths.identity_dir, &journal_id)
        .unwrap()
        .expect("the lock is free");

    let recovered =
        super::recover_if_owner_absent(&open.store, &paths.identity_dir, &journal_id).unwrap();
    assert_eq!(recovered, None);
    assert_eq!(
        super::require(&open.store, &journal_id).unwrap().state,
        OpState::Posting
    );
}

/// Insert one journal directly in a given state, with no owner holding it.
pub(super) fn seeded(
    store: &Store,
    paths: &Paths,
    doc: &IdentityDocument,
    state: OpState,
) -> String {
    let plan = crate::plan::insert_operation(
        store,
        crate::plan::NewOperationPlan {
            identity_key: doc.key.as_str().to_owned(),
            consumer: None,
            kind: PlanKind::DiscussionReply,
            course_id: 5,
            operation: super::OperationPlan {
                target: OperationTarget::DiscussionReply {
                    course_id: 5,
                    topic_id: 55,
                    parent_entry_id: None,
                },
                body: crate::journal::IntendedText {
                    input_sha256: "aa".repeat(32),
                    transform: "text-to-html".to_owned(),
                    sent_sha256: "bb".repeat(32),
                    outbound_bytes: "<p>x</p>".to_owned(),
                },
                subject: None,
                access_code: None,
                attachments: Vec::new(),
                labels: super::OperationLabels::default(),
            },
        },
        Timestamp::now(),
    )
    .unwrap();
    let handle = issue_handle(store, &plan.plan_id, None).unwrap();
    approve(
        store,
        &plan.plan_id,
        &handle,
        ApprovalChannel::Tty,
        None,
        Timestamp::now(),
    )
    .unwrap();
    let plan = require(store, &plan.plan_id).unwrap();

    let admission =
        crate::journal::AdmissionLock::try_acquire_named(&paths.identity_dir, "topic-55").unwrap();
    let (journal_id, owner) =
        super::create_linked(store, &paths.identity_dir, &admission, &plan).unwrap();
    drop(admission);
    if state == OpState::Posting {
        super::mark_posting(store, &owner, &journal_id).unwrap();
    }
    // Releasing the owner lock is what an absent owner looks like.
    drop(owner);
    journal_id
}

// ------------------------------------------------------- quiz submit (M10-a)

/// Preparing freezes the canonical answer set, not the caller's order.
#[tokio::test]
async fn quiz_prepare_freezes_the_canonical_answer_set() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_quiz(&server).await;
    let client = test_client(&server.uri());
    let request = QuizSubmitRequest {
        answers: &[
            canvas_api::models::QuizAnswer {
                id: 902,
                answer: json!("A wet flask dilutes the titrant."),
            },
            canvas_api::models::QuizAnswer {
                id: 901,
                answer: json!(4811),
            },
        ],
        ..quiz_request(&paths, &doc)
    };
    let prepared = prepare_quiz_submit(&client, &open.store, &request, Timestamp::now())
        .await
        .unwrap();
    assert_eq!(
        prepared.operation.target,
        OperationTarget::QuizSubmit {
            course_id: 5,
            quiz_id: 77,
            quiz_submission_id: 601,
            attempt: 1,
        }
    );
    // Canonical order, by question id, whatever order the caller gave.
    let frozen: Vec<canvas_api::models::QuizAnswer> =
        serde_json::from_str(&prepared.operation.body.outbound_bytes).unwrap();
    assert_eq!(
        frozen.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![901, 902]
    );
    assert_eq!(prepared.operation.body.transform, "json");
    assert_eq!(
        prepared.operation.labels.quiz_title.as_deref(),
        Some("Week 3 Reading Quiz")
    );
}

/// A locked quiz is refused before anything is frozen.
#[tokio::test]
async fn quiz_prepare_refuses_a_locked_quiz() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let mut quiz = answerable_quiz();
    quiz["locked_for_user"] = json!(true);
    quiz["lock_explanation"] = json!("the quiz closed at midnight");
    mount_get(&server, "/api/v1/courses/5/quizzes/77", quiz).await;
    let client = test_client(&server.uri());
    let error = prepare_quiz_submit(
        &client,
        &open.store,
        &quiz_request(&paths, &doc),
        Timestamp::now(),
    )
    .await
    .unwrap_err();
    assert_eq!(reason(&error), "locked");
}

/// No session in progress means there is nothing to answer.
#[tokio::test]
async fn quiz_prepare_refuses_without_a_session() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_get(&server, "/api/v1/courses/5/quizzes/77", answerable_quiz()).await;
    mount_get(
        &server,
        "/api/v1/courses/5/quizzes/77/submission",
        json!({ "quiz_submissions": [] }),
    )
    .await;
    let client = test_client(&server.uri());
    let error = prepare_quiz_submit(
        &client,
        &open.store,
        &quiz_request(&paths, &doc),
        Timestamp::now(),
    )
    .await
    .unwrap_err();
    assert_eq!(reason(&error), "no_session");
}

/// An answer for a question the session does not hold is unresolved.
#[tokio::test]
async fn quiz_prepare_refuses_an_unknown_question() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_quiz(&server).await;
    let client = test_client(&server.uri());
    let request = QuizSubmitRequest {
        answers: &[canvas_api::models::QuizAnswer {
            id: 9999,
            answer: json!(1),
        }],
        ..quiz_request(&paths, &doc)
    };
    let error = prepare_quiz_submit(&client, &open.store, &request, Timestamp::now())
        .await
        .unwrap_err();
    assert_eq!(reason(&error), "unresolved");
}

/// `LockDown` and one-question-at-a-time quizzes need the browser.
#[tokio::test]
async fn quiz_prepare_refuses_a_browser_only_quiz() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    let client = test_client(&server.uri());
    for (field, value) in [
        ("require_lockdown_browser", json!(true)),
        ("one_question_at_a_time", json!(true)),
        ("cant_go_back", json!(true)),
    ] {
        let mut quiz = answerable_quiz();
        quiz[field] = value;
        mount_get(&server, "/api/v1/courses/5/quizzes/77", quiz).await;
        let error = prepare_quiz_submit(
            &client,
            &open.store,
            &quiz_request(&paths, &doc),
            Timestamp::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(reason(&error), "unsupported", "{field}");
    }
}

/// The whole write: answers posted, quiz completed, session forgotten.
#[tokio::test]
async fn quiz_post_answers_and_completes() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_quiz(&server).await;
    mount_quiz_answers(&server).await;
    mount_quiz_complete(&server).await;
    let client = test_client(&server.uri());
    let prepared = approved_quiz(&client, &open.store, &paths, &doc).await;

    let admitted = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    let Admitted::Created { journal_id, owner } = admitted else {
        panic!("the plan admitted nothing");
    };
    let posted = post(&client, &open.store, &owner, &journal_id)
        .await
        .unwrap();
    assert_eq!(posted.row.state, OpState::Posted);
    assert_eq!(posted.row.attribution, Attribution::Accepted);
    assert_eq!(
        posted.row.response.as_ref().and_then(|r| r.id.clone()),
        Some("601".to_owned())
    );
    // The session is closed, so its token authorizes nothing anymore.
    assert!(
        crate::quiz::load_session(&open.store, doc.key.as_str(), 77).is_none(),
        "the session was not forgotten"
    );
}

/// A server error on the completion is unknown, and the readback resolves it.
///
/// The answers POST answered 200, then Canvas answered 500 after it may have
/// committed. The readback shows the completed session with the sent answers,
/// so reconcile moves the journal to `posted` with attribution `observed`.
#[tokio::test]
async fn quiz_reconcile_after_a_server_error_posts_observed() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_quiz(&server).await;
    mount_quiz_answers(&server).await;
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/5/quizzes/77/submissions/601/complete",
        ))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "status": "internal_server_error",
            "message": "unexpected",
            "error_report_id": 4242,
        })))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());
    let prepared = approved_quiz(&client, &open.store, &paths, &doc).await;
    let admitted = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    let Admitted::Created { journal_id, owner } = admitted else {
        panic!("the plan admitted nothing");
    };
    let posted = post(&client, &open.store, &owner, &journal_id)
        .await
        .unwrap();
    assert_eq!(posted.row.state, OpState::OutcomeUnknown);
    // Releasing the owner lock is what an absent owner looks like.
    drop(owner);

    // Canvas did commit: the session is complete with the sent answers.
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/quizzes/77/submission"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "quiz_submissions": [{
                "id": 601,
                "quiz_id": 77,
                "user_id": 7,
                "submission_id": 46001,
                "started_at": "2026-09-11T14:00:00Z",
                "finished_at": "2026-09-11T14:21:00Z",
                "attempt": 1,
                "workflow_state": "complete",
                "score": 6.0
            }]
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/quiz_submissions/601/questions"))
        .respond_with(ResponseTemplate::new(200).set_body_json({
            let mut questions = quiz_questions();
            questions["quiz_submission_questions"][0]["answer"] = json!(4811);
            questions["quiz_submission_questions"][1]["answer"] =
                json!("A wet flask dilutes the titrant.");
            questions
        }))
        .with_priority(1)
        .mount(&server)
        .await;
    let done = reconcile(
        &client,
        &open.store,
        &paths.identity_dir,
        &journal_id,
        false,
        Timestamp::now(),
    )
    .await
    .unwrap();
    assert_eq!(done.verdict, Verdict::Observed);
    assert_eq!(done.row.state, OpState::Posted);
    assert_eq!(done.row.attribution, Attribution::Observed);
}

/// Answers recorded on an open session do not close the journal.
#[tokio::test]
async fn quiz_status_with_an_open_session_stays_unknown() {
    let (_dir, paths, open, doc) = setup();
    let server = MockServer::start().await;
    mount_quiz(&server).await;
    mount_quiz_answers(&server).await;
    Mock::given(method("POST"))
        .and(path(
            "/api/v1/courses/5/quizzes/77/submissions/601/complete",
        ))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "status": "internal_server_error",
            "message": "unexpected",
            "error_report_id": 4242,
        })))
        .mount(&server)
        .await;
    let client = test_client(&server.uri());
    let prepared = approved_quiz(&client, &open.store, &paths, &doc).await;
    let admitted = execute(
        &client,
        &open.store,
        &paths.identity_dir,
        doc.key.as_str(),
        &prepared.plan.plan_id,
        Timestamp::now(),
    )
    .await
    .unwrap();
    let Admitted::Created { journal_id, owner } = admitted else {
        panic!("the plan admitted nothing");
    };
    post(&client, &open.store, &owner, &journal_id)
        .await
        .unwrap();
    // Releasing the owner lock is what an absent owner looks like.
    drop(owner);

    // The answers are recorded, but the session is still open.
    Mock::given(method("GET"))
        .and(path("/api/v1/quiz_submissions/601/questions"))
        .respond_with(ResponseTemplate::new(200).set_body_json({
            let mut questions = quiz_questions();
            questions["quiz_submission_questions"][0]["answer"] = json!(4811);
            questions["quiz_submission_questions"][1]["answer"] =
                json!("A wet flask dilutes the titrant.");
            questions
        }))
        .with_priority(1)
        .mount(&server)
        .await;
    let done = status(&client, &open.store, &paths.identity_dir, &journal_id)
        .await
        .unwrap();
    assert_eq!(done.row.state, OpState::OutcomeUnknown);
    assert_eq!(done.row.attribution, Attribution::Observed);
}

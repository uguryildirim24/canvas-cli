//! `prepare` for the three write operations (M8-b, REPORT §3.5).
//!
//! Preparing reads the target, refuses everything that must be refused before
//! anything is sent, freezes the exact bytes and the exact attachments, and
//! stores one `prepared` plan. It never posts, never uploads, and never holds
//! the target admission lock across a person's decision.
//!
//! Every refusal here happens **before** a journal exists, so a refused
//! prepare leaves nothing behind but the reason.

use std::path::{Path, PathBuf};

use canvas_api::{Client, Error as ApiError};
use jiff::Timestamp;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::journal::IntendedText;
use crate::plan::PlanRow;
use crate::store::Store;
use crate::submit::MAX_TEXT_BYTES;

use super::OperationError;
use super::record::{
    OpState, OperationAttachment, OperationLabels, OperationPlan, OperationTarget,
};

/// The largest attachment set one operation may carry.
///
/// Ten files, and one megabyte of body: the same bound `canvas submit` puts on
/// a text entry, so no surface can queue an unbounded upload behind one
/// approval.
const MAX_ATTACHMENTS: usize = 10;

/// A prepared operation and what preparing it observed.
#[derive(Debug, Clone)]
pub struct PreparedOperation {
    /// The stored `prepared` plan row.
    pub plan: PlanRow,
    /// The frozen operation, for the confirmation line.
    pub operation: OperationPlan,
    /// Abandoned journals for this target that preparing recovered.
    pub recovered: Vec<(String, OpState)>,
}

/// A refusal reason, named so a caller can print it without matching on text.
pub type Refusal = &'static str;

/// What `discussion reply` freezes.
#[derive(Debug, Clone)]
pub struct DiscussionReplyRequest<'a> {
    /// Identity directory holding the lock and journal files.
    pub identity_dir: &'a Path,
    /// Identity key the plan is bound to.
    pub identity_key: &'a str,
    /// Consumer that asked, when one did.
    pub consumer: Option<&'a str>,
    /// Course the topic belongs to.
    pub course_id: i64,
    /// Course code, for the confirmation line.
    pub course_code: Option<&'a str>,
    /// Topic to reply to.
    pub topic_id: i64,
    /// The entry to reply to, when `--to` named one.
    pub parent_entry_id: Option<i64>,
    /// The body, already read from the file, stdin, or `--text`.
    pub body: &'a str,
    /// Local attachment paths. Refused: this package does not upload to a topic.
    pub attachments: &'a [PathBuf],
}

/// What `inbox send` freezes.
#[derive(Debug, Clone)]
pub struct InboxSendRequest<'a> {
    /// Identity directory.
    pub identity_dir: &'a Path,
    /// Identity key.
    pub identity_key: &'a str,
    /// Consumer that asked, when one did.
    pub consumer: Option<&'a str>,
    /// Canvas user ids, in the order the caller gave them.
    pub recipients: &'a [String],
    /// Subject, when one was given.
    pub subject: Option<&'a str>,
    /// The body.
    pub body: &'a str,
    /// Local attachment paths.
    pub attachments: &'a [PathBuf],
}

/// What `inbox reply` freezes.
#[derive(Debug, Clone)]
pub struct InboxReplyRequest<'a> {
    /// Identity directory.
    pub identity_dir: &'a Path,
    /// Identity key.
    pub identity_key: &'a str,
    /// Consumer that asked, when one did.
    pub consumer: Option<&'a str>,
    /// The conversation to add a message to.
    pub conversation_id: i64,
    /// The body.
    pub body: &'a str,
    /// Local attachment paths.
    pub attachments: &'a [PathBuf],
}

/// What `quiz submit` freezes.
#[derive(Debug, Clone)]
pub struct QuizSubmitRequest<'a> {
    /// Identity directory holding the lock and journal files.
    pub identity_dir: &'a Path,
    /// Identity key the plan is bound to.
    pub identity_key: &'a str,
    /// Consumer that asked, when one did.
    pub consumer: Option<&'a str>,
    /// Course the quiz belongs to.
    pub course_id: i64,
    /// Course code, for the confirmation line.
    pub course_code: Option<&'a str>,
    /// The quiz to answer.
    pub quiz_id: i64,
    /// Access code for the quiz, when one protects it.
    pub access_code: Option<&'a str>,
    /// The answers, already parsed from the file or stdin.
    pub answers: &'a [canvas_api::models::QuizAnswer],
}

/// Freeze a discussion reply and store it as a `prepared` plan.
pub async fn prepare_discussion_reply(
    client: &Client,
    store: &Store,
    request: &DiscussionReplyRequest<'_>,
    now: Timestamp,
) -> Result<PreparedOperation, OperationError> {
    // A topic takes no attachment on this surface. Refusing at prepare keeps
    // the promise the plan makes: what is approved is what is sent.
    if !request.attachments.is_empty() {
        return Err(OperationError::refused(
            "unsupported",
            "a discussion reply cannot carry an attachment in this version",
        ));
    }
    let body = frozen_body(request.body, Transform::Html)?;

    let topic: Value = get(
        client,
        &format!(
            "/api/v1/courses/{}/discussion_topics/{}",
            request.course_id, request.topic_id
        ),
    )
    .await?;
    check_topic_writable(&topic)?;

    // `--to` must name an entry that is actually in this topic; a reply
    // addressed to something else is `unresolved`, never redirected.
    if let Some(entry_id) = request.parent_entry_id {
        let entries = entries_of(client, request.course_id, request.topic_id).await?;
        if !entries
            .iter()
            .any(|entry| json_id(entry.get("id")) == Some(entry_id.to_string()))
        {
            return Err(OperationError::refused(
                "unresolved",
                format!("entry {entry_id} is not in topic {}", request.topic_id),
            ));
        }
    }

    let operation = OperationPlan {
        target: OperationTarget::DiscussionReply {
            course_id: request.course_id,
            topic_id: request.topic_id,
            parent_entry_id: request.parent_entry_id,
        },
        body,
        subject: None,
        access_code: None,
        attachments: Vec::new(),
        labels: OperationLabels {
            course_code: request.course_code.map(str::to_owned),
            topic_title: topic
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..OperationLabels::default()
        },
    };
    store_plan(
        store,
        request.identity_dir,
        request.identity_key,
        request.consumer,
        operation,
        now,
    )
}

/// Freeze a new conversation and store it as a `prepared` plan.
pub async fn prepare_inbox_send(
    client: &Client,
    store: &Store,
    request: &InboxSendRequest<'_>,
    now: Timestamp,
) -> Result<PreparedOperation, OperationError> {
    let body = frozen_body(request.body, Transform::Plain)?;
    let attachments = freeze_attachments(request.attachments)?;
    if request.recipients.is_empty() {
        return Err(OperationError::refused(
            "unresolved",
            "at least one recipient is required",
        ));
    }

    // Deduplicate, keeping the order the caller gave: the plan freezes exact
    // recipients, and a repeated id must not become a repeated recipient.
    let mut recipients: Vec<String> = Vec::new();
    for raw in request.recipients {
        let id = raw.trim();
        if id.is_empty() || id.parse::<i64>().is_err() {
            return Err(OperationError::refused(
                "unresolved",
                format!("recipient {id:?} is not a Canvas user id"),
            ));
        }
        if !recipients.iter().any(|kept| kept == id) {
            recipients.push(id.to_owned());
        }
    }

    // Every recipient must be one this identity can message. An id Canvas
    // does not return is `unresolved`, and nothing is sent to the rest.
    let mut names = Vec::with_capacity(recipients.len());
    for id in &recipients {
        names.push(resolve_recipient(client, id).await?);
    }

    let operation = OperationPlan {
        target: OperationTarget::InboxSend {
            recipients: recipients.clone(),
        },
        body,
        subject: request.subject.map(str::to_owned).filter(|s| !s.is_empty()),
        access_code: None,
        attachments,
        labels: OperationLabels {
            recipients: names,
            ..OperationLabels::default()
        },
    };
    store_plan(
        store,
        request.identity_dir,
        request.identity_key,
        request.consumer,
        operation,
        now,
    )
}

/// Freeze a conversation reply and store it as a `prepared` plan.
pub async fn prepare_inbox_reply(
    client: &Client,
    store: &Store,
    request: &InboxReplyRequest<'_>,
    now: Timestamp,
) -> Result<PreparedOperation, OperationError> {
    let body = frozen_body(request.body, Transform::Plain)?;
    let attachments = freeze_attachments(request.attachments)?;

    let conversation: Value = get(
        client,
        &format!(
            "/api/v1/conversations/{}?auto_mark_as_read=false",
            request.conversation_id
        ),
    )
    .await?;

    let operation = OperationPlan {
        target: OperationTarget::InboxReply {
            conversation_id: request.conversation_id,
        },
        body,
        subject: None,
        access_code: None,
        attachments,
        labels: OperationLabels {
            conversation_subject: conversation
                .get("subject")
                .and_then(Value::as_str)
                .map(str::to_owned),
            recipients: participant_names(&conversation),
            ..OperationLabels::default()
        },
    };
    store_plan(
        store,
        request.identity_dir,
        request.identity_key,
        request.consumer,
        operation,
        now,
    )
}

/// Freeze quiz answers and store them as a `prepared` plan.
///
/// Preparing refuses everything the quiz itself refuses — locked, one
/// question at a time, no going back, `LockDown`, an IP filter — and every
/// answer id the live session does not hold. The body is the exact JSON of
/// the answers, so the approval binds the same bytes execute sends.
pub async fn prepare_quiz_submit(
    client: &Client,
    store: &Store,
    request: &QuizSubmitRequest<'_>,
    now: Timestamp,
) -> Result<PreparedOperation, OperationError> {
    let quiz: canvas_api::models::Quiz = match client
        .get(&format!(
            "/api/v1/courses/{}/quizzes/{}",
            request.course_id, request.quiz_id
        ))
        .await
    {
        Ok(quiz) => quiz,
        Err(
            ApiError::Unauthorized
            | ApiError::NotFound
            | ApiError::Forbidden {
                rate_limited: false,
                ..
            }
            | ApiError::Denied {
                status: 401 | 403 | 404,
            },
        ) => {
            return Err(OperationError::refused(
                "denied",
                "this identity cannot see that quiz",
            ));
        }
        Err(other) => return Err(OperationError::Network(other)),
    };
    check_quiz_answerable(&quiz)?;

    // Answers exist only against a live session, and starting one is the
    // person's decision: preparing refuses when none is in progress.
    let submission = crate::quiz::own_submission(client, request.course_id, request.quiz_id)
        .await?
        .filter(canvas_api::models::QuizSubmission::is_live)
        .ok_or_else(|| {
            OperationError::refused(
                "no_session",
                format!(
                    "quiz {} has no session in progress; run `canvas quiz questions` first",
                    request.quiz_id
                ),
            )
        })?;
    let attempt = submission.attempt.unwrap_or(1);
    let questions = crate::quiz::questions(client, submission.id).await?;
    let answers = canonical_answers(&questions, request.answers)?;

    let body = frozen_answers(&answers)?;
    let operation = OperationPlan {
        target: OperationTarget::QuizSubmit {
            course_id: request.course_id,
            quiz_id: request.quiz_id,
            quiz_submission_id: submission.id,
            attempt,
        },
        body,
        subject: None,
        access_code: request.access_code.map(str::to_owned),
        attachments: Vec::new(),
        labels: OperationLabels {
            course_code: request.course_code.map(str::to_owned),
            quiz_title: quiz.title.clone(),
            ..OperationLabels::default()
        },
    };
    store_plan(
        store,
        request.identity_dir,
        request.identity_key,
        request.consumer,
        operation,
        now,
    )
}

/// Refuse a quiz this package must not answer.
///
/// The order is the order a person would ask about it: may the API read it at
/// all, is it locked, and does Canvas require a browser for it. One question
/// at a time hides its questions from the Quiz Submission Questions API for a
/// student, so it is refused here rather than failing mid-quiz.
pub(super) fn check_quiz_answerable(quiz: &canvas_api::models::Quiz) -> Result<(), OperationError> {
    if quiz.require_lockdown_browser.unwrap_or(false) {
        return Err(OperationError::refused(
            "unsupported",
            "this quiz requires LockDown Browser, and must be taken in the browser",
        ));
    }
    if quiz.ip_filter.as_deref().is_some_and(|f| !f.is_empty()) {
        return Err(OperationError::refused(
            "unsupported",
            "this quiz is IP filtered, and must be taken on a permitted network",
        ));
    }
    if quiz.one_question_at_a_time.unwrap_or(false) {
        return Err(OperationError::refused(
            "unsupported",
            "this quiz shows one question at a time, and the API cannot read its questions",
        ));
    }
    if quiz.cant_go_back.unwrap_or(false) {
        return Err(OperationError::refused(
            "unsupported",
            "this quiz does not allow going back, so partial answers are not safe",
        ));
    }
    let locked = quiz.locked_for_user.unwrap_or(false)
        || !quiz.unlocked_for_user.unwrap_or(true)
        || quiz.published == Some(false);
    if locked {
        return Err(OperationError::refused(
            "locked",
            quiz.lock_explanation
                .clone()
                .unwrap_or_else(|| "the quiz is locked".to_owned()),
        ));
    }
    Ok(())
}
///
/// The lock covers the recovery pass only. Nothing is held while a person
/// decides, exactly as REPORT §3.5 requires of a submission plan.
/// Build the canonical answer set: every session question in id order, with
/// `null` where the caller answered nothing.
///
/// The reconciliation compares the frozen bytes with what Canvas holds, which
/// only works on a canonical full set. A duplicate id, or an id the session
/// does not hold, is refused; a missing one is a blank answer, which the
/// approval displays as `null`.
fn canonical_answers(
    questions: &[canvas_api::models::QuizSubmissionQuestion],
    given: &[canvas_api::models::QuizAnswer],
) -> Result<Vec<canvas_api::models::QuizAnswer>, OperationError> {
    use std::collections::BTreeMap;
    let mut by_id: BTreeMap<i64, &serde_json::Value> = BTreeMap::new();
    for answer in given {
        if by_id.insert(answer.id, &answer.answer).is_some() {
            return Err(OperationError::refused(
                "unresolved",
                format!("question {} is answered twice", answer.id),
            ));
        }
    }
    let mut full = Vec::with_capacity(questions.len());
    for question in questions {
        if let Some(answer) = by_id.remove(&question.id) {
            full.push(canvas_api::models::QuizAnswer {
                id: question.id,
                answer: answer.clone(),
            });
        } else {
            full.push(canvas_api::models::QuizAnswer {
                id: question.id,
                answer: serde_json::Value::Null,
            });
        }
    }
    if let Some(unknown) = by_id.keys().next() {
        return Err(OperationError::refused(
            "unresolved",
            format!(
                "question {unknown} is not one of the {} questions this session holds",
                questions.len()
            ),
        ));
    }
    full.sort_by_key(|a| a.id);
    Ok(full)
}

/// Freeze the answers: digest the input JSON and the bytes that are sent.
fn frozen_answers(
    answers: &[canvas_api::models::QuizAnswer],
) -> Result<IntendedText, OperationError> {
    if answers.is_empty() {
        return Err(OperationError::refused(
            "empty_body",
            "the answer set is empty",
        ));
    }
    let outbound = serde_json::to_string(answers)?;
    if outbound.len() > MAX_TEXT_BYTES {
        return Err(OperationError::refused(
            "unsupported",
            format!("the answers exceed {MAX_TEXT_BYTES} bytes"),
        ));
    }
    Ok(IntendedText {
        input_sha256: sha256_hex(outbound.as_bytes()),
        transform: "json".to_owned(),
        sent_sha256: sha256_hex(outbound.as_bytes()),
        outbound_bytes: outbound,
    })
}

fn store_plan(
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    consumer: Option<&str>,
    operation: OperationPlan,
    now: Timestamp,
) -> Result<PreparedOperation, OperationError> {
    let recovered = super::ops::recover_active(store, identity_dir, &operation.target)?;
    let plan = crate::plan::insert_operation(
        store,
        crate::plan::NewOperationPlan {
            identity_key: identity_key.to_owned(),
            consumer: consumer.map(str::to_owned),
            kind: operation.kind().plan_kind(),
            course_id: operation.target.course_id().unwrap_or(0),
            operation: operation.clone(),
        },
        now,
    )?;
    Ok(PreparedOperation {
        plan,
        operation,
        recovered,
    })
}

/// Refuse a topic this package must not write to.
///
/// The order is the order a person would ask about it: is it a group topic, is
/// it closed, and is it gated behind an initial post this identity has not
/// made. The gate is a refusal and never a reason to post anything.
pub(super) fn check_topic_writable(topic: &Value) -> Result<(), OperationError> {
    let group_children = topic
        .get("group_topic_children")
        .and_then(Value::as_array)
        .is_some_and(|children| !children.is_empty());
    if topic.get("group_category_id").is_some_and(|v| !v.is_null()) || group_children {
        return Err(OperationError::refused(
            "group_write",
            "this is a group discussion, and this version writes no group discussion",
        ));
    }
    let locked = topic
        .get("locked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || topic
            .get("locked_for_user")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if locked {
        return Err(OperationError::refused(
            "locked",
            "the topic is locked or closed for comments",
        ));
    }
    // The gate is closed exactly when Canvas requires an initial post and
    // still hides the thread from this identity. Opening it needs a real first
    // post, which is a decision for a person and never a placeholder.
    let gated = topic
        .get("require_initial_post")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !topic
            .get("user_can_see_posts")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    if gated {
        return Err(OperationError::refused(
            "initial_post_required",
            "the topic needs your own first post before replies are visible",
        ));
    }
    Ok(())
}

/// Which transform an operation applies before it sends a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Transform {
    /// Canvas renders a discussion `message` as HTML, so the text is escaped
    /// and wrapped exactly as a text submission is. A `<` a person typed is
    /// never markup.
    Html,
    /// Canvas treats a conversation `body` as plain text and escapes it
    /// itself, so the bytes are sent as they were typed.
    Plain,
}

impl Transform {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "text-to-html",
            Self::Plain => "plain",
        }
    }
}

/// Freeze the body: normalize newlines, digest the input, digest what is sent.
pub(super) fn frozen_body(raw: &str, transform: Transform) -> Result<IntendedText, OperationError> {
    if raw.len() > MAX_TEXT_BYTES {
        return Err(OperationError::refused(
            "unsupported",
            format!("the body exceeds {MAX_TEXT_BYTES} bytes"),
        ));
    }
    let normalized = raw.replace("\r\n", "\n");
    if normalized.trim().is_empty() {
        return Err(OperationError::refused("empty_body", "the body is empty"));
    }
    let outbound = match transform {
        Transform::Html => crate::submit::text_to_html(&normalized),
        Transform::Plain => normalized.clone(),
    };
    Ok(IntendedText {
        input_sha256: sha256_hex(normalized.as_bytes()),
        transform: transform.as_str().to_owned(),
        sent_sha256: sha256_hex(outbound.as_bytes()),
        outbound_bytes: outbound,
    })
}

/// Freeze the attachments: name, size, digest, and absolute path.
pub(super) fn freeze_attachments(
    paths: &[PathBuf],
) -> Result<Vec<OperationAttachment>, OperationError> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(OperationError::refused(
            "unsupported",
            format!("at most {MAX_ATTACHMENTS} attachments"),
        ));
    }
    paths.iter().map(|path| freeze_attachment(path)).collect()
}

/// Freeze one attachment. Missing bytes are `unresolved`, not a crash.
pub(super) fn freeze_attachment(path: &Path) -> Result<OperationAttachment, OperationError> {
    let absolute = std::fs::canonicalize(path).map_err(|e| {
        OperationError::refused(
            "unresolved",
            format!("{} is unreadable: {e}", path.display()),
        )
    })?;
    let metadata = std::fs::metadata(&absolute)?;
    if !metadata.is_file() {
        return Err(OperationError::refused(
            "unresolved",
            format!("{} is not a file", absolute.display()),
        ));
    }
    let name = absolute
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            OperationError::refused("unresolved", "an attachment name is not valid UTF-8")
        })?
        .to_owned();
    let sha256 = digest_file(&absolute)?;
    Ok(OperationAttachment {
        name,
        size: metadata.len(),
        sha256,
        path: absolute
            .to_str()
            .ok_or_else(|| {
                OperationError::refused("unresolved", "an attachment path is not valid UTF-8")
            })?
            .to_owned(),
        canvas_file_id: None,
    })
}

/// Stream a file through SHA-256 without holding it in memory.
pub(super) fn digest_file(path: &Path) -> Result<String, OperationError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A `GET` whose access failures are refusals, not transport errors.
pub(super) async fn get(client: &Client, path: &str) -> Result<Value, OperationError> {
    match client.get::<Value>(path).await {
        Ok(value) => Ok(value),
        Err(
            ApiError::Unauthorized
            | ApiError::NotFound
            | ApiError::Forbidden {
                rate_limited: false,
                ..
            },
        ) => Err(OperationError::refused(
            "denied",
            "this identity cannot see that target",
        )),
        Err(ApiError::Denied { status }) if status == 401 || status == 403 || status == 404 => Err(
            OperationError::refused("denied", "this identity cannot see that target"),
        ),
        Err(other) => Err(OperationError::Network(other)),
    }
}

/// Every **top-level** entry of a topic, following Canvas' pagination.
///
/// Canvas nests a threaded reply under its parent and never lists it here, so
/// a caller that wants a nested reply asks [`entry_replies_of`].
pub(super) async fn entries_of(
    client: &Client,
    course_id: i64,
    topic_id: i64,
) -> Result<Vec<Value>, OperationError> {
    read_entries(
        client,
        &format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries"),
    )
    .await
}

/// Every reply nested under one entry, following Canvas' pagination.
pub(super) async fn entry_replies_of(
    client: &Client,
    course_id: i64,
    topic_id: i64,
    entry_id: i64,
) -> Result<Vec<Value>, OperationError> {
    read_entries(
        client,
        &format!(
            "/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries/{entry_id}/replies"
        ),
    )
    .await
}

async fn read_entries(client: &Client, path: &str) -> Result<Vec<Value>, OperationError> {
    match client.get_all_vec::<Value>(path).await {
        Ok(entries) => Ok(entries),
        Err(
            ApiError::Unauthorized
            | ApiError::NotFound
            | ApiError::Forbidden {
                rate_limited: false,
                ..
            },
        ) => Err(OperationError::refused(
            "denied",
            "this identity cannot read that topic's replies",
        )),
        Err(other) => Err(OperationError::Network(other)),
    }
}

/// Confirm one recipient is someone this identity may message.
async fn resolve_recipient(client: &Client, user_id: &str) -> Result<String, OperationError> {
    let path = format!("/api/v1/search/recipients?user_id={user_id}");
    let found: Value = get(client, &path).await?;
    let matched = found
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| json_id(row.get("id")).as_deref() == Some(user_id));
    match matched {
        Some(row) => Ok(row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(user_id)
            .to_owned()),
        None => Err(OperationError::refused(
            "unresolved",
            format!("no recipient {user_id} this identity can message"),
        )),
    }
}

fn participant_names(conversation: &Value) -> Vec<String> {
    conversation
        .get("participants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Canvas ids arrive as numbers or as strings; both read as the same id.
pub(super) fn json_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Number(n) => n.as_i64().filter(|n| *n > 0).map(|n| n.to_string()),
        Value::String(s) => s
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn topic() -> Value {
        json!({
            "id": 55,
            "title": "Week 3",
            "locked": false,
            "locked_for_user": false,
            "require_initial_post": false,
            "user_can_see_posts": true,
        })
    }

    #[test]
    fn a_group_topic_is_refused_before_anything_is_frozen() {
        let mut by_category = topic();
        by_category["group_category_id"] = json!(9);
        assert_eq!(
            check_topic_writable(&by_category)
                .unwrap_err()
                .refusal_reason(),
            Some("group_write")
        );
        let mut by_children = topic();
        by_children["group_topic_children"] = json!([{ "id": 58, "group_id": 77 }]);
        assert_eq!(
            check_topic_writable(&by_children)
                .unwrap_err()
                .refusal_reason(),
            Some("group_write")
        );
        // An empty child list is not a group topic.
        let mut empty = topic();
        empty["group_topic_children"] = json!([]);
        empty["group_category_id"] = Value::Null;
        assert!(check_topic_writable(&empty).is_ok());
    }

    #[test]
    fn a_locked_topic_and_a_gated_topic_are_refused_apart() {
        let mut locked = topic();
        locked["locked"] = json!(true);
        assert_eq!(
            check_topic_writable(&locked).unwrap_err().refusal_reason(),
            Some("locked")
        );
        let mut for_user = topic();
        for_user["locked_for_user"] = json!(true);
        assert_eq!(
            check_topic_writable(&for_user)
                .unwrap_err()
                .refusal_reason(),
            Some("locked")
        );
        let mut gated = topic();
        gated["require_initial_post"] = json!(true);
        gated["user_can_see_posts"] = json!(false);
        assert_eq!(
            check_topic_writable(&gated).unwrap_err().refusal_reason(),
            Some("initial_post_required")
        );
        // A gate this identity has already passed is not a refusal.
        let mut passed = topic();
        passed["require_initial_post"] = json!(true);
        passed["user_can_see_posts"] = json!(true);
        assert!(check_topic_writable(&passed).is_ok());
    }

    #[test]
    fn a_body_is_frozen_by_its_two_digests_and_never_by_its_path() {
        let html = frozen_body("a < b\n\nnext", Transform::Html).unwrap();
        assert_eq!(html.transform, "text-to-html");
        assert_eq!(html.outbound_bytes, "<p>a &lt; b</p><p>next</p>");
        assert_ne!(html.input_sha256, html.sent_sha256);
        let plain = frozen_body("a < b", Transform::Plain).unwrap();
        assert_eq!(plain.transform, "plain");
        assert_eq!(plain.outbound_bytes, "a < b");
        assert_eq!(plain.input_sha256, plain.sent_sha256);
        // A CRLF body digests as the LF body it becomes.
        assert_eq!(
            frozen_body("one\r\ntwo", Transform::Plain)
                .unwrap()
                .input_sha256,
            frozen_body("one\ntwo", Transform::Plain)
                .unwrap()
                .input_sha256
        );
    }

    #[test]
    fn an_empty_body_is_refused() {
        for raw in ["", "   ", "\n\n", "\r\n"] {
            assert_eq!(
                frozen_body(raw, Transform::Plain)
                    .unwrap_err()
                    .refusal_reason(),
                Some("empty_body"),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn an_id_reads_the_same_as_a_number_and_as_a_string() {
        assert_eq!(json_id(Some(&json!(31))), Some("31".to_owned()));
        assert_eq!(json_id(Some(&json!("31"))), Some("31".to_owned()));
        assert_eq!(json_id(Some(&json!(0))), None);
        assert_eq!(json_id(Some(&json!("x"))), None);
        assert_eq!(json_id(None), None);
    }
}

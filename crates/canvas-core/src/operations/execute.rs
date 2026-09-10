//! `execute` and the `POST` itself (M8-b).
//!
//! Execute is the only door from an operation plan to an operation journal. It
//! refuses an expired, invalidated, or unapproved plan before any network
//! write, reads the target again and re-applies every prepare refusal to what
//! it finds, re-hashes the attachments from disk, and then runs one state
//! transaction that consumes the approval and publishes the journal. Uploading
//! and posting begin only after that transaction commits.
//!
//! The `POST` is classified once and recorded once. A 2xx becomes `posted`, a
//! Canvas answer that is not 2xx becomes `failed`, and an outcome this process
//! never observed becomes `outcome_unknown` — which nothing here ever retries.

use std::path::Path;

use canvas_api::{Client, Error as ApiError};
use jiff::Timestamp;
use reqwest::Method;
use serde_json::{Value, json};

use crate::journal::{AdmissionLock, LockError, OwnerLock};
use crate::plan::{PlanRow, PlanState};
use crate::store::Store;

use super::OperationError;
use super::ops;
use super::prepare::{check_topic_writable, digest_file, entries_of, get, json_id};
use super::record::{
    OpState, OperationKind, OperationPlan, OperationRow, OperationTarget, ResponseRecord,
};

/// The Canvas folder conversation attachments live in.
const CONVERSATION_FOLDER: &str = "conversation attachments";

/// How long a blocked execute waits for its own concurrent execute to commit.
const CONTENTION_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// What one execute produced.
#[derive(Debug)]
pub enum Admitted {
    /// A new operation journal. This process owns it and runs the `POST`.
    Created {
        /// Journal id.
        journal_id: String,
        /// Owner lock, held until the journal is terminal.
        owner: Box<OwnerLock>,
    },
    /// This plan already admitted a journal; no second operation was created.
    Existing {
        /// The journal the plan admitted.
        journal_id: String,
    },
}

/// What one `POST` produced.
#[derive(Debug)]
pub struct Posted {
    /// The journal as it stands after the outcome was recorded.
    pub row: Box<OperationRow>,
    /// A caveat worth printing beside the result.
    pub warning: Option<String>,
}

/// Revalidate an approved operation plan and admit it to a journal.
pub async fn execute(
    client: &Client,
    store: &Store,
    identity_dir: &Path,
    identity_key: &str,
    plan_id: &str,
    now: Timestamp,
) -> Result<Admitted, OperationError> {
    let plan = crate::plan::require(store, plan_id)?;

    // A concurrent execute, a restarted host, or a replayed approval gets the
    // journal this plan already admitted. Expiry gates first admission only, so
    // it is never evaluated for a plan that is already executed.
    if plan.state == PlanState::Executed {
        return existing(&plan);
    }
    let operation = operation_of(&plan)?;

    crate::plan::guard_admission(&plan, now)?;
    require_approval(&plan)?;
    if plan.digest() != plan.plan_sha256 {
        return Err(OperationError::refused(
            "invalidated",
            "the stored plan no longer matches the plan that was approved",
        ));
    }
    if plan.identity_key != identity_key {
        return Err(invalidate(store, &plan, "plan belongs to another identity"));
    }
    if crate::plan::identity_generation(store)? != plan.identity_generation {
        return Err(invalidate(store, &plan, "identity generation changed"));
    }

    // The target is read again, and every prepare refusal is applied to what
    // the fresh read shows. A topic that closed while a person was deciding is
    // refused here, before a journal exists.
    revalidate(client, &operation).await?;

    let name = operation.target.admission_name(&plan.plan_id);
    let admission = match AdmissionLock::try_acquire_named(identity_dir, &name) {
        Ok(lock) => lock,
        Err(LockError::InProgress) => return wait_for_existing(store, &plan).await,
        Err(other) => return Err(other.into()),
    };

    // Admission serializes this target, so the first read under it is
    // authoritative: a concurrent execute may have finished in between.
    let plan = crate::plan::require(store, plan_id)?;
    if plan.state == PlanState::Executed {
        drop(admission);
        return existing(&plan);
    }
    crate::plan::guard_admission(&plan, now)?;
    require_approval(&plan)?;
    ops::recover_active(store, identity_dir, &operation.target)?;

    // The bytes behind every attachment are read again. Changed bytes
    // invalidate the plan, and nothing is sent (REPORT §3.5).
    verify_attachments(store, &plan, &operation)?;

    let (journal_id, owner) = ops::create_linked(store, identity_dir, &admission, &plan)?;
    drop(admission);
    Ok(Admitted::Created {
        journal_id,
        owner: Box::new(owner),
    })
}

fn existing(plan: &PlanRow) -> Result<Admitted, OperationError> {
    plan.journal_id
        .clone()
        .map(|journal_id| Admitted::Existing { journal_id })
        .ok_or_else(|| OperationError::refused("invalidated", "executed plan has no journal"))
}

fn require_approval(plan: &PlanRow) -> Result<(), OperationError> {
    if plan.state == PlanState::Approved && plan.approval.is_some() {
        return Ok(());
    }
    Err(OperationError::refused(
        "approval_required",
        "plan has no recorded human approval",
    ))
}

fn operation_of(plan: &PlanRow) -> Result<OperationPlan, OperationError> {
    plan.operation.clone().ok_or_else(|| {
        OperationError::refused("invalidated", "plan is a submission, not an operation")
    })
}

/// Wait for this plan's own concurrent execute, then return its journal.
async fn wait_for_existing(store: &Store, plan: &PlanRow) -> Result<Admitted, OperationError> {
    let deadline = std::time::Instant::now() + CONTENTION_WAIT;
    loop {
        if let Some(current) = crate::plan::load(store, &plan.plan_id)?
            && current.state == PlanState::Executed
            && let Some(journal_id) = current.journal_id
        {
            return Ok(Admitted::Existing { journal_id });
        }
        if std::time::Instant::now() >= deadline {
            return Err(OperationError::InProgress);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Read the target again and re-apply every refusal to what it shows now.
async fn revalidate(client: &Client, operation: &OperationPlan) -> Result<(), OperationError> {
    match &operation.target {
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id,
        } => {
            let topic = get(
                client,
                &format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}"),
            )
            .await?;
            check_topic_writable(&topic)?;
            if let Some(entry_id) = parent_entry_id {
                let entries = entries_of(client, *course_id, *topic_id).await?;
                if !entries
                    .iter()
                    .any(|entry| json_id(entry.get("id")) == Some(entry_id.to_string()))
                {
                    return Err(OperationError::refused(
                        "unresolved",
                        format!("entry {entry_id} is no longer in topic {topic_id}"),
                    ));
                }
            }
            Ok(())
        }
        OperationTarget::InboxReply { conversation_id } => {
            get(
                client,
                &format!("/api/v1/conversations/{conversation_id}?auto_mark_as_read=false"),
            )
            .await?;
            Ok(())
        }
        // A new conversation has nothing to read back: the recipients were
        // resolved at prepare and they are frozen into the plan digest.
        OperationTarget::InboxSend { .. } => Ok(()),
    }
}

/// Re-hash every attachment. Changed bytes invalidate the plan.
fn verify_attachments(
    store: &Store,
    plan: &PlanRow,
    operation: &OperationPlan,
) -> Result<(), OperationError> {
    for attachment in &operation.attachments {
        let path = Path::new(&attachment.path);
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(e) => {
                return Err(invalidate(
                    store,
                    plan,
                    &format!("{} is no longer readable: {e}", attachment.name),
                ));
            }
        };
        let digest = digest_file(path)?;
        if metadata.len() != attachment.size || digest != attachment.sha256 {
            return Err(invalidate(
                store,
                plan,
                &format!("{} changed since the plan was prepared", attachment.name),
            ));
        }
    }
    Ok(())
}

fn invalidate(store: &Store, plan: &PlanRow, reason: &str) -> OperationError {
    if let Err(e) = crate::plan::invalidate(store, &plan.plan_id, reason) {
        return e.into();
    }
    OperationError::refused("invalidated", reason.to_owned())
}

// ------------------------------------------------------------------- the POST

/// Upload the attachments, send the request, and record what came back.
///
/// The owner lock stays held for the whole of this. A caller that dies inside
/// it leaves a `planned` or `posting` journal, and owner-absent recovery gives
/// each of those the only honest answer it can have.
pub async fn post(
    client: &Client,
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
) -> Result<Posted, OperationError> {
    let row = ops::require(store, journal_id)?;
    if row.state != OpState::Planned {
        return Ok(Posted {
            row: Box::new(row),
            warning: None,
        });
    }

    // Uploads happen while the journal is `planned`, so a process that dies
    // mid-upload recovers to `refused`: nothing was posted, and the file ids
    // it did record name files nothing points at.
    let attachment_ids = match upload_attachments(client, store, owner, journal_id, &row).await {
        Ok(ids) => ids,
        Err(e) => {
            let _ = ops::transition(
                store,
                owner,
                journal_id,
                OpState::Planned,
                OpState::Refused,
                ops::Patch {
                    error_text: Some(format!("attachment upload failed: {e}")),
                    not_posted_evidence: Some(super::NotPostedEvidence::NeverSent),
                    ..ops::Patch::default()
                },
            );
            return Ok(Posted {
                row: Box::new(ops::require(store, journal_id)?),
                warning: Some("nothing was posted: an attachment did not upload".to_owned()),
            });
        }
    };

    ops::mark_posting(store, owner, journal_id)?;
    let (path, body) = request_of(&row, &attachment_ids);
    let url = client.api_url(&path).map_err(|_| {
        OperationError::refused("unresolved", "the target URL is not on this origin")
    })?;
    let request = canvas_api::ApiRequest::new(Method::POST, url)
        .json(&body)
        .map_err(OperationError::Network)?
        .route_key(route_key(row.kind));

    match client.execute_api(request).await {
        Ok((status, _headers, bytes, _url)) => {
            record_response(store, owner, journal_id, status.as_u16(), &bytes)
        }
        // Nothing observed. The journal says so and stays there: only
        // `operation reconcile` moves it, and nothing ever resends it.
        Err(ApiError::Timeout | ApiError::Network) => {
            ops::transition(
                store,
                owner,
                journal_id,
                OpState::Posting,
                OpState::OutcomeUnknown,
                ops::Patch {
                    response_kind: Some("none"),
                    error_text: Some("the request ended without an observed response".to_owned()),
                    ..ops::Patch::default()
                },
            )?;
            Ok(Posted {
                row: Box::new(ops::require(store, journal_id)?),
                warning: Some(
                    "the request may still complete; operation reconcile re-checks it".to_owned(),
                ),
            })
        }
        Err(other) => {
            ops::transition(
                store,
                owner,
                journal_id,
                OpState::Posting,
                OpState::OutcomeUnknown,
                ops::Patch {
                    response_kind: Some("none"),
                    error_text: Some(other.to_string()),
                    ..ops::Patch::default()
                },
            )?;
            Ok(Posted {
                row: Box::new(ops::require(store, journal_id)?),
                warning: Some(
                    "the request may still complete; operation reconcile re-checks it".to_owned(),
                ),
            })
        }
    }
}

fn record_response(
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    status: u16,
    bytes: &[u8],
) -> Result<Posted, OperationError> {
    let row = ops::require(store, journal_id)?;
    let value: Option<Value> = serde_json::from_slice(bytes).ok();
    if (200..300).contains(&status) {
        let response = response_record(row.kind, value.as_ref(), bytes);
        let named = response.id.is_some();
        ops::commit_posted(store, owner, journal_id, status, &response)?;
        persist_identity(store, journal_id, &ops::require(store, journal_id)?)?;
        return Ok(Posted {
            row: Box::new(ops::require(store, journal_id)?),
            warning: (!named).then(|| {
                "Canvas accepted the request but named no object, so nothing links this \
                 journal to a post; operation status re-checks it"
                    .to_owned()
            }),
        });
    }

    // Canvas answered, and the answer was not an acceptance. Nothing was
    // posted, and the status says why.
    let canvas_shaped = value
        .as_ref()
        .is_some_and(|v| v.get("errors").is_some() || v.get("message").is_some());
    ops::transition(
        store,
        owner,
        journal_id,
        OpState::Posting,
        OpState::Failed,
        ops::Patch {
            error_text: Some(errors_of(value.as_ref()).unwrap_or_else(|| format!("HTTP {status}"))),
            post_status: Some(i64::from(status)),
            response_kind: Some(if canvas_shaped {
                "canvas-error"
            } else {
                "other"
            }),
            not_posted_evidence: None,
        },
    )?;
    Ok(Posted {
        row: Box::new(ops::require(store, journal_id)?),
        warning: None,
    })
}

/// Store the identity block on the receipt the commit built.
fn persist_identity(
    store: &Store,
    journal_id: &str,
    row: &OperationRow,
) -> Result<(), OperationError> {
    let Some(receipt) = row.receipt.clone() else {
        return Ok(());
    };
    let mut receipt = receipt;
    ops::attach_identity(store, &mut receipt)?;
    let json = serde_json::to_string(&receipt)?;
    let jid = journal_id.to_owned();
    store.call_blocking(move |conns| {
        conns.state.execute(
            "UPDATE operation_journal SET receipt_record_json = ?1 WHERE journal_id = ?2",
            rusqlite::params![json, jid],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// The route this operation posts to and the body it sends.
#[must_use]
pub fn request_of(row: &OperationRow, attachment_ids: &[String]) -> (String, Value) {
    let body = &row.intended.body.outbound_bytes;
    match &row.intended.target {
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id: Some(entry_id),
        } => (
            format!(
                "/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries/{entry_id}/replies"
            ),
            json!({ "message": body }),
        ),
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id: None,
        } => (
            format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries"),
            json!({ "message": body }),
        ),
        OperationTarget::InboxSend { recipients } => (
            "/api/v1/conversations".to_owned(),
            json!({
                "recipients": recipients,
                "subject": row.intended.subject,
                "body": body,
                // Never a group conversation: this package writes none.
                "group_conversation": false,
                "attachment_ids": attachment_ids,
            }),
        ),
        OperationTarget::InboxReply { conversation_id } => (
            format!("/api/v1/conversations/{conversation_id}/add_message"),
            json!({ "body": body, "attachment_ids": attachment_ids }),
        ),
    }
}

const fn route_key(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::DiscussionReply => "discussion_entries:create",
        OperationKind::InboxSend => "conversations:create",
        OperationKind::InboxReply => "conversations:add_message",
    }
}

/// Read the allowlisted record out of what Canvas answered.
///
/// Only ids, times, an author, and digests. The message itself never enters a
/// journal, and neither does anything Canvas returns that is not on this list.
#[must_use]
pub fn response_record(kind: OperationKind, value: Option<&Value>, raw: &[u8]) -> ResponseRecord {
    let mut record = ResponseRecord {
        response_sha256: Some(hex_sha256(raw)),
        ..ResponseRecord::default()
    };
    let Some(value) = value else {
        return record;
    };
    match kind {
        OperationKind::DiscussionReply => {
            record.id = json_id(value.get("id"));
            record.created_at = string_of(value.get("created_at"));
            record.user_id = json_id(value.get("user_id"));
            record.body_sha256 = string_of(value.get("message")).map(|m| hex_sha256(m.as_bytes()));
            record.attachment_ids = attachment_ids_of(value);
        }
        OperationKind::InboxSend => {
            // `POST /conversations` answers with a list, one per conversation
            // it created. Only a single non-group conversation is ever asked
            // for, so a list of exactly one is the shape this reads.
            let first = value
                .as_array()
                .and_then(|rows| rows.first())
                .unwrap_or(value);
            record.conversation_id = json_id(first.get("id"));
            let message = first
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|m| m.first());
            if let Some(message) = message {
                record.id = json_id(message.get("id"));
                record.created_at = string_of(message.get("created_at"));
                record.user_id = json_id(message.get("author_id"));
                record.body_sha256 =
                    string_of(message.get("body")).map(|b| hex_sha256(b.as_bytes()));
                record.attachment_ids = attachment_ids_of(message);
            }
            if record.created_at.is_none() {
                record.created_at = string_of(first.get("last_message_at"));
            }
        }
        OperationKind::InboxReply => {
            record.conversation_id = json_id(value.get("id"));
            let message = value
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|m| m.first());
            if let Some(message) = message {
                record.id = json_id(message.get("id"));
                record.created_at = string_of(message.get("created_at"));
                record.user_id = json_id(message.get("author_id"));
                record.body_sha256 =
                    string_of(message.get("body")).map(|b| hex_sha256(b.as_bytes()));
                record.attachment_ids = attachment_ids_of(message);
            }
        }
    }
    record.created_at_local = super::receipt::local(record.created_at.as_deref());
    record
}

fn attachment_ids_of(value: &Value) -> Vec<String> {
    value
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| json_id(a.get("id")))
        .collect()
}

fn string_of(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn errors_of(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Some(canvas_api::redact::redact(message));
    }
    let errors = value.get("errors")?;
    let text = match errors {
        Value::Array(items) => items
            .iter()
            .filter_map(|e| e.get("message").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("; "),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    (!text.is_empty()).then(|| canvas_api::redact::redact(&text))
}

pub(super) fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Upload every frozen attachment and return the Canvas file ids, in order.
///
/// The digest is verified from the stream as §11 requires, so bytes that
/// changed between the plan check and the upload still stop the operation
/// before the `POST`.
async fn upload_attachments(
    client: &Client,
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    row: &OperationRow,
) -> Result<Vec<String>, OperationError> {
    let mut ids = Vec::with_capacity(row.intended.attachments.len());
    for (index, attachment) in row.intended.attachments.iter().enumerate() {
        if let Some(id) = &attachment.canvas_file_id {
            ids.push(id.clone());
            continue;
        }
        let file = tokio::fs::File::open(&attachment.path).await?;
        let meta = canvas_api::upload::UploadMeta {
            name: attachment.name.clone(),
            size: attachment.size,
            content_type: "application/octet-stream".to_owned(),
        };
        let result =
            canvas_api::upload::upload_user_file(client, CONVERSATION_FOLDER, &meta, file).await?;
        if hex(&result.sha256) != attachment.sha256 {
            return Err(OperationError::refused(
                "invalidated",
                format!("{} changed while it was uploading", attachment.name),
            ));
        }
        ops::record_attachment_id(store, owner, journal_id, index, result.file_id)?;
        ids.push(result.file_id.to_string());
    }
    Ok(ids)
}

fn hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::IntendedText;
    use crate::operations::record::{Attribution, OperationLabels};

    fn row(target: OperationTarget, subject: Option<&str>) -> OperationRow {
        OperationRow {
            journal_id: "j".into(),
            identity_key: "k".into(),
            plan_id: "p".into(),
            kind: target.kind(),
            intended: OperationPlan {
                target,
                body: IntendedText {
                    input_sha256: "a".into(),
                    transform: "plain".into(),
                    sent_sha256: "b".into(),
                    outbound_bytes: "hello".into(),
                },
                subject: subject.map(str::to_owned),
                attachments: Vec::new(),
                labels: OperationLabels::default(),
            },
            approval: None,
            state: OpState::Planned,
            post_status: None,
            response_kind: None,
            not_posted_evidence: None,
            attribution: Attribution::None,
            response: None,
            readback: None,
            server_match: None,
            receipt: None,
            uploaded_file_ids: Vec::new(),
            error_text: None,
            created_at: "2026-09-10T00:00:00Z".into(),
            posting_started_at: None,
            terminal_at: None,
            acknowledged_at: None,
        }
    }

    #[test]
    fn every_operation_posts_to_the_route_the_contract_names() {
        let (path, body) = request_of(
            &row(
                OperationTarget::DiscussionReply {
                    course_id: 5,
                    topic_id: 55,
                    parent_entry_id: None,
                },
                None,
            ),
            &[],
        );
        assert_eq!(path, "/api/v1/courses/5/discussion_topics/55/entries");
        assert_eq!(body["message"], "hello");

        let (path, _) = request_of(
            &row(
                OperationTarget::DiscussionReply {
                    course_id: 5,
                    topic_id: 55,
                    parent_entry_id: Some(900),
                },
                None,
            ),
            &[],
        );
        assert_eq!(
            path,
            "/api/v1/courses/5/discussion_topics/55/entries/900/replies"
        );

        let (path, body) = request_of(
            &row(
                OperationTarget::InboxSend {
                    recipients: vec!["31".into(), "32".into()],
                },
                Some("Lab"),
            ),
            &["7".to_owned()],
        );
        assert_eq!(path, "/api/v1/conversations");
        assert_eq!(body["recipients"], serde_json::json!(["31", "32"]));
        assert_eq!(body["subject"], "Lab");
        assert_eq!(
            body["group_conversation"], false,
            "this package writes no group conversation"
        );
        assert_eq!(body["attachment_ids"], serde_json::json!(["7"]));

        let (path, body) = request_of(
            &row(
                OperationTarget::InboxReply {
                    conversation_id: 701,
                },
                None,
            ),
            &[],
        );
        assert_eq!(path, "/api/v1/conversations/701/add_message");
        assert_eq!(body["body"], "hello");
        assert_eq!(body["attachment_ids"], serde_json::json!([]));
    }

    #[test]
    fn a_response_record_keeps_ids_and_digests_and_nothing_else() {
        let raw = br#"[{"id":9,"messages":[{"id":81,"author_id":123,"body":"hello",
            "created_at":"2026-09-10T10:00:00Z","attachments":[{"id":7,"url":"https://s3/x?sig=1"}]}]}]"#;
        let value: Value = serde_json::from_slice(raw).unwrap();
        let record = response_record(OperationKind::InboxSend, Some(&value), raw);
        assert_eq!(record.conversation_id.as_deref(), Some("9"));
        assert_eq!(record.id.as_deref(), Some("81"));
        assert_eq!(record.user_id.as_deref(), Some("123"));
        assert_eq!(record.attachment_ids, ["7"]);
        assert_eq!(record.body_sha256, Some(hex_sha256(b"hello")));
        let serialized = serde_json::to_string(&record).unwrap();
        assert!(!serialized.contains("hello"), "no body ever travels");
        assert!(!serialized.contains("sig="), "no signed URL ever travels");
    }

    #[test]
    fn an_accepted_response_without_an_id_claims_nothing() {
        let raw = b"{}";
        let value: Value = serde_json::from_slice(raw).unwrap();
        let record = response_record(OperationKind::DiscussionReply, Some(&value), raw);
        assert!(record.id.is_none());
        assert!(record.response_sha256.is_some());
    }
}

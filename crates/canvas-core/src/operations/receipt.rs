//! The `receipt@1` document an operation journal produces (M8-b).
//!
//! A submission receipt and an operation receipt are one document type. The
//! submission half is `null` on an operation and the `operation` block is
//! `null` on a submission, which is Appendix D's nullable convention: the field
//! is always present, and an absent fact reads as `null` rather than as a
//! different shape.

use crate::receipts::ReceiptDocument;

use super::record::{OperationReceipt, OperationRow, OperationTarget};

/// Render `created_at` in a time zone, as the journal does for submissions.
#[must_use]
pub fn local(value: Option<&str>) -> Option<String> {
    let zone = jiff::tz::TimeZone::system();
    let ts: jiff::Timestamp = value?.parse().ok()?;
    let local = ts.to_zoned(zone);
    Some(format!("{}{}", local.datetime(), local.strftime("%:z")))
}

/// The `operation` block of a receipt.
#[must_use]
pub fn operation_block(row: &OperationRow) -> OperationReceipt {
    let (course_id, topic_id, parent_entry_id, mut conversation_id, recipients) = match &row
        .intended
        .target
    {
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id,
        } => (
            Some(course_id.to_string()),
            Some(topic_id.to_string()),
            parent_entry_id.map(|id| id.to_string()),
            None,
            Vec::new(),
        ),
        OperationTarget::InboxSend { recipients } => (None, None, None, None, recipients.clone()),
        OperationTarget::InboxReply { conversation_id } => (
            None,
            None,
            None,
            Some(conversation_id.to_string()),
            Vec::new(),
        ),
    };
    // A send has no conversation id until Canvas answers with one.
    if conversation_id.is_none() {
        conversation_id = row
            .response
            .as_ref()
            .and_then(|r| r.conversation_id.clone());
    }
    let server_body_sha256 = row
        .response
        .as_ref()
        .and_then(|r| r.body_sha256.clone())
        .or_else(|| row.readback.as_ref().and_then(|r| r.body_sha256.clone()));
    OperationReceipt {
        kind: row.kind.as_str().to_owned(),
        course_id,
        topic_id,
        parent_entry_id,
        conversation_id,
        recipients,
        subject: row.intended.subject.clone(),
        state: row.state.as_str().to_owned(),
        input_sha256: row.intended.body.input_sha256.clone(),
        transform: row.intended.body.transform.clone(),
        sent_sha256: row.intended.body.sent_sha256.clone(),
        server_body_sha256,
        attachments: row.intended.attachments.clone(),
        posted: row.response.clone(),
        readback: row.readback.clone(),
        server_match: row.server_match.clone(),
        attribution: row.attribution.as_str().to_owned(),
        delivery: row.delivery().to_owned(),
    }
}

/// Build the whole receipt document for one operation journal.
///
/// The identity block is filled in by the caller, which holds the store; the
/// document is otherwise complete.
#[must_use]
pub fn build(row: &OperationRow, receipt_id: String) -> ReceiptDocument {
    ReceiptDocument {
        receipt_id,
        journal_id: row.journal_id.clone(),
        identity: crate::receipts::ReceiptIdentity {
            origin: String::new(),
            user_id: String::new(),
            key: row.identity_key.clone(),
        },
        course_id: row.intended.target.course_id().map(|id| id.to_string()),
        course_code: row.intended.labels.course_code.clone(),
        assignment_id: None,
        assignment_name: None,
        kind: row.kind.as_str().to_owned(),
        baseline_attempt: None,
        attribution: row.attribution.as_str().to_owned(),
        posted: None,
        readback: None,
        files: Vec::new(),
        text: Some(crate::receipts::ReceiptText {
            input_sha256: row.intended.body.input_sha256.clone(),
            transform: row.intended.body.transform.clone(),
            sent_sha256: row.intended.body.sent_sha256.clone(),
            server_body_sha256: row
                .response
                .as_ref()
                .and_then(|r| r.body_sha256.clone())
                .or_else(|| row.readback.as_ref().and_then(|r| r.body_sha256.clone())),
        }),
        url: None,
        due_at: None,
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        created_at: row.created_at.clone(),
        plan_id: Some(row.plan_id.clone()),
        approval: row.approval.clone(),
        operation: Some(operation_block(row)),
    }
}

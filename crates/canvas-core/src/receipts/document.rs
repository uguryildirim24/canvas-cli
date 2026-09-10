//! `receipt@1` document types (§12.2 / Appendix D).

use serde::{Deserialize, Serialize};

use crate::journal::{PostedRecord, ReadbackRecord};

/// Identity block embedded in a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptIdentity {
    /// Canonical Canvas origin.
    pub origin: String,
    /// Canvas user id as a string.
    pub user_id: String,
    /// Identity key.
    pub key: String,
}

/// Frozen file entry on a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptFile {
    /// File name.
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// Content digest.
    pub sha256: String,
    /// Canvas file id when uploaded.
    #[serde(default)]
    pub canvas_file_id: Option<String>,
}

/// Frozen text digests on a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptText {
    /// Digest of the raw input before transform.
    pub input_sha256: String,
    /// Transform name (`plain` / `html`).
    pub transform: String,
    /// Digest of the bytes sent.
    pub sent_sha256: String,
    /// `posted.body_sha256 ?? readback.body_sha256 ?? null`.
    #[serde(default)]
    pub server_body_sha256: Option<String>,
}

/// Full `receipt@1` document (export file and `receipts show` receipt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptDocument {
    /// Receipt id.
    pub receipt_id: String,
    /// Journal id.
    pub journal_id: String,
    /// Bound identity.
    pub identity: ReceiptIdentity,
    /// Course id (string).
    pub course_id: String,
    /// Course code when known.
    #[serde(default)]
    pub course_code: Option<String>,
    /// Assignment id (string).
    pub assignment_id: String,
    /// Assignment name when known.
    #[serde(default)]
    pub assignment_name: Option<String>,
    /// Submission kind.
    pub kind: String,
    /// Baseline attempt at plan time.
    pub baseline_attempt: i64,
    /// `observed` or `unproven`.
    pub attribution: String,
    /// Allowlisted posted record.
    pub posted: PostedRecord,
    /// Readback when enriched.
    #[serde(default)]
    pub readback: Option<ReadbackRecord>,
    /// Frozen files.
    pub files: Vec<ReceiptFile>,
    /// Frozen text digests when kind is text/html.
    #[serde(default)]
    pub text: Option<ReceiptText>,
    /// Frozen URL when kind is URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Due-at when known.
    #[serde(default)]
    pub due_at: Option<String>,
    /// CLI version that built the receipt.
    pub cli_version: String,
    /// Journal `created_at`.
    pub created_at: String,
}

impl ReceiptDocument {
    /// Apply `text.server_body_sha256 = posted.body_sha256 ?? readback.body_sha256 ?? null`.
    pub(crate) fn recompute_server_body_sha256(&mut self) {
        if let Some(text) = &mut self.text {
            text.server_body_sha256 = self
                .posted
                .body_sha256
                .clone()
                .or_else(|| self.readback.as_ref().and_then(|r| r.body_sha256.clone()));
        }
    }
}

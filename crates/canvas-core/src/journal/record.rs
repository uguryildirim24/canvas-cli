//! Allowlisted response / receipt records (§12.2).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::journal::JournalError;

/// Evidence source for a posted record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// Observed from the submission POST response.
    PostResponse,
    /// Rebuilt from submission history.
    HistoryFiles,
}

/// Attachment allowlist entry (no URLs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRecord {
    /// Canvas attachment id.
    pub id: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// MIME type.
    pub content_type: Option<String>,
}

/// Allowlisted posted / response record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostedRecord {
    /// Evidence source.
    pub evidence: Evidence,
    /// Submission id when known.
    pub submission_id: Option<String>,
    /// Attempt number.
    pub attempt: Option<i64>,
    /// Submitted-at timestamp.
    pub submitted_at: Option<String>,
    /// Workflow state.
    pub workflow_state: Option<String>,
    /// Late flag.
    pub late: Option<bool>,
    /// Missing flag.
    pub missing: Option<bool>,
    /// Excused flag.
    pub excused: Option<bool>,
    /// Submission type.
    pub submission_type: Option<String>,
    /// Allowlisted attachments (no URLs).
    pub attachments: Vec<AttachmentRecord>,
    /// SHA-256 of the response/history `body` field when present.
    pub body_sha256: Option<String>,
    /// Submitted URL when present (the assignment URL field, not a signed transfer URL).
    pub url: Option<String>,
    /// SHA-256 of the raw HTTP response body (`null` for history-files).
    pub response_sha256: Option<String>,
}

/// Readback enrichment record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadbackRecord {
    /// Submitted-at.
    pub submitted_at: Option<String>,
    /// Late flag.
    pub late: Option<bool>,
    /// Attachments.
    pub attachments: Vec<AttachmentRecord>,
    /// Body digest.
    pub body_sha256: Option<String>,
}

/// Reconcile candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRecord {
    /// Attempt number.
    pub attempt: i64,
    /// Submitted-at.
    pub submitted_at: Option<String>,
    /// Attachment ids (empty for text/URL).
    pub attachment_ids: Vec<String>,
}

/// Stored receipt document (subset; full export is M2-b/receipts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptRecord {
    /// Receipt id.
    pub receipt_id: String,
    /// Journal id.
    pub journal_id: String,
    /// Attribution.
    pub attribution: String,
    /// Posted allowlist.
    pub posted: PostedRecord,
    /// Readback when known.
    pub readback: Option<ReadbackRecord>,
}

/// Build an allowlisted posted record from a JSON object.
///
/// Capability-bearing URLs on attachments are dropped. Only the submission `url`
/// field (http/https assignment URL) is retained.
pub fn allowlist_from_json(
    evidence: Evidence,
    value: &Value,
    raw_body: Option<&[u8]>,
) -> Result<PostedRecord, JournalError> {
    let response_sha256 = match (evidence, raw_body) {
        (Evidence::PostResponse, Some(bytes)) => Some(hex_sha256(bytes)),
        _ => None,
    };
    let body_sha256 = value
        .get("body")
        .and_then(Value::as_str)
        .map(|s| hex_sha256(s.as_bytes()));

    let attachments = value
        .get("attachments")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = json_id(item.get("id")?)?;
                    Some(AttachmentRecord {
                        id,
                        display_name: item
                            .get("display_name")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        size: item.get("size").and_then(Value::as_u64),
                        content_type: item
                            .get("content_type")
                            .or_else(|| item.get("content-type"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .filter(|u| {
            // Drop signed / capability-bearing query URLs typical of storage.
            let lower = u.to_ascii_lowercase();
            !(lower.contains("x-amz-")
                || lower.contains("signature=")
                || lower.contains("access_token"))
        })
        .map(str::to_owned);

    Ok(PostedRecord {
        evidence,
        submission_id: value.get("id").and_then(json_id),
        attempt: value.get("attempt").and_then(Value::as_i64),
        submitted_at: value
            .get("submitted_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        workflow_state: value
            .get("workflow_state")
            .and_then(Value::as_str)
            .map(str::to_owned),
        late: value.get("late").and_then(Value::as_bool),
        missing: value.get("missing").and_then(Value::as_bool),
        excused: value.get("excused").and_then(Value::as_bool),
        submission_type: value
            .get("submission_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        attachments,
        body_sha256,
        url,
        response_sha256,
    })
}

fn json_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allowlist_strips_capability_urls() {
        let value = json!({
            "id": 1,
            "attempt": 2,
            "url": "https://example.test/assignment",
            "attachments": [{
                "id": 9,
                "display_name": "a.pdf",
                "size": 3,
                "content_type": "application/pdf",
                "url": "https://s3.example/x?X-Amz-Signature=abc&token=secret"
            }]
        });
        let raw = br#"{"id":1}"#;
        let posted = allowlist_from_json(Evidence::PostResponse, &value, Some(raw)).unwrap();
        let ser = serde_json::to_string(&posted).unwrap();
        assert!(!ser.contains("X-Amz"));
        assert!(!ser.contains("token=secret"));
        assert!(!ser.contains("s3.example"));
        assert_eq!(posted.attachments.len(), 1);
        assert_eq!(posted.url.as_deref(), Some("https://example.test/assignment"));
        assert!(posted.response_sha256.is_some());
    }
}

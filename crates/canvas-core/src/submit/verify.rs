//! `submission verify` (§12.2).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use canvas_api::{Client, get_submission_history};
use cap_std::fs::Dir;
use thiserror::Error;

use crate::download::contain::{self, ContainError};
use crate::identity::Paths;
use crate::journal::{JournalError, get_journal};
use crate::receipts::{ReceiptDocument, ReceiptError, rebuild_from_journal, show};
use crate::store::{DbError, Store};
use crate::submit::freeze::hex_sha256;

/// Verify outcome label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// File hashes match.
    Verified,
    /// Text body digest matches.
    VerifiedBody,
    /// Hash or ID set mismatch.
    Mismatch,
    /// Missing server data.
    Unavailable,
    /// Local validation refused.
    Refused,
}

/// Per-file verify row.
#[derive(Debug, Clone)]
pub struct VerifyFileRow {
    pub canvas_file_id: String,
    pub name: Option<String>,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub status: String,
}

/// Body verify row.
#[derive(Debug, Clone)]
pub struct VerifyBodyRow {
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub status: String,
}

/// Verify result.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub outcome: VerifyOutcome,
    pub receipt_id: String,
    pub attempt: Option<i64>,
    pub attribution: Option<String>,
    pub files: Vec<VerifyFileRow>,
    pub body: Option<VerifyBodyRow>,
    pub reason: Option<String>,
}

/// Verify errors outside the structured result.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Receipt(#[from] ReceiptError),
    #[error(transparent)]
    Contain(#[from] ContainError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Store(#[from] DbError),
}

/// Validate a receipt document then fetch history and compare digests.
pub async fn verify(
    client: &Client,
    store: &Store,
    paths: &Paths,
    identity_key: &str,
    receipt: &ReceiptDocument,
) -> Result<VerifyResult, VerifyError> {
    if let Err(result) = validate_local(store, identity_key, receipt) {
        return Ok(result);
    }

    let course_id: i64 = receipt
        .course_id
        .parse()
        .map_err(|_| VerifyError::Receipt(ReceiptError::Refused))?;
    let assignment_id: i64 = receipt
        .assignment_id
        .parse()
        .map_err(|_| VerifyError::Receipt(ReceiptError::Refused))?;
    let attempt = receipt.posted.attempt.ok_or(ReceiptError::Refused)?;

    let history = get_submission_history(client, course_id, assignment_id).await?;
    let entry = history
        .submission_history
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|e| e.attempt.as_value().copied() == Some(attempt));
    let Some(entry) = entry else {
        return Ok(VerifyResult {
            outcome: VerifyOutcome::Unavailable,
            receipt_id: receipt.receipt_id.clone(),
            attempt: Some(attempt),
            attribution: Some(receipt.attribution.clone()),
            files: Vec::new(),
            body: None,
            reason: Some("no history entry for posted attempt".into()),
        });
    };

    if receipt.kind == "online_upload" {
        return verify_files(client, &paths.identity_dir, receipt, entry).await;
    }
    if receipt.kind == "online_text_entry" {
        return Ok(verify_text(receipt, entry));
    }
    Ok(VerifyResult {
        outcome: VerifyOutcome::Refused,
        receipt_id: receipt.receipt_id.clone(),
        attempt: Some(attempt),
        attribution: Some(receipt.attribution.clone()),
        files: Vec::new(),
        body: None,
        reason: Some("URL receipts cannot be verified".into()),
    })
}

/// Load a receipt by receipt id (export file or journal rebuild).
pub fn load_receipt_for_verify(
    store: &Store,
    paths: &Paths,
    receipt_id: &str,
) -> Result<ReceiptDocument, VerifyError> {
    let path = paths
        .identity_dir
        .join("receipts")
        .join(format!("{receipt_id}.json"));
    if path.is_file() {
        let bytes = std::fs::read(&path)?;
        let mut doc: ReceiptDocument = serde_json::from_slice(&bytes)?;
        doc.recompute_server_body_sha256();
        return Ok(doc);
    }
    let shown = show(store, &paths.identity_dir, receipt_id)?;
    if let Some(doc) = shown.receipt {
        return Ok(doc);
    }
    rebuild_from_journal(store, &shown.journal.journal_id).map_err(Into::into)
}

fn validate_local(
    store: &Store,
    identity_key: &str,
    receipt: &ReceiptDocument,
) -> Result<(), VerifyResult> {
    let refused = |reason: &str| VerifyResult {
        outcome: VerifyOutcome::Refused,
        receipt_id: receipt.receipt_id.clone(),
        attempt: receipt.posted.attempt,
        attribution: Some(receipt.attribution.clone()),
        files: Vec::new(),
        body: None,
        reason: Some(reason.into()),
    };
    if receipt.identity.key != identity_key {
        return Err(refused("identity mismatch"));
    }
    if receipt.posted.attempt.is_none() {
        return Err(refused("posted.attempt missing"));
    }
    if receipt.kind == "online_url" {
        return Err(refused("URL receipts cannot be verified"));
    }
    if receipt.kind == "online_upload" {
        let file_ids: BTreeSet<_> = receipt
            .files
            .iter()
            .filter_map(|f| f.canvas_file_id.clone())
            .collect();
        let posted_ids: BTreeSet<_> = receipt
            .posted
            .attachments
            .iter()
            .map(|a| a.id.clone())
            .collect();
        if file_ids.is_empty() || file_ids != posted_ids {
            return Err(refused(
                "file id sets on receipt files and posted.attachments must match and be non-empty",
            ));
        }
        if let Ok(Some(row)) = get_journal(store, &receipt.journal_id) {
            let uploaded: BTreeSet<String> =
                serde_json::from_str::<Vec<i64>>(&row.uploaded_file_ids_json)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect();
            if uploaded != file_ids {
                return Err(refused(
                    "journal uploaded file ids do not match receipt file ids",
                ));
            }
        }
    }
    Ok(())
}

async fn verify_files(
    client: &Client,
    identity_dir: &Path,
    receipt: &ReceiptDocument,
    entry: &canvas_api::models::SubmissionHistoryEntry,
) -> Result<VerifyResult, VerifyError> {
    let expected: BTreeSet<_> = receipt
        .files
        .iter()
        .filter_map(|f| f.canvas_file_id.clone())
        .collect();
    let actual: BTreeSet<_> = entry
        .attachments
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|a| a.id.to_string())
        .collect();
    let mut rows = Vec::new();
    let mut mismatch = false;
    let mut unavailable = false;

    for id in expected.difference(&actual) {
        mismatch = true;
        rows.push(VerifyFileRow {
            canvas_file_id: id.clone(),
            name: receipt
                .files
                .iter()
                .find(|f| f.canvas_file_id.as_deref() == Some(id.as_str()))
                .map(|f| f.name.clone()),
            expected_sha256: receipt
                .files
                .iter()
                .find(|f| f.canvas_file_id.as_deref() == Some(id.as_str()))
                .map(|f| f.sha256.clone()),
            actual_sha256: None,
            status: "missing".into(),
        });
    }
    for id in actual.difference(&expected) {
        mismatch = true;
        rows.push(VerifyFileRow {
            canvas_file_id: id.clone(),
            name: None,
            expected_sha256: None,
            actual_sha256: None,
            status: "extra".into(),
        });
    }

    let tmp_root = identity_dir.join("tmp");
    std::fs::create_dir_all(&tmp_root)?;
    let root = Dir::open_ambient_dir(&tmp_root, cap_std::ambient_authority())?;

    for file in &receipt.files {
        let Some(fid) = &file.canvas_file_id else {
            continue;
        };
        if !actual.contains(fid) {
            continue;
        }
        let attachment = entry
            .attachments
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|a| a.id.to_string() == *fid);
        let Some(attachment) = attachment else {
            continue;
        };
        let Some(url) = attachment.url.clone() else {
            unavailable = true;
            rows.push(VerifyFileRow {
                canvas_file_id: fid.clone(),
                name: Some(file.name.clone()),
                expected_sha256: Some(file.sha256.clone()),
                actual_sha256: None,
                status: "unavailable".into(),
            });
            continue;
        };
        let rel = format!("verify-{fid}");
        let contained = contain::walk_parent(&root, &rel)?;
        let mut part = contain::open_contained_file(&contained, true)?;
        let mut sink = Vec::new();
        match canvas_api::download::download(client, url, &mut sink, attachment.size, |_| {}).await
        {
            Ok(_) => {
                let actual_hash = hex_sha256(&sink);
                part.write_all(&sink)?;
                let status = if actual_hash == file.sha256 {
                    "ok"
                } else {
                    mismatch = true;
                    "mismatch"
                };
                rows.push(VerifyFileRow {
                    canvas_file_id: fid.clone(),
                    name: Some(file.name.clone()),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: Some(actual_hash),
                    status: status.into(),
                });
            }
            Err(_) => {
                unavailable = true;
                rows.push(VerifyFileRow {
                    canvas_file_id: fid.clone(),
                    name: Some(file.name.clone()),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: None,
                    status: "unavailable".into(),
                });
            }
        }
    }

    rows.sort_by(|a, b| a.canvas_file_id.cmp(&b.canvas_file_id));
    let outcome = if mismatch {
        VerifyOutcome::Mismatch
    } else if unavailable {
        VerifyOutcome::Unavailable
    } else {
        VerifyOutcome::Verified
    };
    Ok(VerifyResult {
        outcome,
        receipt_id: receipt.receipt_id.clone(),
        attempt: receipt.posted.attempt,
        attribution: Some(receipt.attribution.clone()),
        files: rows,
        body: None,
        reason: None,
    })
}

fn verify_text(
    receipt: &ReceiptDocument,
    entry: &canvas_api::models::SubmissionHistoryEntry,
) -> VerifyResult {
    let reference = receipt
        .text
        .as_ref()
        .and_then(|t| t.server_body_sha256.clone());
    let Some(expected) = reference else {
        return VerifyResult {
            outcome: VerifyOutcome::Unavailable,
            receipt_id: receipt.receipt_id.clone(),
            attempt: receipt.posted.attempt,
            attribution: Some(receipt.attribution.clone()),
            files: Vec::new(),
            body: Some(VerifyBodyRow {
                expected_sha256: None,
                actual_sha256: None,
                status: "unavailable".into(),
            }),
            reason: Some("no server body digest recorded".into()),
        };
    };
    let Some(body) = &entry.body else {
        return VerifyResult {
            outcome: VerifyOutcome::Unavailable,
            receipt_id: receipt.receipt_id.clone(),
            attempt: receipt.posted.attempt,
            attribution: Some(receipt.attribution.clone()),
            files: Vec::new(),
            body: Some(VerifyBodyRow {
                expected_sha256: Some(expected),
                actual_sha256: None,
                status: "unavailable".into(),
            }),
            reason: Some("history entry has no body".into()),
        };
    };
    let actual = hex_sha256(body.as_bytes());
    if actual == expected {
        VerifyResult {
            outcome: VerifyOutcome::VerifiedBody,
            receipt_id: receipt.receipt_id.clone(),
            attempt: receipt.posted.attempt,
            attribution: Some(receipt.attribution.clone()),
            files: Vec::new(),
            body: Some(VerifyBodyRow {
                expected_sha256: Some(expected),
                actual_sha256: Some(actual),
                status: "ok".into(),
            }),
            reason: None,
        }
    } else {
        VerifyResult {
            outcome: VerifyOutcome::Mismatch,
            receipt_id: receipt.receipt_id.clone(),
            attempt: receipt.posted.attempt,
            attribution: Some(receipt.attribution.clone()),
            files: Vec::new(),
            body: Some(VerifyBodyRow {
                expected_sha256: Some(expected),
                actual_sha256: Some(actual),
                status: "mismatch".into(),
            }),
            reason: None,
        }
    }
}

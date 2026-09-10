//! Receipt rebuild, export, list, show, and acknowledge (§12.2).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::Paths;
use crate::journal::{
    CandidateRecord, IntendedPayload, JournalError, JournalRow, LockError, PostedRecord,
    ReadbackRecord, State, acknowledge as journal_acknowledge, get_journal, is_superseded,
    owner_status_for,
};
use crate::receipts::document::{ReceiptDocument, ReceiptFile, ReceiptIdentity, ReceiptText};
use crate::store::{DbError, Store};

/// Receipt domain errors.
#[derive(Debug, Error)]
pub enum ReceiptError {
    /// Journal or receipt missing.
    #[error("not found")]
    NotFound,
    /// Journal is not in a receipt-bearing state.
    #[error("refused")]
    Refused,
    /// Expected-state or payload conflict.
    #[error("state conflict")]
    StateConflict,
    /// Damaged or incomplete receipt data.
    #[error("corrupt receipt")]
    Corrupt,
    /// Journal error.
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// Store / `SQLite` error.
    #[error(transparent)]
    Store(DbError),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<DbError> for ReceiptError {
    fn from(error: DbError) -> Self {
        if matches!(&error, DbError::Message(s) if s == "state conflict") {
            Self::StateConflict
        } else {
            Self::Store(error)
        }
    }
}

impl From<rusqlite::Error> for ReceiptError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Store(DbError::from(value))
    }
}

impl From<LockError> for ReceiptError {
    fn from(value: LockError) -> Self {
        Self::Journal(JournalError::from(value))
    }
}

/// Filters for `receipts list` (class B: numeric course id only).
#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    /// Course id when set.
    pub course_id: Option<i64>,
    /// Journal state when set.
    pub state: Option<State>,
}

/// One journal row for `receipts list|show` (Appendix D `Journal`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSummary {
    /// Journal id.
    pub journal_id: String,
    /// Lifecycle state.
    pub state: String,
    /// Owner probe: `live` / `absent` / `n/a`.
    pub owner: String,
    /// Later confirmed attempt for the same assignment.
    pub superseded: bool,
    /// Acknowledge timestamp when set.
    #[serde(default)]
    pub acknowledged_at: Option<String>,
    /// Course id (string).
    pub course_id: String,
    /// Course code from frozen intent.
    #[serde(default)]
    pub course_code: Option<String>,
    /// Assignment id (string).
    pub assignment_id: String,
    /// Assignment name from frozen intent.
    #[serde(default)]
    pub assignment_name: Option<String>,
    /// Submission kind.
    pub kind: String,
    /// Baseline attempt.
    pub baseline_attempt: i64,
    /// Row creation time.
    pub created_at: String,
    /// Latest journal timestamp.
    pub updated_at: String,
    /// Uploaded Canvas file ids.
    pub uploaded_file_ids: Vec<String>,
    /// HTTP post status when recorded.
    #[serde(default)]
    pub post_status: Option<i64>,
    /// Response kind when recorded.
    #[serde(default)]
    pub response_kind: Option<String>,
    /// Not-submitted evidence when recorded.
    #[serde(default)]
    pub not_submitted_evidence: Option<String>,
    /// Posted allowlist when present.
    #[serde(default)]
    pub posted: Option<PostedRecord>,
    /// Readback when present.
    #[serde(default)]
    pub readback: Option<ReadbackRecord>,
    /// Server match candidate when present.
    #[serde(default)]
    pub server_match: Option<CandidateRecord>,
    /// Receipt id when a receipt exists.
    #[serde(default)]
    pub receipt_id: Option<String>,
    /// Error text when present.
    #[serde(default)]
    pub error: Option<String>,
}

/// Result of `receipts show`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowResult {
    /// Journal summary.
    pub journal: JournalSummary,
    /// Receipt when the journal is `submitted` or `matched`.
    #[serde(default)]
    pub receipt: Option<ReceiptDocument>,
}

/// Result of `receipts export`.
#[derive(Debug, Clone)]
pub struct ExportResult {
    /// Receipt id written or streamed.
    pub receipt_id: String,
    /// Path when written to disk (`None` for stdout).
    pub path: Option<PathBuf>,
    /// Byte length of the JSON document.
    pub bytes: usize,
    /// Document bytes when `out` is `Some("-")`.
    pub body: Option<Vec<u8>>,
}

/// Result of `receipts acknowledge`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeResult {
    /// Journal id.
    pub journal_id: String,
    /// Acknowledge timestamp.
    pub acknowledged_at: String,
}

/// Rebuild a `receipt@1` document for a `submitted` or `matched` journal.
///
/// Prefers the stored `receipt_record_json`. Recomputes `text.server_body_sha256`.
pub fn rebuild_from_journal(
    store: &Store,
    journal_id: &str,
) -> Result<ReceiptDocument, ReceiptError> {
    let row = get_journal(store, journal_id)?.ok_or(ReceiptError::NotFound)?;
    if !matches!(row.state, State::Submitted | State::Matched) {
        return Err(ReceiptError::Refused);
    }
    if let Some(raw) = row.receipt_record_json.as_deref()
        && let Ok(mut doc) = serde_json::from_str::<ReceiptDocument>(raw)
    {
        doc.recompute_server_body_sha256();
        return Ok(doc);
    }
    rebuild_from_row(store, &row)
}

/// Write or stream a receipt export. Class B: no locks, no state change.
///
/// `out = None` writes `<identity dir>/receipts/<receipt_id>.json` (mode `0600`).
/// `out = Some("-")` returns the bytes in [`ExportResult::body`].
/// `out = Some(path)` writes that path (mode `0600`).
pub fn export(
    store: &Store,
    paths: &Paths,
    journal_id_or_receipt_id: &str,
    out: Option<&Path>,
) -> Result<ExportResult, ReceiptError> {
    let jid = resolve_journal_id(store, journal_id_or_receipt_id)?;
    let doc = rebuild_from_journal(store, &jid)?;
    let bytes = serde_json::to_vec_pretty(&doc)?;
    let receipt_id = doc.receipt_id.clone();

    if out.is_some_and(|p| p == Path::new("-")) {
        let len = bytes.len();
        return Ok(ExportResult {
            receipt_id,
            path: None,
            bytes: len,
            body: Some(bytes),
        });
    }

    let path = if let Some(p) = out {
        p.to_path_buf()
    } else {
        let dir = paths.identity_dir.join("receipts");
        fs::create_dir_all(&dir)?;
        dir.join(format!("{receipt_id}.json"))
    };
    write_mode_0600(&path, &bytes)?;
    Ok(ExportResult {
        receipt_id,
        path: Some(path),
        bytes: bytes.len(),
        body: None,
    })
}

/// List journals for an identity, newest first. Class B: never lock, never transition.
pub fn list_journals(
    store: &Store,
    identity_dir: &Path,
    filter: &ListFilter,
) -> Result<Vec<JournalSummary>, ReceiptError> {
    let course_id = filter.course_id;
    let state_filter = filter.state.map(|s| s.as_str().to_string());
    let rows = store.call_blocking(move |conns| {
        let ids = match (course_id, state_filter.as_deref()) {
            (Some(cid), Some(state)) => {
                let mut stmt = conns.state.prepare(
                    "SELECT journal_id FROM submission_journal
                     WHERE course_id = ?1 AND state = ?2
                     ORDER BY created_at DESC, journal_id ASC",
                )?;
                stmt.query_map(params![cid, state], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            }
            (Some(cid), None) => {
                let mut stmt = conns.state.prepare(
                    "SELECT journal_id FROM submission_journal
                     WHERE course_id = ?1
                     ORDER BY created_at DESC, journal_id ASC",
                )?;
                stmt.query_map(params![cid], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            }
            (None, Some(state)) => {
                let mut stmt = conns.state.prepare(
                    "SELECT journal_id FROM submission_journal
                     WHERE state = ?1
                     ORDER BY created_at DESC, journal_id ASC",
                )?;
                stmt.query_map(params![state], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            }
            (None, None) => {
                let mut stmt = conns.state.prepare(
                    "SELECT journal_id FROM submission_journal
                     ORDER BY created_at DESC, journal_id ASC",
                )?;
                stmt.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(ids)
    })?;

    let mut out = Vec::with_capacity(rows.len());
    for jid in rows {
        out.push(summarize(store, identity_dir, &jid)?);
    }
    Ok(out)
}

/// Show a journal by journal id or receipt id. Class B: never lock, never transition.
pub fn show(store: &Store, identity_dir: &Path, id: &str) -> Result<ShowResult, ReceiptError> {
    let jid = resolve_journal_id(store, id)?;
    let journal = summarize(store, identity_dir, &jid)?;
    let receipt = if matches!(
        State::from_str(&journal.state),
        Ok(State::Submitted | State::Matched)
    ) {
        Some(rebuild_from_journal(store, &jid)?)
    } else {
        None
    };
    Ok(ShowResult { journal, receipt })
}

/// Acknowledge an `outcome_unknown` journal (pending hook). Delegates to the journal.
pub fn acknowledge(store: &Store, journal_id: &str) -> Result<AcknowledgeResult, ReceiptError> {
    journal_acknowledge(store, journal_id)?;
    let row = get_journal(store, journal_id)?.ok_or(ReceiptError::NotFound)?;
    let acknowledged_at = row.acknowledged_at.ok_or(ReceiptError::StateConflict)?;
    Ok(AcknowledgeResult {
        journal_id: journal_id.to_string(),
        acknowledged_at,
    })
}

fn resolve_journal_id(store: &Store, id: &str) -> Result<String, ReceiptError> {
    if get_journal(store, id)?.is_some() {
        return Ok(id.to_string());
    }
    let needle = id.to_string();
    let found = store.call_blocking(move |conns| {
        conns
            .state
            .query_row(
                "SELECT journal_id FROM submission_journal
                 WHERE receipt_record_json IS NOT NULL
                   AND json_extract(receipt_record_json, '$.receipt_id') = ?1",
                params![needle],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(DbError::from)
    })?;
    found.ok_or(ReceiptError::NotFound)
}

fn summarize(
    store: &Store,
    identity_dir: &Path,
    journal_id: &str,
) -> Result<JournalSummary, ReceiptError> {
    let row = get_journal(store, journal_id)?.ok_or(ReceiptError::NotFound)?;
    let owner = owner_status_for(identity_dir, journal_id, row.state)?
        .as_str()
        .to_string();
    let superseded = is_superseded(store, journal_id)?;
    let intent: IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
    let uploaded_file_ids: Vec<String> = {
        let ids: Vec<serde_json::Value> =
            serde_json::from_str(&row.uploaded_file_ids_json).unwrap_or_default();
        ids.into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                serde_json::Value::Number(n) => n.as_u64().map(|n| n.to_string()),
                _ => None,
            })
            .collect()
    };
    let posted = row
        .response_record_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let readback = row
        .readback_record_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let server_match = row
        .server_match_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let receipt_id = row.receipt_record_json.as_deref().and_then(|raw| {
        serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|v| {
                v.get("receipt_id")
                    .and_then(|id| id.as_str())
                    .map(str::to_owned)
            })
    });
    Ok(JournalSummary {
        journal_id: row.journal_id.clone(),
        state: row.state.as_str().to_string(),
        owner,
        superseded,
        acknowledged_at: row.acknowledged_at.clone(),
        course_id: row.course_id.to_string(),
        course_code: intent.course_code,
        assignment_id: row.assignment_id.to_string(),
        assignment_name: intent.assignment_name,
        kind: row.kind.clone(),
        baseline_attempt: row.baseline_attempt.unwrap_or(0),
        created_at: row.created_at.clone(),
        updated_at: updated_at(&row),
        uploaded_file_ids,
        post_status: row.post_status,
        response_kind: row.response_kind.clone(),
        not_submitted_evidence: row.not_submitted_evidence.clone(),
        posted,
        readback,
        server_match,
        receipt_id,
        error: row.error_text.clone(),
    })
}

fn updated_at(row: &JournalRow) -> String {
    let candidates: [Option<&str>; 14] = [
        Some(row.created_at.as_str()),
        row.planned_at.as_deref(),
        row.uploading_at.as_deref(),
        row.uploaded_at.as_deref(),
        row.posting_at.as_deref(),
        row.posting_started_at.as_deref(),
        row.submitted_at.as_deref(),
        row.matched_at.as_deref(),
        row.terminal_at.as_deref(),
        row.upload_incomplete_at.as_deref(),
        row.uploaded_not_submitted_at.as_deref(),
        row.outcome_unknown_at.as_deref(),
        row.refused_at.as_deref(),
        row.acknowledged_at.as_deref(),
    ];
    candidates
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(row.created_at.as_str())
        .to_string()
}

fn rebuild_from_row(store: &Store, row: &JournalRow) -> Result<ReceiptDocument, ReceiptError> {
    let posted: PostedRecord = serde_json::from_str(
        row.response_record_json
            .as_deref()
            .ok_or(ReceiptError::Corrupt)?,
    )?;
    let readback = row
        .readback_record_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let intent: IntendedPayload = serde_json::from_str(&row.intended_payload_json)?;
    let identity = load_identity(store)?;
    let receipt_id = row
        .receipt_record_json
        .as_deref()
        .and_then(|raw| {
            serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| {
                    v.get("receipt_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_owned)
                })
        })
        .ok_or(ReceiptError::Corrupt)?;
    let attribution = match row.state {
        State::Submitted => "observed".to_string(),
        State::Matched => "unproven".to_string(),
        _ => return Err(ReceiptError::Refused),
    };
    let files = intent
        .files
        .into_iter()
        .map(|f| ReceiptFile {
            name: f.name,
            size: f.size,
            sha256: f.sha256,
            canvas_file_id: f.canvas_file_id,
        })
        .collect();
    let text = intent.text.map(|t| ReceiptText {
        input_sha256: t.input_sha256,
        transform: t.transform,
        sent_sha256: t.sent_sha256,
        server_body_sha256: None,
    });
    let mut doc = ReceiptDocument {
        receipt_id,
        journal_id: row.journal_id.clone(),
        identity,
        course_id: row.course_id.to_string(),
        course_code: intent.course_code,
        assignment_id: row.assignment_id.to_string(),
        assignment_name: intent.assignment_name,
        kind: row.kind.clone(),
        baseline_attempt: row.baseline_attempt.unwrap_or(0),
        attribution,
        posted,
        readback,
        files,
        text,
        url: intent.url,
        due_at: intent.due_at,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: row.created_at.clone(),
    };
    doc.recompute_server_body_sha256();
    Ok(doc)
}

fn load_identity(store: &Store) -> Result<ReceiptIdentity, ReceiptError> {
    Ok(store.call_blocking(|conns| {
        let origin: String =
            conns
                .state
                .query_row("SELECT value FROM identity WHERE key = 'origin'", [], |r| {
                    r.get(0)
                })?;
        let user_id: String = conns.state.query_row(
            "SELECT value FROM identity WHERE key = 'user_id'",
            [],
            |r| r.get(0),
        )?;
        let key: String =
            conns
                .state
                .query_row("SELECT value FROM identity WHERE key = 'key'", [], |r| {
                    r.get(0)
                })?;
        Ok(ReceiptIdentity {
            origin,
            user_id,
            key,
        })
    })?)
}

/// Rebuild a document from a loaded journal row (submitted/matched only).
pub fn document_from_row(row: &JournalRow) -> Result<ReceiptDocument, ReceiptError> {
    if !matches!(row.state, State::Submitted | State::Matched) {
        return Err(ReceiptError::Refused);
    }
    if let Some(raw) = row.receipt_record_json.as_deref()
        && let Ok(mut doc) = serde_json::from_str::<ReceiptDocument>(raw)
    {
        doc.recompute_server_body_sha256();
        return Ok(doc);
    }
    Err(ReceiptError::Corrupt)
}

/// Parse a receipt document from JSON bytes.
pub fn parse_document(bytes: &[u8]) -> Result<ReceiptDocument, ReceiptError> {
    let mut doc: ReceiptDocument = serde_json::from_slice(bytes)?;
    doc.recompute_server_body_sha256();
    Ok(doc)
}

/// Default on-disk path for a receipt export.
#[must_use]
pub fn receipt_path(identity_dir: &Path, receipt_id: &str) -> PathBuf {
    identity_dir
        .join("receipts")
        .join(format!("{receipt_id}.json"))
}

/// Export a journal's receipt to the default identity receipts directory.
pub fn export_journal(
    store: &Store,
    identity_dir: &Path,
    journal_id: &str,
) -> Result<ExportResult, ReceiptError> {
    let doc = rebuild_from_journal(store, journal_id)?;
    let bytes = serde_json::to_vec_pretty(&doc)?;
    let receipt_id = doc.receipt_id.clone();
    let dir = identity_dir.join("receipts");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{receipt_id}.json"));
    write_mode_0600(&path, &bytes)?;
    Ok(ExportResult {
        receipt_id,
        path: Some(path),
        bytes: bytes.len(),
        body: None,
    })
}

fn write_mode_0600(path: &Path, bytes: &[u8]) -> Result<(), ReceiptError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".receipt-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<(), std::io::Error> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{IdentityDocument, Paths};
    use crate::journal::{
        AdmissionLock, CreateOpts, Evidence, OwnerLock, ReadbackRecord, ReceiptRecord, State,
        TransitionPatch, allowlist_from_json, append_uploaded_file_id, commit_success, create,
        enrich_readback, mark_posting, recover_if_owner_absent, transition,
    };
    use crate::store::OpenIdentity;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Paths, Store, IdentityDocument) {
        let dir = TempDir::new().unwrap();
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path(), &doc.key);
        std::fs::create_dir_all(&paths.identity_dir).unwrap();
        std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        (dir, paths, open.store, doc)
    }

    fn opts_for(
        doc: &IdentityDocument,
        assignment: i64,
        payload: &serde_json::Value,
    ) -> CreateOpts {
        CreateOpts {
            identity_key: doc.key.to_string(),
            course_id: 1,
            assignment_id: assignment,
            kind: "online_upload".into(),
            intended_payload_json: payload.to_string(),
            baseline_attempt: Some(0),
            baseline_submission_id: None,
        }
    }

    fn to_posting(
        store: &Store,
        paths: &Paths,
        doc: &IdentityDocument,
        assignment: i64,
        payload: &serde_json::Value,
    ) -> (String, OwnerLock) {
        let file_count = payload
            .get("files")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, assignment).unwrap();
        let (jid, owner) = create(
            store,
            &paths.identity_dir,
            &admission,
            &opts_for(doc, assignment, payload),
        )
        .unwrap();
        drop(admission);
        if file_count > 0 {
            transition(
                store,
                &owner,
                &jid,
                State::Planned,
                State::Uploading,
                TransitionPatch::default(),
            )
            .unwrap();
            for index in 0..file_count {
                let file_id = 100 + i64::try_from(index).unwrap();
                append_uploaded_file_id(store, &owner, &jid, index, file_id).unwrap();
            }
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
        mark_posting(store, &owner, &jid).unwrap();
        (jid, owner)
    }

    fn commit_file(
        store: &Store,
        paths: &Paths,
        doc: &IdentityDocument,
        assignment: i64,
    ) -> (String, String) {
        let payload = json!({
            "files": [{"name": "a.pdf", "size": 3, "sha256": "aa".repeat(32)}],
            "course_code": "CHEM301",
            "assignment_name": "PS3"
        });
        let (jid, owner) = to_posting(store, paths, doc, assignment, &payload);
        let raw = br#"{"id":5,"attempt":1,"attachments":[{"id":100,"display_name":"a.pdf","size":3,"content_type":"application/pdf"}]}"#;
        let posted = allowlist_from_json(
            Evidence::PostResponse,
            &serde_json::from_slice(raw).unwrap(),
            Some(raw),
        )
        .unwrap();
        let receipt_id = format!("r-{jid}");
        let receipt = ReceiptRecord {
            receipt_id: receipt_id.clone(),
            journal_id: jid.clone(),
            attribution: "observed".into(),
            posted,
            readback: None,
        };
        commit_success(store, &owner, &jid, 201, &receipt).unwrap();
        (jid, receipt_id)
    }

    #[test]
    fn rebuild_parses_stored_receipt() {
        let (_dir, paths, store, doc) = setup();
        let (jid, receipt_id) = commit_file(&store, &paths, &doc, 42);
        let doc = rebuild_from_journal(&store, &jid).unwrap();
        assert_eq!(doc.receipt_id, receipt_id);
        assert_eq!(doc.journal_id, jid);
        assert_eq!(doc.course_id, "1");
        assert_eq!(doc.assignment_id, "42");
        assert_eq!(doc.course_code.as_deref(), Some("CHEM301"));
        assert_eq!(doc.files[0].name, "a.pdf");
        assert_eq!(doc.attribution, "observed");
        assert!(doc.readback.is_none());
    }

    #[test]
    fn rebuild_refuses_non_receipt_states() {
        let (_dir, paths, store, doc) = setup();
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 9).unwrap();
        let (jid, _owner) = create(
            &store,
            &paths.identity_dir,
            &admission,
            &opts_for(&doc, 9, &json!({})),
        )
        .unwrap();
        assert!(matches!(
            rebuild_from_journal(&store, &jid),
            Err(ReceiptError::Refused)
        ));
    }

    #[test]
    fn export_writes_0600_and_stdout() {
        let (_dir, paths, store, doc) = setup();
        let (jid, receipt_id) = commit_file(&store, &paths, &doc, 11);
        let written = export(&store, &paths, &jid, None).unwrap();
        assert_eq!(written.receipt_id, receipt_id);
        let path = written.path.unwrap();
        assert_eq!(
            path,
            paths
                .identity_dir
                .join("receipts")
                .join(format!("{receipt_id}.json"))
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let disk: ReceiptDocument = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk.receipt_id, receipt_id);

        let streamed = export(&store, &paths, &receipt_id, Some(Path::new("-"))).unwrap();
        assert!(streamed.path.is_none());
        let body = streamed.body.unwrap();
        assert_eq!(body.len(), streamed.bytes);
        let parsed: ReceiptDocument = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.receipt_id, receipt_id);
    }

    #[test]
    fn export_refuses_non_receipt_states() {
        let (_dir, paths, store, doc) = setup();
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 21).unwrap();
        let (jid, owner) = create(
            &store,
            &paths.identity_dir,
            &admission,
            &opts_for(&doc, 21, &json!({})),
        )
        .unwrap();
        drop(admission);
        drop(owner);
        let recovered = recover_if_owner_absent(&store, &paths.identity_dir, &jid)
            .unwrap()
            .unwrap();
        assert_eq!(recovered, State::Refused);
        assert!(matches!(
            export(&store, &paths, &jid, None),
            Err(ReceiptError::Refused)
        ));
    }

    #[test]
    fn list_newest_first_and_filters() {
        let (_dir, paths, store, doc) = setup();
        let (j1, _) = commit_file(&store, &paths, &doc, 31);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let (j2, _) = commit_file(&store, &paths, &doc, 32);
        let all = list_journals(&store, &paths.identity_dir, &ListFilter::default()).unwrap();
        assert!(all.len() >= 2);
        assert_eq!(all[0].journal_id, j2);
        assert_eq!(all[1].journal_id, j1);
        assert_eq!(all[0].owner, "n/a");
        assert!(all[0].receipt_id.is_some());

        let filtered = list_journals(
            &store,
            &paths.identity_dir,
            &ListFilter {
                course_id: Some(1),
                state: Some(State::Submitted),
            },
        )
        .unwrap();
        assert!(filtered.iter().all(|j| j.state == "submitted"));
    }

    #[test]
    fn show_by_receipt_id_and_acknowledge() {
        let (_dir, paths, store, doc) = setup();
        let (jid, receipt_id) = commit_file(&store, &paths, &doc, 41);
        let shown = show(&store, &paths.identity_dir, &receipt_id).unwrap();
        assert_eq!(shown.journal.journal_id, jid);
        assert_eq!(shown.receipt.as_ref().unwrap().receipt_id, receipt_id);

        // Acknowledge requires outcome_unknown.
        let payload = json!({"files":[{"name":"a","size":1,"sha256":"cc".repeat(32)}]});
        let (unknown, owner) = to_posting(&store, &paths, &doc, 42, &payload);
        drop(owner);
        recover_if_owner_absent(&store, &paths.identity_dir, &unknown)
            .unwrap()
            .unwrap();
        let row = get_journal(&store, &unknown).unwrap().unwrap();
        assert_eq!(row.state, State::OutcomeUnknown);
        let ack = acknowledge(&store, &unknown).unwrap();
        assert_eq!(ack.journal_id, unknown);
        assert!(!ack.acknowledged_at.is_empty());
        let listed = list_journals(
            &store,
            &paths.identity_dir,
            &ListFilter {
                course_id: None,
                state: Some(State::OutcomeUnknown),
            },
        )
        .unwrap();
        let entry = listed.iter().find(|j| j.journal_id == unknown).unwrap();
        assert!(entry.acknowledged_at.is_some());
    }

    #[test]
    fn server_body_sha256_prefers_posted_then_readback() {
        let (_dir, paths, store, doc) = setup();
        let payload = json!({
            "text": {
                "input_sha256": "11".repeat(32),
                "transform": "plain",
                "sent_sha256": "22".repeat(32),
                "outbound_bytes": "hello"
            }
        });
        let (jid, owner) = to_posting(&store, &paths, &doc, 50, &payload);
        let raw = br#"{"id":9,"attempt":1,"body":"server","attachments":[]}"#;
        let mut posted = allowlist_from_json(
            Evidence::PostResponse,
            &serde_json::from_slice(raw).unwrap(),
            Some(raw),
        )
        .unwrap();
        // Drop posted body digest so readback supplies it after enrichment.
        posted.body_sha256 = None;
        let receipt = ReceiptRecord {
            receipt_id: "r-text".into(),
            journal_id: jid.clone(),
            attribution: "observed".into(),
            posted,
            readback: None,
        };
        commit_success(&store, &owner, &jid, 201, &receipt).unwrap();
        let before = rebuild_from_journal(&store, &jid).unwrap();
        assert!(before.text.as_ref().unwrap().server_body_sha256.is_none());

        let readback = ReadbackRecord {
            submitted_at: Some("2026-01-01T00:00:00Z".into()),
            submitted_at_local: None,
            late: Some(false),
            attachments: vec![],
            body_sha256: Some("33".repeat(32)),
        };
        enrich_readback(&store, &owner, &jid, 1, &readback).unwrap();
        let after = rebuild_from_journal(&store, &jid).unwrap();
        let expected = "33".repeat(32);
        assert_eq!(
            after.text.as_ref().unwrap().server_body_sha256.as_deref(),
            Some(expected.as_str())
        );
        assert!(!serde_json::to_string(&after).unwrap().contains("hello"));
    }
}

//! Execute steps 8–12 (§12.2).

use canvas_api::upload::{UploadMeta, upload_submission_file};
use canvas_api::{
    Client, SubmissionBody, get_submission_history, is_canvas_error_body, post_submission,
    sanitize_post_error_text,
};
use jiff::Timestamp;
use thiserror::Error;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::identity::Paths;
use crate::journal::{
    Evidence, JournalError, JournalRow, OwnerLock, ReceiptRecord, ResponseKind, State,
    TransitionPatch, allowlist_from_json, append_uploaded_file_id, commit_success,
    enrich_readback_full, get_journal, mark_posting, transition,
};
use crate::receipts::export;
use crate::store::Store;
use crate::submit::freeze::{FrozenInput, InputKind};
use crate::submit::reconcile::{ReconcileResult, history_entry_json, reconcile_history};

/// Execute-phase errors.
#[derive(Debug, Error)]
pub enum ExecuteError {
    #[error(transparent)]
    Network(#[from] canvas_api::Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Refused(String),
}

/// Result of execute steps 8–12.
#[derive(Debug)]
pub struct ExecuteOutcome {
    /// Final journal state.
    pub state: State,
    /// Journal id.
    pub journal_id: String,
    /// Receipt id when submitted/matched.
    pub receipt_id: Option<String>,
    /// Attribution when confirmed.
    pub attribution: Option<String>,
    /// HTTP post status when known.
    pub post_status: Option<i64>,
    /// Response kind when unknown.
    pub response_kind: Option<ResponseKind>,
    /// Immediate reconcile result when POST was not an observed success.
    pub reconcile: Option<ReconcileResult>,
    /// Soft warning (e.g. readback/export failure).
    pub warning: Option<String>,
}

/// Run uploads → POST → commit / `outcome_unknown` → optional reconcile → readback → export.
pub async fn execute(
    client: &Client,
    store: &Store,
    paths: &Paths,
    owner: &OwnerLock,
    journal_id: &str,
    frozen: &FrozenInput,
) -> Result<ExecuteOutcome, ExecuteError> {
    // Step 8: uploads
    match frozen.kind {
        InputKind::OnlineUpload => {
            transition(
                store,
                owner,
                journal_id,
                State::Planned,
                State::Uploading,
                TransitionPatch::default(),
            )?;
            upload_all(client, store, owner, journal_id, frozen).await?;
            transition(
                store,
                owner,
                journal_id,
                State::Uploading,
                State::Uploaded,
                TransitionPatch::default(),
            )?;
        }
        InputKind::OnlineTextEntry | InputKind::OnlineHtml | InputKind::OnlineUrl => {
            transition(
                store,
                owner,
                journal_id,
                State::Planned,
                State::Uploaded,
                TransitionPatch::default(),
            )?;
        }
    }

    post_and_finish(client, store, paths, owner, journal_id, frozen).await
}

/// Steps 9–12 from an `uploaded` journal (also used by tests that seed uploads).
#[allow(clippy::too_many_lines)]
pub async fn post_and_finish(
    client: &Client,
    store: &Store,
    paths: &Paths,
    owner: &OwnerLock,
    journal_id: &str,
    frozen: &FrozenInput,
) -> Result<ExecuteOutcome, ExecuteError> {
    // Step 9: posting + POST
    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let body = build_post_body(&row, frozen)?;
    mark_posting(store, owner, journal_id)?;
    let post_result = post_submission(client, row.course_id, row.assignment_id, &body).await;

    let (status, bytes, transport_err) = match post_result {
        Ok((status, _headers, bytes, _url)) => (Some(status.as_u16()), Some(bytes), None),
        Err(e) => (None, None, Some(e)),
    };

    // Observed success: 2xx + decodable attempt
    if let (Some(status), Some(bytes)) = (status, bytes.as_ref())
        && (200..300).contains(&status)
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes)
        && value
            .get("attempt")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|a| a >= 1)
        && let Ok(posted) = allowlist_from_json(Evidence::PostResponse, &value, Some(bytes))
    {
        let receipt = ReceiptRecord {
            receipt_id: Uuid::new_v4().to_string(),
            journal_id: journal_id.to_owned(),
            attribution: "observed".into(),
            posted: posted.clone(),
            readback: None,
        };
        commit_success(store, owner, journal_id, status, &receipt)?;

        // Step 11: readback
        let mut warning = None;
        if let Some(attempt) = posted.attempt {
            match get_submission_history(client, row.course_id, row.assignment_id).await {
                Ok(history) => {
                    if let Some(entry) = history
                        .submission_history
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .find(|e| e.attempt.as_value().copied() == Some(attempt))
                    {
                        let record = allowlist_from_json(
                            Evidence::HistoryFiles,
                            &history_entry_json(entry),
                            None,
                        )?;
                        if let Err(e) = enrich_readback_full(store, owner, journal_id, &record) {
                            warning = Some(format!("readback enrichment failed: {e}"));
                        }
                    } else {
                        warning = Some("readback has no entry for posted attempt".into());
                    }
                }
                Err(e) => warning = Some(format!("readback fetch failed: {e}")),
            }
        }

        // Step 12: export
        if let Err(e) = export(store, paths, journal_id, None) {
            let msg = format!("receipt export failed: {e}");
            warning = Some(match warning {
                Some(w) => format!("{w}; {msg}"),
                None => msg,
            });
        }

        return Ok(ExecuteOutcome {
            state: State::Submitted,
            journal_id: journal_id.to_owned(),
            receipt_id: Some(receipt.receipt_id),
            attribution: Some("observed".into()),
            post_status: Some(i64::from(status)),
            response_kind: None,
            reconcile: None,
            warning,
        });
    }

    // Every other outcome → outcome_unknown (never auto-negative).
    let (post_status, response_kind, error_text) =
        classify_unknown(status, bytes.as_deref(), transport_err);
    transition(
        store,
        owner,
        journal_id,
        State::Posting,
        State::OutcomeUnknown,
        TransitionPatch {
            post_status: Some(post_status),
            response_kind: Some(response_kind),
            error_text: Some(error_text),
            ..TransitionPatch::default()
        },
    )?;

    // Step 9b: immediate reconcile when a response was received.
    let mut reconcile = None;
    if status.is_some() {
        let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
        if let Ok(history) = get_submission_history(client, row.course_id, row.assignment_id).await
        {
            let now = Timestamp::now();
            reconcile = Some(
                reconcile_history(store, owner, paths, &row, &history, now).map_err(
                    |e| match e {
                        super::reconcile::ReconcileError::Journal(e) => ExecuteError::Journal(e),
                        super::reconcile::ReconcileError::Json(e) => ExecuteError::Json(e),
                        super::reconcile::ReconcileError::Io(e) => ExecuteError::Io(e),
                        super::reconcile::ReconcileError::Lock(e) => {
                            ExecuteError::Journal(e.into())
                        }
                        super::reconcile::ReconcileError::Network(e) => ExecuteError::Network(e),
                    },
                )?,
            );
        }
    }

    let row = get_journal(store, journal_id)?.ok_or(JournalError::NotFound)?;
    let (state, receipt_id, attribution) = if matches!(row.state, State::Matched) {
        let receipt_id = row
            .receipt_record_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| {
                v.get("receipt_id")
                    .and_then(|x| x.as_str())
                    .map(str::to_owned)
            });
        (row.state, receipt_id, Some("unproven".into()))
    } else {
        (row.state, None, None)
    };

    Ok(ExecuteOutcome {
        state,
        journal_id: journal_id.to_owned(),
        receipt_id,
        attribution,
        post_status: row.post_status,
        response_kind: Some(response_kind),
        reconcile,
        warning: None,
    })
}

async fn upload_all(
    client: &Client,
    store: &Store,
    owner: &OwnerLock,
    journal_id: &str,
    frozen: &FrozenInput,
) -> Result<(), ExecuteError> {
    let course_id = get_journal(store, journal_id)?
        .ok_or(JournalError::NotFound)?
        .course_id;
    let assignment_id = get_journal(store, journal_id)?
        .ok_or(JournalError::NotFound)?
        .assignment_id;

    // Concurrency 2 via JoinSet; journal appends stay serialized on the store.
    let mut set = JoinSet::new();
    let mut next = 0usize;
    let total = frozen.file_paths.len();

    while next < total || !set.is_empty() {
        while set.len() < 2 && next < total {
            let idx = next;
            next += 1;
            let path = frozen.file_paths[idx].clone();
            let meta = UploadMeta {
                name: frozen.payload.files[idx].name.clone(),
                size: frozen.payload.files[idx].size,
                content_type: "application/octet-stream".into(),
            };
            let expected = frozen.payload.files[idx].sha256.clone();
            let client = client.clone();
            set.spawn(async move {
                let file = tokio::fs::File::open(&path).await?;
                if file.metadata().await?.len() != meta.size {
                    return Err(ExecuteError::Refused(
                        "file size differs from frozen input".into(),
                    ));
                }
                let result =
                    upload_submission_file(&client, course_id, assignment_id, &meta, file).await?;
                let got = hex_sha256_bytes(&result.sha256);
                if got != expected {
                    return Err(ExecuteError::Refused(
                        "streamed upload hash differs from frozen hash".into(),
                    ));
                }
                Ok::<_, ExecuteError>((idx, result.file_id))
            });
        }
        match set.join_next().await {
            Some(Ok(Ok((idx, file_id)))) => {
                append_uploaded_file_id(store, owner, journal_id, idx, file_id)?;
            }
            Some(Ok(Err(e))) => {
                let to = if matches!(&e, ExecuteError::Refused(_)) {
                    State::Refused
                } else {
                    State::UploadIncomplete
                };
                transition(
                    store,
                    owner,
                    journal_id,
                    State::Uploading,
                    to,
                    TransitionPatch {
                        error_text: Some(e.to_string()),
                        ..TransitionPatch::default()
                    },
                )?;
                set.abort_all();
                while set.join_next().await.is_some() {}
                return Err(e);
            }
            Some(Err(e)) => {
                transition(
                    store,
                    owner,
                    journal_id,
                    State::Uploading,
                    State::UploadIncomplete,
                    TransitionPatch {
                        error_text: Some(e.to_string()),
                        ..TransitionPatch::default()
                    },
                )?;
                set.abort_all();
                while set.join_next().await.is_some() {}
                return Err(ExecuteError::Refused(format!("upload task failed: {e}")));
            }
            None => break,
        }
    }
    Ok(())
}

fn hex_sha256_bytes(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn build_post_body(row: &JournalRow, frozen: &FrozenInput) -> Result<SubmissionBody, ExecuteError> {
    let comment = frozen.payload.comment.clone();
    Ok(match frozen.kind {
        InputKind::OnlineUpload => {
            let ids: Vec<i64> = serde_json::from_str(&row.uploaded_file_ids_json)?;
            SubmissionBody::OnlineUpload {
                file_ids: ids,
                comment,
            }
        }
        InputKind::OnlineTextEntry | InputKind::OnlineHtml => {
            let text = frozen
                .payload
                .text
                .as_ref()
                .ok_or_else(|| ExecuteError::Refused("missing text payload".into()))?;
            SubmissionBody::OnlineTextEntry {
                body: text.outbound_bytes.clone(),
                comment,
            }
        }
        InputKind::OnlineUrl => SubmissionBody::OnlineUrl {
            url: frozen
                .payload
                .url
                .clone()
                .ok_or_else(|| ExecuteError::Refused("missing url payload".into()))?,
            comment,
        },
    })
}

fn classify_unknown(
    status: Option<u16>,
    bytes: Option<&[u8]>,
    transport_err: Option<canvas_api::Error>,
) -> (Option<i64>, ResponseKind, String) {
    if let Some(err) = transport_err {
        return (None, ResponseKind::None, err.to_string());
    }
    let status_code = status.map(i64::from);
    let bytes = bytes.unwrap_or_default();
    if is_canvas_error_body(bytes) {
        (
            status_code,
            ResponseKind::CanvasError,
            sanitize_post_error_text(bytes),
        )
    } else {
        (
            status_code,
            ResponseKind::Other,
            sanitize_post_error_text(bytes),
        )
    }
}

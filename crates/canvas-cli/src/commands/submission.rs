//! `canvas submission` (show / verify / reconcile).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::submit::{
    ReconcileError, ReconcileOutcome, VerifyError, VerifyOutcome, load_receipt_for_verify,
    reconcile, validate_receipt, verify,
};
use jiff::Timestamp;
use serde::Serialize;

use super::Globals;
use super::emit::{base_envelope, emit_error, require_client, session_error};
use super::handled::Handled;
use crate::output::{Outcome, SCHEMA_RECONCILE, SCHEMA_SUBMISSION, SCHEMA_VERIFY};

/// Submission subcommand operands after clap parsing.
pub enum SubmissionCmd {
    Show {
        course: String,
        assignment: Option<String>,
        history: bool,
    },
    Verify {
        receipt_id: String,
    },
    Reconcile {
        journal_id: String,
        assume_not_submitted: bool,
    },
}

#[derive(Debug, Serialize)]
struct ReconcileJson {
    outcome: String,
    state: String,
    journal_id: String,
    owner: String,
    response_kind: Option<String>,
    not_submitted_evidence: Option<String>,
    assume_available: bool,
    attribution: Option<String>,
    receipt_id: Option<String>,
    posted: Option<serde_json::Value>,
    server_match: Option<serde_json::Value>,
    candidates: Vec<serde_json::Value>,
    message: String,
}

#[derive(Debug, Serialize)]
struct VerifyJson {
    outcome: String,
    receipt_id: String,
    attempt: Option<i64>,
    attribution: Option<String>,
    files: Vec<VerifyFileJson>,
    body: Option<VerifyBodyJson>,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct VerifyFileJson {
    canvas_file_id: String,
    name: Option<String>,
    expected_sha256: Option<String>,
    actual_sha256: Option<String>,
    status: String,
}

#[derive(Debug, Serialize)]
struct VerifyBodyJson {
    expected_sha256: Option<String>,
    actual_sha256: Option<String>,
    status: String,
}

/// Run `canvas submission` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, cmd: SubmissionCmd) -> ExitCode {
    handle(globals, cmd).await.emit(globals.json)
}

/// Dispatch submission commands.
pub async fn handle(globals: &Globals, cmd: SubmissionCmd) -> Handled {
    match cmd {
        SubmissionCmd::Show {
            course,
            assignment,
            history,
        } => show_cmd(globals, &course, assignment.as_deref(), history).await,
        SubmissionCmd::Verify { receipt_id } => verify_cmd(globals, &receipt_id).await,
        SubmissionCmd::Reconcile {
            journal_id,
            assume_not_submitted,
        } => reconcile_cmd(globals, &journal_id, assume_not_submitted).await,
    }
}

async fn reconcile_cmd(globals: &Globals, journal_id: &str, assume_not_submitted: bool) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let needs_network = canvas_core::journal::get_journal(&session.open.store, journal_id)
        .ok()
        .flatten()
        .is_some_and(|row| {
            matches!(
                row.state,
                canvas_core::journal::State::OutcomeUnknown | canvas_core::journal::State::Posting
            ) || matches!(
                row.state,
                canvas_core::journal::State::Submitted | canvas_core::journal::State::Matched
            ) && row.readback_record_json.is_none()
        });
    if needs_network
        && canvas_core::journal::probe_owner(&session.paths.identity_dir, journal_id)
            .is_ok_and(|s| s != canvas_core::journal::OwnerStatus::Live)
        && let Err(e) = session.validate_network_token().await
    {
        return super::emit::sync_error(&session, &e);
    }
    let result = match reconcile(
        client,
        &session.open.store,
        &session.paths,
        journal_id,
        assume_not_submitted,
        crate::output::now_timestamp(),
    )
    .await
    {
        Ok(r) => r,
        Err(ReconcileError::Network(e)) => {
            let error: canvas_core::sync::SyncError = e.into();
            let (code, exit, status) = error.classification();
            return reconcile_abort(
                &session,
                journal_id,
                code,
                &error.safe_message(),
                exit,
                status,
            );
        }
        Err(ReconcileError::Journal(canvas_core::journal::JournalError::NotFound)) => {
            return emit_error(
                "refused",
                "journal not found",
                8,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => {
            return reconcile_abort(&session, journal_id, "local", &e.to_string(), 13, None);
        }
    };
    let (outcome, exit) = match result.outcome {
        ReconcileOutcome::Ok => (Outcome::Ok, 0),
        ReconcileOutcome::Recovery => (Outcome::Recovery, 9),
        ReconcileOutcome::Refused => (Outcome::Refused, 8),
    };
    let payload = ReconcileJson {
        outcome: match result.outcome {
            ReconcileOutcome::Ok => "ok",
            ReconcileOutcome::Recovery => "recovery",
            ReconcileOutcome::Refused => "refused",
        }
        .into(),
        state: result.state.as_str().into(),
        journal_id: result.journal_id,
        owner: result.owner.as_str().into(),
        response_kind: result.response_kind.map(|k| k.as_str().into()),
        not_submitted_evidence: result.not_submitted_evidence,
        assume_available: result.assume_available,
        attribution: result.attribution,
        receipt_id: result.receipt_id,
        posted: result
            .posted
            .as_ref()
            .and_then(|p| serde_json::to_value(p).ok()),
        server_match: result
            .server_match
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok()),
        candidates: result
            .candidates
            .iter()
            .filter_map(|c| serde_json::to_value(c).ok())
            .collect(),
        message: result.message,
    };
    let mut env = base_envelope(SCHEMA_RECONCILE, &session, payload);
    env.exit = exit;
    env.outcome = outcome;
    env.requests = session.requests();
    Handled::new(env, move |env| {
        writeln!(
            io::stdout(),
            "reconcile {} — {}",
            env.result.outcome,
            env.result.message
        )?;
        writeln!(
            io::stdout(),
            "journal {}  state={}  owner={}  attribution={}",
            env.result.journal_id,
            env.result.state,
            env.result.owner,
            env.result.attribution.as_deref().unwrap_or("unknown")
        )?;
        if let Some(posted) = &env.result.posted {
            writeln!(
                io::stdout(),
                "attempt {}  submitted_at {}",
                posted["attempt"],
                posted["submitted_at_local"].as_str().unwrap_or("unknown")
            )?;
        }
        for candidate in &env.result.candidates {
            writeln!(
                io::stdout(),
                "candidate attempt {}  submitted_at {}",
                candidate["attempt"],
                candidate["submitted_at_local"]
                    .as_str()
                    .unwrap_or("unknown")
            )?;
        }
        Ok(())
    })
}

fn reconcile_abort(
    session: &crate::session::Session,
    jid: &str,
    code: &str,
    message: &str,
    exit: u8,
    status: Option<u16>,
) -> Handled {
    let row = canvas_core::journal::get_journal(&session.open.store, jid)
        .ok()
        .flatten();
    let details = serde_json::json!({"journal_id":jid,"state":row.as_ref().map(|r|r.state.as_str()),"posted":row.as_ref().and_then(|r|r.response_record_json.as_deref()).and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok())});
    let mut env = crate::output::error_envelope(code, message, status, details, exit);
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    Handled::new(env, move |env| {
        writeln!(io::stderr(), "{}", env.result.message)
    })
}

async fn verify_cmd(globals: &Globals, receipt_id: &str) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let receipt = match load_receipt_for_verify(&session.open.store, &session.paths, receipt_id) {
        Ok(r) => r,
        Err(e) => {
            return emit_error(
                "refused",
                &e.to_string(),
                8,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let result = if let Some(refused) =
        validate_receipt(&session.open.store, session.identity.key.as_str(), &receipt)
    {
        refused
    } else {
        let client = match require_client(globals, &session) {
            Ok(c) => c,
            Err(code) => return code,
        };
        if let Err(e) = session.validate_network_token().await {
            return super::emit::sync_error(&session, &e);
        }
        match verify(
            client,
            &session.open.store,
            &session.paths,
            session.identity.key.as_str(),
            &receipt,
        )
        .await
        {
            Ok(r) => r,
            Err(VerifyError::Network(e)) => {
                return super::emit::sync_error(&session, &e.into());
            }
            Err(e) => {
                return emit_error(
                    "verify",
                    &e.to_string(),
                    13,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        }
    };
    let (outcome, exit) = match result.outcome {
        VerifyOutcome::Verified | VerifyOutcome::VerifiedBody => (Outcome::Ok, 0),
        VerifyOutcome::Mismatch => (Outcome::Mismatch, 10),
        VerifyOutcome::Unavailable => (Outcome::Partial, 12),
        VerifyOutcome::Refused => (Outcome::Refused, 8),
    };
    let payload = VerifyJson {
        outcome: match result.outcome {
            VerifyOutcome::Verified => "verified",
            VerifyOutcome::VerifiedBody => "verified_body",
            VerifyOutcome::Mismatch => "mismatch",
            VerifyOutcome::Unavailable => "unavailable",
            VerifyOutcome::Refused => "refused",
        }
        .into(),
        receipt_id: result.receipt_id,
        attempt: result.attempt,
        attribution: result.attribution,
        files: result
            .files
            .into_iter()
            .map(|f| VerifyFileJson {
                canvas_file_id: f.canvas_file_id,
                name: f.name,
                expected_sha256: f.expected_sha256,
                actual_sha256: f.actual_sha256,
                status: f.status,
            })
            .collect(),
        body: result.body.map(|b| VerifyBodyJson {
            expected_sha256: b.expected_sha256,
            actual_sha256: b.actual_sha256,
            status: b.status,
        }),
        reason: result.reason,
    };
    let mut env = base_envelope(SCHEMA_VERIFY, &session, payload);
    env.exit = exit;
    env.outcome = outcome;
    env.requests = session.requests();
    Handled::new(env, move |env| {
        writeln!(
            io::stdout(),
            "verify {}  receipt={}  attempt={}  attribution={}",
            env.result.outcome,
            env.result.receipt_id,
            env.result
                .attempt
                .map_or_else(|| "unknown".into(), |a| a.to_string()),
            env.result.attribution.as_deref().unwrap_or("unknown")
        )?;
        if let Some(reason) = &env.result.reason {
            writeln!(io::stdout(), "{reason}")?;
        }
        let mut table = crate::output::new_table();
        table.set_header([
            "Canvas ID",
            "File",
            "Status",
            "Expected SHA-256",
            "Actual SHA-256",
        ]);
        for file in &env.result.files {
            table.add_row(vec![
                file.canvas_file_id.clone(),
                file.name.clone().unwrap_or_default(),
                file.status.clone(),
                file.expected_sha256.clone().unwrap_or_default(),
                file.actual_sha256.clone().unwrap_or_default(),
            ]);
        }
        crate::output::apply_two_space_padding(&mut table);
        if !env.result.files.is_empty() {
            writeln!(io::stdout(), "{table}")?;
        }
        if let Some(body) = &env.result.body {
            writeln!(
                io::stdout(),
                "body {}  expected={}  actual={}",
                body.status,
                body.expected_sha256.as_deref().unwrap_or("unknown"),
                body.actual_sha256.as_deref().unwrap_or("unknown")
            )?;
        }
        Ok(())
    })
}

/// `submission <course> <assignment> [--history]` over the M1-c `submission` dataset.
async fn show_cmd(
    globals: &Globals,
    course: &str,
    assignment: Option<&str>,
    history: bool,
) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let mut freshness = Vec::new();
    let mut outcomes = Vec::new();
    let (course, assignment_id) = match super::assignment_read::resolve_target(
        &session,
        globals,
        course,
        assignment,
        &mut freshness,
        &mut outcomes,
    )
    .await
    {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match super::assignment_read::submission(&session, globals, course.id, assignment_id).await {
        Ok(o) => outcomes.push(o),
        Err(e) => return super::emit::sync_error(&session, &e),
    }

    let cached = match session
        .open
        .store
        .call(move |conns| {
            let pending =
                canvas_core::store::pending_journals_for_assignment(&conns.state, assignment_id)?;
            Ok((load_submission_row(conns, assignment_id)?, pending))
        })
        .await
    {
        Ok(pair) => pair,
        Err(e) => return super::emit::sync_error(&session, &e.into()),
    };
    let (row, pending_journals) = cached;
    let Some(row) = row else {
        return emit_error(
            "offline",
            "no cached submission for this assignment",
            7,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };

    let zone = session.time_zone();
    let result = submission_json(&row, &pending_journals, history, &zone);
    let mut envelope = base_envelope(SCHEMA_SUBMISSION, &session, result);
    freshness.extend(outcomes.iter().map(super::course_load::outcome_freshness));
    envelope.freshness = freshness;
    envelope
        .warnings
        .extend(outcomes.into_iter().filter_map(|o| o.error));
    envelope.requests = session.requests();
    Handled::new(envelope, move |envelope| {
        render_submission(io::stdout(), &envelope.result, &course, &zone)
    })
}

/// One cached `submission` row: the typed columns plus the JSON side-car.
struct SubmissionRow {
    attempt: Option<i64>,
    score: Option<f64>,
    grade: Option<String>,
    submitted_at: Option<String>,
    workflow_state: Option<String>,
    late: Option<bool>,
    missing: Option<bool>,
    excused: Option<bool>,
    data: serde_json::Value,
}

/// Read the submission the `submission:assignment:<id>` dataset published.
fn load_submission_row(
    conns: &canvas_core::store::StoreConns,
    assignment_id: i64,
) -> Result<Option<SubmissionRow>, canvas_core::store::DbError> {
    use rusqlite::OptionalExtension;
    let row = conns
        .cache
        .query_row(
            "SELECT s.attempt, s.score, s.grade, s.submitted_at, s.workflow_state, s.late, \
             s.missing, s.excused, s.data_json FROM membership m \
             JOIN submissions s ON s.id = CAST(m.entity_id AS INTEGER) \
             WHERE m.dataset = 'submission' AND m.scope = ?1",
            [format!("assignment:{assignment_id}")],
            |r| {
                let raw: String = r.get(8)?;
                Ok(SubmissionRow {
                    attempt: r.get(0)?,
                    score: r.get(1)?,
                    grade: r.get(2)?,
                    submitted_at: r.get(3)?,
                    workflow_state: r.get(4)?,
                    late: r.get::<_, Option<i64>>(5)?.map(|v| v != 0),
                    missing: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
                    excused: r.get::<_, Option<i64>>(7)?.map(|v| v != 0),
                    data: serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null),
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// `Attachment` objects, passed through from the dataset's stored projection.
fn attachments_of(value: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    value
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "id": a["id"],
                "display_name": a["display_name"],
                "size": a["size"],
                "content_type": a["content_type"],
            })
        })
        .collect()
}

/// Build the `submission@1` result (Appendix D).
///
/// While a journal is pending the §10 hook makes the status unknown; the
/// server-observed payload around it is still reported as Canvas shows it.
fn submission_json(
    row: &SubmissionRow,
    pending_journals: &[String],
    history: bool,
    zone: &jiff::tz::TimeZone,
) -> serde_json::Value {
    let pending = !pending_journals.is_empty();
    let submitted_at = row.submitted_at.as_ref().and_then(|s| s.parse().ok());
    let submitted = row.workflow_state.as_deref().map_or_else(
        || submitted_at.map(|_: Timestamp| true),
        |w| Some(w != "unsubmitted"),
    );
    let mut status = serde_json::json!({
        "submitted": submitted,
        "graded": row.workflow_state.as_deref().map(|w| w == "graded"),
        "score": row.score,
        "grade": row.grade,
        "late": row.late,
        "missing": row.missing.unwrap_or(false),
        "excused": row.excused,
        "workflow_state": row.workflow_state,
        "submitted_at": row.submitted_at,
        "submitted_at_local": super::assignment_read::local(submitted_at, zone),
        "attempt": row.attempt,
        "posted_at": row.data["posted_at"],
        "pending": pending,
    });
    if pending {
        for field in [
            "submitted",
            "graded",
            "score",
            "grade",
            "late",
            "excused",
            "workflow_state",
            "submitted_at",
            "submitted_at_local",
            "attempt",
            "posted_at",
        ] {
            status[field] = serde_json::Value::Null;
        }
    }
    let object = status.as_object_mut().expect("status object");
    object.insert(
        "submission_type".into(),
        row.data["submission_type"].clone(),
    );
    object.insert("body_sha256".into(), row.data["body_sha256"].clone());
    object.insert("url".into(), row.data["url"].clone());
    object.insert(
        "attachments".into(),
        attachments_of(row.data.get("attachments_json")).into(),
    );
    object.insert("comments".into(), comments_json(&row.data, zone).into());
    object.insert(
        "rubric_assessed".into(),
        row.data["rubric_assessment_json"].is_array().into(),
    );
    // A row cached before M8-a has no `rating_id`; re-projecting it keeps the
    // shape the schema declares whatever wrote the row (§7).
    object.insert(
        "rubric_assessment".into(),
        row.data
            .get("rubric_assessment_json")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .map(canvas_core::sync::assessment_row_json)
            .collect(),
    );

    serde_json::json!({
        "submission": status,
        "history": if history { history_json(&row.data, zone) } else { Vec::new() },
        "pending_journals": pending_journals,
    })
}

/// `comments[]`, newest field order per Appendix D.
fn comments_json(data: &serde_json::Value, zone: &jiff::tz::TimeZone) -> Vec<serde_json::Value> {
    data.get("submission_comments_json")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let at = c["created_at"].as_str().and_then(|s| s.parse().ok());
            serde_json::json!({
                "id": c["id"],
                "author": c["author_name"],
                "created_at": c["created_at"],
                "created_at_local": super::assignment_read::local(at, zone),
                "text": c["comment"],
            })
        })
        .collect()
}

/// `history[]`, sorted by `attempt` ascending.
fn history_json(data: &serde_json::Value, zone: &jiff::tz::TimeZone) -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = data
        .get("submission_history_json")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|h| {
            let at = h["submitted_at"].as_str().and_then(|s| s.parse().ok());
            serde_json::json!({
                "attempt": h["attempt"],
                "submitted_at": h["submitted_at"],
                "submitted_at_local": super::assignment_read::local(at, zone),
                "score": h["score"],
                "attachments": attachments_of(h.get("attachments")),
            })
        })
        .collect();
    rows.sort_by_key(|r| r["attempt"].as_i64().unwrap_or(i64::MAX));
    rows
}

/// Human `submission` output: status, attachments, comments, and history.
fn render_submission(
    mut out: impl Write,
    result: &serde_json::Value,
    course: &canvas_core::resolve::ResolvedCourse,
    zone: &jiff::tz::TimeZone,
) -> io::Result<()> {
    let s = &result["submission"];
    // §7 human dates: the identity zone, `Tue Sep 15, 11:59 PM`. These are
    // times things happened at, so they carry no `overdue`/`in` suffix.
    let when = |v: &serde_json::Value| {
        v.as_str()
            .and_then(|raw| raw.parse::<Timestamp>().ok())
            .map(|ts| crate::output::format_local_instant(ts, zone))
    };
    // Table cells use the em dash the other list renderers use; the status
    // line spells out `unknown`, where absence is the point.
    let cell = |v: &serde_json::Value| {
        v.as_str().map_or_else(
            || {
                if v.is_null() {
                    "\u{2014}".into()
                } else {
                    v.to_string()
                }
            },
            ToOwned::to_owned,
        )
    };
    let unknown = |v: &serde_json::Value| {
        v.as_str().map_or_else(
            || {
                if v.is_null() {
                    "unknown".into()
                } else {
                    v.to_string()
                }
            },
            ToOwned::to_owned,
        )
    };
    writeln!(
        out,
        "course {}{}",
        course.id,
        course
            .code
            .as_deref()
            .map_or_else(String::new, |c| format!("  {c}"))
    )?;
    writeln!(
        out,
        "state {}  attempt {}  submitted {}  score {}",
        unknown(&s["workflow_state"]),
        unknown(&s["attempt"]),
        when(&s["submitted_at"]).unwrap_or_else(|| "unknown".into()),
        unknown(&s["score"])
    )?;
    if s["pending"] == serde_json::Value::Bool(true) {
        writeln!(
            out,
            "pending: {} unresolved journal(s); status unknown until reconciled",
            result["pending_journals"].as_array().map_or(0, Vec::len)
        )?;
        for journal in result["pending_journals"].as_array().unwrap_or(&Vec::new()) {
            writeln!(out, "  journal {}", cell(journal))?;
        }
    }
    let attachments = s["attachments"].as_array().cloned().unwrap_or_default();
    if !attachments.is_empty() {
        let mut table = crate::output::new_table();
        table.set_header(["Canvas ID", "File", "Size", "Type"]);
        for a in &attachments {
            table.add_row(vec![
                cell(&a["id"]),
                cell(&a["display_name"]),
                cell(&a["size"]),
                cell(&a["content_type"]),
            ]);
        }
        crate::output::apply_two_space_padding(&mut table);
        writeln!(out, "{table}")?;
    }
    for c in s["comments"].as_array().unwrap_or(&Vec::new()) {
        writeln!(
            out,
            "comment {} {}: {}",
            when(&c["created_at"]).unwrap_or_else(|| "\u{2014}".into()),
            cell(&c["author"]),
            cell(&c["text"])
        )?;
    }
    let history = result["history"].as_array().cloned().unwrap_or_default();
    if !history.is_empty() {
        let mut table = crate::output::new_table();
        table.set_header(["Attempt", "Submitted", "Score", "Files"]);
        for h in &history {
            table.add_row(vec![
                cell(&h["attempt"]),
                when(&h["submitted_at"]).unwrap_or_else(|| "\u{2014}".into()),
                cell(&h["score"]),
                h["attachments"].as_array().map_or(0, Vec::len).to_string(),
            ]);
        }
        crate::output::apply_two_space_padding(&mut table);
        writeln!(out, "{table}")?;
    }
    Ok(())
}

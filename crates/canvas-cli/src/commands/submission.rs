//! `canvas submission` (verify / reconcile / show stub).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::submit::{
    ReconcileError, ReconcileOutcome, VerifyError, VerifyOutcome, load_receipt_for_verify,
    reconcile, validate_receipt, verify,
};
use jiff::Timestamp;
use serde::Serialize;

use super::Globals;
use super::emit::{
    base_envelope, emit, emit_error, parse_numeric_id, require_client, session_error,
};
use crate::output::{Outcome, SCHEMA_RECONCILE, SCHEMA_VERIFY};

/// Submission subcommand operands after clap parsing.
pub enum SubmissionCmd {
    Show {
        course: String,
        assignment: String,
        #[allow(dead_code)]
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

/// Dispatch submission commands.
pub async fn run(globals: &Globals, cmd: SubmissionCmd) -> ExitCode {
    match cmd {
        SubmissionCmd::Show {
            course,
            assignment,
            history: _,
        } => {
            if parse_numeric_id(&course, "course").is_err()
                || parse_numeric_id(&assignment, "assignment").is_err()
            {
                return emit_error(
                    globals.json,
                    "usage",
                    "course and assignment resolution needs M1-c; use numeric ids for now",
                    2,
                    globals.profile.clone(),
                    None,
                );
            }
            emit_error(
                globals.json,
                "not_implemented",
                "submission show waiting for M1-c",
                1,
                globals.profile.clone(),
                None,
            )
        }
        SubmissionCmd::Verify { receipt_id } => verify_cmd(globals, &receipt_id).await,
        SubmissionCmd::Reconcile {
            journal_id,
            assume_not_submitted,
        } => reconcile_cmd(globals, &journal_id, assume_not_submitted).await,
    }
}

async fn reconcile_cmd(
    globals: &Globals,
    journal_id: &str,
    assume_not_submitted: bool,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
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
        return super::emit::sync_error(globals, &session, &e);
    }
    let result = match reconcile(
        client,
        &session.open.store,
        &session.paths,
        journal_id,
        assume_not_submitted,
        Timestamp::now(),
    )
    .await
    {
        Ok(r) => r,
        Err(ReconcileError::Network(e)) => {
            let error: canvas_core::sync::SyncError = e.into();
            let (code, exit, status) = error.classification();
            return reconcile_abort(
                globals,
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
                globals.json,
                "refused",
                "journal not found",
                8,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => {
            return reconcile_abort(
                globals,
                &session,
                journal_id,
                "local",
                &e.to_string(),
                13,
                None,
            );
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
    emit(globals.json, &env, || {
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
    globals: &Globals,
    session: &crate::session::Session,
    jid: &str,
    code: &str,
    message: &str,
    exit: u8,
    status: Option<u16>,
) -> ExitCode {
    let row = canvas_core::journal::get_journal(&session.open.store, jid)
        .ok()
        .flatten();
    let details = serde_json::json!({"journal_id":jid,"state":row.as_ref().map(|r|r.state.as_str()),"posted":row.as_ref().and_then(|r|r.response_record_json.as_deref()).and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok())});
    let mut env = crate::output::error_envelope(code, message, status, details, exit);
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    emit(globals.json, &env, || {
        writeln!(io::stderr(), "{}", env.result.message)
    })
}

async fn verify_cmd(globals: &Globals, receipt_id: &str) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let receipt = match load_receipt_for_verify(&session.open.store, &session.paths, receipt_id) {
        Ok(r) => r,
        Err(e) => {
            return emit_error(
                globals.json,
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
            return super::emit::sync_error(globals, &session, &e);
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
                return super::emit::sync_error(globals, &session, &e.into());
            }
            Err(e) => {
                return emit_error(
                    globals.json,
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
    emit(globals.json, &env, || {
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

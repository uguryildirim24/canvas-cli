//! `canvas submission` (verify / reconcile / show stub).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::submit::{
    ReconcileOutcome, VerifyOutcome, load_receipt_for_verify, reconcile, verify,
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
        Err(e) => {
            return emit_error(
                globals.json,
                "reconcile",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
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
        Ok(())
    })
}

async fn verify_cmd(globals: &Globals, receipt_id: &str) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
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
    let result = match verify(
        client,
        &session.open.store,
        &session.paths,
        session.identity.key.as_str(),
        &receipt,
    )
    .await
    {
        Ok(r) => r,
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
        writeln!(io::stdout(), "verify {}", env.result.outcome)?;
        Ok(())
    })
}

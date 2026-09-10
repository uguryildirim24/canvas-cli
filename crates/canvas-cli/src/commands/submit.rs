//! `canvas submit` (class D mutation).

use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::journal::{State, get_journal};
use canvas_core::submit::{
    ExecuteError, ExecuteOutcome, FreezeError, FrozenInput, InputKind, Plan, PreflightError,
    SubmitError, TextSource, create_from_plan, execute, freeze_files, freeze_html, freeze_text,
    freeze_url, preflight,
};
use jiff::Timestamp;

use super::Globals;
use super::emit::{
    base_envelope, emit, emit_error, parse_numeric_id, require_client, session_error,
    validate_token_error,
};
use crate::output::{
    Outcome, SCHEMA_SUBMIT, SubmitCandidateJson, SubmitFileJson, SubmitResult, SubmitTextJson,
};
use crate::session::Session;

/// Run `canvas submit`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run(
    globals: &Globals,
    target: String,
    assignment: Option<String>,
    files: Vec<PathBuf>,
    text: Option<String>,
    html: Option<PathBuf>,
    url: Option<String>,
    comment: Option<String>,
    yes: bool,
) -> ExitCode {
    let (course_id, assignment_id) = match resolve_numeric_pair(&target, assignment.as_deref()) {
        Ok(ids) => ids,
        Err(message) => {
            return emit_error(
                globals.json,
                "usage",
                &message,
                2,
                globals.profile.clone(),
                None,
            );
        }
    };

    if globals.offline {
        return emit_error(
            globals.json,
            "usage",
            "this command cannot run with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return validate_token_error(globals.json, e, &session);
    }

    let frozen = match freeze_inputs(
        &files,
        text.as_deref(),
        html.as_deref(),
        url.as_deref(),
        comment.as_deref(),
    ) {
        Ok(f) => f,
        Err(e) => {
            return emit_error(
                globals.json,
                "usage",
                &e.to_string(),
                2,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    let now = Timestamp::now();
    let outcome = match preflight(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        session.identity.key.as_str(),
        course_id,
        assignment_id,
        frozen,
        now,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return map_preflight_error(globals, &session, e),
    };

    for (jid, state) in &outcome.plan.recovered {
        let _ = writeln!(io::stderr(), "recovered journal {jid} → {state}");
    }

    if !yes {
        print_plan(&outcome.plan);
        match confirm_tty() {
            Ok(true) => {}
            Ok(false) => {
                drop(outcome.admission);
                return emit_error(
                    globals.json,
                    "cancelled",
                    "submission cancelled",
                    11,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
            Err(message) => {
                drop(outcome.admission);
                return emit_error(
                    globals.json,
                    "usage",
                    &message,
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        }
    }

    let (journal_id, owner) = match create_from_plan(
        &session.open.store,
        &session.paths.identity_dir,
        session.identity.key.as_str(),
        &outcome.admission,
        &outcome.plan,
    ) {
        Ok(pair) => pair,
        Err(e) => {
            drop(outcome.admission);
            return map_preflight_error(globals, &session, e);
        }
    };
    drop(outcome.admission);

    let frozen = outcome.plan.frozen.clone();
    match execute(
        client,
        &session.open.store,
        &session.paths,
        &owner,
        &journal_id,
        &frozen,
    )
    .await
    {
        Ok(exec) => emit_submit_ok(globals, &session, &exec, &frozen),
        Err(e) => map_execute_error(globals, &session, &journal_id, e),
    }
}

fn resolve_numeric_pair(target: &str, assignment: Option<&str>) -> Result<(i64, i64), String> {
    let Some(assignment) = assignment else {
        return Err(
            "course and assignment resolution needs M1-c; pass two numeric ids for now".into(),
        );
    };
    let course_id = parse_numeric_id(target, "course")?;
    let assignment_id = parse_numeric_id(assignment, "assignment")?;
    Ok((course_id, assignment_id))
}

fn freeze_inputs(
    files: &[PathBuf],
    text: Option<&str>,
    html: Option<&std::path::Path>,
    url: Option<&str>,
    comment: Option<&str>,
) -> Result<FrozenInput, FreezeError> {
    if !files.is_empty() {
        return freeze_files(files, comment);
    }
    if let Some(path) = html {
        return freeze_html(path, comment);
    }
    if let Some(url) = url {
        return freeze_url(url, comment);
    }
    if let Some(text) = text {
        if text == "-" {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            return freeze_text(&TextSource::Bytes(&buf), comment);
        }
        return freeze_text(&TextSource::Path(std::path::Path::new(text)), comment);
    }
    Err(FreezeError::Validation(
        "one of --file, --text, --html, or --url is required".into(),
    ))
}

fn print_plan(plan: &Plan) {
    let _ = writeln!(io::stderr(), "Submit plan");
    let _ = writeln!(
        io::stderr(),
        "  course {}  assignment {}",
        plan.course_id,
        plan.assignment_id
    );
    if let Some(name) = &plan.assignment_name {
        let _ = writeln!(io::stderr(), "  name {name}");
    }
    if let Some(code) = &plan.course_code {
        let _ = writeln!(io::stderr(), "  course_code {code}");
    }
    if let Some(due) = &plan.due_at {
        let _ = writeln!(io::stderr(), "  due_at {due}");
    }
    if plan.past_due {
        let _ = writeln!(io::stderr(), "  warning: past due");
    }
    let _ = writeln!(
        io::stderr(),
        "  kind {}  estimated_attempt {}",
        plan.kind.as_str(),
        plan.estimated_attempt
    );
    match plan.kind {
        InputKind::OnlineUpload => {
            for f in &plan.frozen.payload.files {
                let _ = writeln!(
                    io::stderr(),
                    "  file {} ({} bytes, sha256 {})",
                    f.name,
                    f.size,
                    f.sha256
                );
            }
        }
        InputKind::OnlineTextEntry | InputKind::OnlineHtml => {
            if let Some(t) = &plan.frozen.payload.text {
                let _ = writeln!(
                    io::stderr(),
                    "  text transform={} input_sha256={} sent_sha256={}",
                    t.transform,
                    t.input_sha256,
                    t.sent_sha256
                );
            }
        }
        InputKind::OnlineUrl => {
            if let Some(u) = &plan.frozen.payload.url {
                let _ = writeln!(io::stderr(), "  url {u}");
            }
        }
    }
    if let Some(c) = &plan.frozen.payload.comment {
        let _ = writeln!(io::stderr(), "  comment ({} chars)", c.chars().count());
    }
    let _ = write!(io::stderr(), "Submit? [y/N] ");
    let _ = io::stderr().flush();
}

fn confirm_tty() -> Result<bool, String> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| {
            "confirmation required but no controlling terminal; re-run with --yes".to_string()
        })?;
    let mut reader = io::BufReader::new(tty);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("failed to read confirmation: {e}"))?;
    let answer = line.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}

fn emit_submit_ok(
    globals: &Globals,
    session: &Session,
    exec: &ExecuteOutcome,
    frozen: &FrozenInput,
) -> ExitCode {
    let (outcome, exit) = match exec.state {
        State::Submitted | State::Matched => (Outcome::Ok, 0),
        State::Refused => (Outcome::Refused, 8),
        State::UploadIncomplete | State::UploadedNotSubmitted | State::OutcomeUnknown => {
            (Outcome::Recovery, 9)
        }
        _ => (Outcome::Recovery, 9),
    };

    let mut result = build_submit_result(exec, frozen);
    if let Some(rec) = &exec.reconcile {
        result.server_match = rec.server_match.as_ref().map(|c| SubmitCandidateJson {
            attempt: c.attempt,
            submitted_at: c.submitted_at.clone(),
            submitted_at_local: c.submitted_at_local.clone(),
            attachment_ids: c.attachment_ids.clone(),
        });
        result.candidates = rec
            .candidates
            .iter()
            .map(|c| SubmitCandidateJson {
                attempt: c.attempt,
                submitted_at: c.submitted_at.clone(),
                submitted_at_local: c.submitted_at_local.clone(),
                attachment_ids: c.attachment_ids.clone(),
            })
            .collect();
        if result.attribution.is_none() {
            result.attribution.clone_from(&rec.attribution);
        }
        if result.receipt_id.is_none() {
            result.receipt_id.clone_from(&rec.receipt_id);
        }
    }
    result.outcome = match outcome {
        Outcome::Ok => "ok".into(),
        Outcome::Recovery => "recovery".into(),
        Outcome::Refused => "refused".into(),
        Outcome::Mismatch => "mismatch".into(),
        Outcome::Partial => "partial".into(),
        Outcome::Error => "error".into(),
    };

    let mut envelope = base_envelope(SCHEMA_SUBMIT, session, result);
    envelope.outcome = outcome;
    envelope.exit = exit;
    envelope.requests = session.requests();
    if let Some(w) = &exec.warning {
        envelope.warnings.push(w.clone());
    }
    emit(globals.json, &envelope, || {
        let r = &envelope.result;
        writeln!(
            io::stdout(),
            "submit {} journal={} receipt={:?} attribution={:?}",
            r.state,
            r.journal_id,
            r.receipt_id,
            r.attribution
        )
    })
}

fn build_submit_result(exec: &ExecuteOutcome, frozen: &FrozenInput) -> SubmitResult {
    let files = frozen
        .payload
        .files
        .iter()
        .map(|f| SubmitFileJson {
            name: f.name.clone(),
            size: f.size,
            sha256: f.sha256.clone(),
            canvas_file_id: f.canvas_file_id.clone(),
        })
        .collect();
    let text = frozen.payload.text.as_ref().map(|t| SubmitTextJson {
        input_sha256: t.input_sha256.clone(),
        transform: t.transform.clone(),
        sent_sha256: t.sent_sha256.clone(),
    });
    SubmitResult {
        outcome: String::new(),
        state: exec.state.as_str().to_owned(),
        journal_id: exec.journal_id.clone(),
        receipt_id: exec.receipt_id.clone(),
        attribution: exec.attribution.clone(),
        post_status: exec.post_status,
        response_kind: exec.response_kind.map(|k| k.as_str().to_owned()),
        posted: None,
        server_match: None,
        candidates: Vec::new(),
        files,
        text,
        url: frozen.payload.url.clone(),
        error: None,
    }
}

fn map_preflight_error(globals: &Globals, session: &Session, err: PreflightError) -> ExitCode {
    map_submit_error(globals, session, err.into())
}

#[allow(clippy::too_many_lines)]
fn map_execute_error(
    globals: &Globals,
    session: &Session,
    journal_id: &str,
    err: ExecuteError,
) -> ExitCode {
    match err {
        ExecuteError::Refused(message) => {
            let state = get_journal(&session.open.store, journal_id)
                .ok()
                .flatten()
                .map_or(State::Refused, |r| r.state);
            let exit = if matches!(state, State::UploadIncomplete) {
                9
            } else {
                8
            };
            let outcome = if exit == 9 {
                Outcome::Recovery
            } else {
                Outcome::Refused
            };
            let result = SubmitResult {
                outcome: if exit == 9 {
                    "recovery".into()
                } else {
                    "refused".into()
                },
                state: state.as_str().to_owned(),
                journal_id: journal_id.to_owned(),
                receipt_id: None,
                attribution: None,
                post_status: None,
                response_kind: None,
                posted: None,
                server_match: None,
                candidates: Vec::new(),
                files: Vec::new(),
                text: None,
                url: None,
                error: Some(message),
            };
            let mut envelope = base_envelope(SCHEMA_SUBMIT, session, result);
            envelope.outcome = outcome;
            envelope.exit = exit;
            envelope.requests = session.requests();
            emit(globals.json, &envelope, || {
                writeln!(
                    io::stderr(),
                    "{}",
                    envelope.result.error.as_deref().unwrap_or("")
                )
            })
        }
        ExecuteError::Network(e) => {
            let state = get_journal(&session.open.store, journal_id)
                .ok()
                .flatten()
                .map(|r| r.state);
            if matches!(
                state,
                Some(State::UploadIncomplete | State::OutcomeUnknown | State::UploadedNotSubmitted)
            ) {
                let result = SubmitResult {
                    outcome: "recovery".into(),
                    state: state.unwrap().as_str().to_owned(),
                    journal_id: journal_id.to_owned(),
                    receipt_id: None,
                    attribution: None,
                    post_status: None,
                    response_kind: None,
                    posted: None,
                    server_match: None,
                    candidates: Vec::new(),
                    files: Vec::new(),
                    text: None,
                    url: None,
                    error: Some(e.to_string()),
                };
                let mut envelope = base_envelope(SCHEMA_SUBMIT, session, result);
                envelope.outcome = Outcome::Recovery;
                envelope.exit = 9;
                envelope.requests = session.requests();
                return emit(globals.json, &envelope, || writeln!(io::stderr(), "{e}"));
            }
            emit_error(
                globals.json,
                "network",
                &e.to_string(),
                4,
                session.profile.clone(),
                Some(session.identity_ref()),
            )
        }
        ExecuteError::Journal(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ExecuteError::Io(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ExecuteError::Json(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

fn map_submit_error(globals: &Globals, session: &Session, err: SubmitError) -> ExitCode {
    match err {
        SubmitError::Validation(message) => emit_error(
            globals.json,
            "usage",
            &message,
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Network(e) => emit_error(
            globals.json,
            "network",
            &e.to_string(),
            4,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::InProgress { journal_id } => {
            let message = match journal_id {
                Some(id) => format!("in_progress journal {id}"),
                None => "in_progress".into(),
            };
            let mut env =
                crate::output::error_envelope("refused", message, None, serde_json::json!({}), 8);
            env.profile.clone_from(&session.profile);
            env.identity = Some(session.identity_ref());
            env.requests = session.requests();
            emit(globals.json, &env, || {
                writeln!(io::stderr(), "{}", env.result.message)
            })
        }
        SubmitError::Refused(message) => {
            let mut env =
                crate::output::error_envelope("refused", message, None, serde_json::json!({}), 8);
            env.profile.clone_from(&session.profile);
            env.identity = Some(session.identity_ref());
            env.requests = session.requests();
            emit(globals.json, &env, || {
                writeln!(io::stderr(), "{}", env.result.message)
            })
        }
        SubmitError::Recovery(message) => emit_error(
            globals.json,
            "recovery",
            &message,
            9,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Mismatch(message) => emit_error(
            globals.json,
            "mismatch",
            &message,
            10,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Unavailable(message) => emit_error(
            globals.json,
            "unavailable",
            &message,
            12,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::StateConflict => emit_error(
            globals.json,
            "local",
            "state conflict",
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Journal(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Io(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        SubmitError::Json(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

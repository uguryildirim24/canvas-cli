//! `canvas submit` (class D mutation).

use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::journal::{State, get_journal};
use canvas_core::submit::{
    ExecuteError, ExecuteOutcome, FreezeError, FrozenInput, InputKind, Plan, PreflightError,
    SubmitError, TextSource, create_from_plan, execute, freeze_files, freeze_html, freeze_text,
    freeze_url, preflight_with_input,
};
use jiff::Timestamp;

use super::Globals;
use super::emit::{base_envelope, emit_error, require_client, session_error, sync_error};
use super::handled::Handled;
use crate::output::{
    Outcome, SCHEMA_SUBMIT, SubmitCandidateJson, SubmitFileJson, SubmitResult, SubmitTextJson,
};
use crate::session::Session;

/// Operands and flags of one `canvas submit` invocation (§5).
#[derive(Debug, Clone, Default)]
pub struct SubmitArgs {
    pub target: String,
    pub assignment: Option<String>,
    pub files: Vec<PathBuf>,
    pub text: Option<String>,
    pub html: Option<PathBuf>,
    pub url: Option<String>,
    pub comment: Option<String>,
    pub yes: bool,
}

/// Run `canvas submit` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, args: SubmitArgs) -> ExitCode {
    handle(globals, args).await.emit(globals.json)
}

/// Prepare and dispatch one submission (§12.2).
#[allow(clippy::too_many_lines)]
pub async fn handle(globals: &Globals, args: SubmitArgs) -> Handled {
    let SubmitArgs {
        target,
        assignment,
        files,
        text,
        html,
        url,
        comment,
        yes,
    } = args;
    if globals.offline {
        return emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return sync_error(&session, &e);
    }

    // Pre-flight step 1: resolve the course and the assignment (SPEC §6).
    let mut freshness = Vec::new();
    let mut outcomes = Vec::new();
    let (course, assignment_id) = match super::assignment_read::resolve_target(
        &session,
        globals,
        &target,
        assignment.as_deref(),
        &mut freshness,
        &mut outcomes,
    )
    .await
    {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let course_id = course.id;
    freshness.extend(outcomes.iter().map(super::course_load::outcome_freshness));

    let kind = if !files.is_empty() {
        InputKind::OnlineUpload
    } else if html.is_some() {
        InputKind::OnlineHtml
    } else if url.is_some() {
        InputKind::OnlineUrl
    } else {
        InputKind::OnlineTextEntry
    };
    let mut outcome = match preflight_with_input(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        session.identity.key.as_str(),
        course_id,
        assignment_id,
        kind,
        move || {
            freeze_inputs(
                &files,
                text.as_deref(),
                html.as_deref(),
                url.as_deref(),
                comment.as_deref(),
            )
        },
        Timestamp::now(),
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return map_preflight_error(&session, e),
    };

    // The assignment GET omits the course include; fall back to the resolved code.
    if outcome.plan.course_code.is_none() {
        outcome.plan.course_code.clone_from(&course.code);
        outcome
            .plan
            .frozen
            .payload
            .course_code
            .clone_from(&course.code);
    }

    for (jid, state) in &outcome.plan.recovered {
        let _ = writeln!(io::stderr(), "recovered journal {jid} → {state}");
    }

    print_plan(&outcome.plan);
    if !yes {
        match confirm_tty().await {
            Ok(true) => {}
            Ok(false) => {
                drop(outcome.admission);
                return selected_error(&session, "cancelled", "submission cancelled", 11);
            }
            Err(message) => {
                drop(outcome.admission);
                return selected_error(&session, "usage", &message, 2);
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
            return map_preflight_error(&session, e);
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
        Ok(mut exec) => {
            if outcome.plan.past_due {
                exec.warning = Some(exec.warning.map_or_else(
                    || "assignment is past due".into(),
                    |w| format!("assignment is past due; {w}"),
                ));
            }
            emit_submit_ok(&session, &exec, &frozen, freshness)
        }
        Err(e) => map_execute_error(&session, &journal_id, e, freshness),
    }
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
            io::stdin().take(1_048_577).read_to_end(&mut buf)?;
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
}

async fn confirm_tty() -> Result<bool, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = send.send(read_confirmation());
    });
    tokio::select! {
        result = receive => result.map_err(|_| "confirmation worker stopped".to_owned())?,
        signal = tokio::signal::ctrl_c() => { signal.map_err(|e| e.to_string())?; Ok(false) }
    }
}

fn read_confirmation() -> Result<bool, String> {
    #[cfg(windows)]
    let terminal = "CONIN$";
    #[cfg(not(windows))]
    let terminal = "/dev/tty";
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(terminal)
        .map_err(|_| {
            "confirmation required but no controlling terminal; re-run with --yes".to_string()
        })?;
    let _ = write!(io::stderr(), "Submit? [y/N] ");
    let _ = io::stderr().flush();
    let mut reader = io::BufReader::new(tty);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("failed to read confirmation: {e}"))?;
    let answer = line.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}

fn emit_submit_ok(
    session: &Session,
    exec: &ExecuteOutcome,
    frozen: &FrozenInput,
    freshness: Vec<crate::output::Freshness>,
) -> Handled {
    let (outcome, exit) = match exec.state {
        State::Submitted | State::Matched => (Outcome::Ok, 0),
        State::Refused => (Outcome::Refused, 8),
        State::UploadIncomplete | State::UploadedNotSubmitted | State::OutcomeUnknown => {
            (Outcome::Recovery, 9)
        }
        _ => (Outcome::Recovery, 9),
    };

    let mut result = build_submit_result(exec, frozen);
    if let Err(e) = hydrate_submit_result(&session.open.store, &mut result) {
        return emit_error(
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
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
    envelope.freshness = freshness;
    envelope.outcome = outcome;
    envelope.exit = exit;
    envelope.requests = session.requests();
    if let Some(rec) = &exec.reconcile {
        envelope.warnings.push(rec.message.clone());
    } else if exec.state == State::OutcomeUnknown {
        envelope.warnings.push("the original request may still complete; submission reconcile re-checks; --assume-not-submitted becomes available after 30 minutes".into());
    }
    if let Some(w) = &exec.warning {
        envelope.warnings.push(w.clone());
    }
    Handled::new(envelope, move |envelope| {
        render_submit(io::stdout(), &envelope.result)
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

fn map_preflight_error(session: &Session, err: PreflightError) -> Handled {
    map_submit_error(session, err.into())
}

fn map_execute_error(
    session: &Session,
    journal_id: &str,
    err: ExecuteError,
    freshness: Vec<crate::output::Freshness>,
) -> Handled {
    let row = get_journal(&session.open.store, journal_id).ok().flatten();
    if let Some(row) = &row
        && matches!(
            row.state,
            State::Refused
                | State::UploadIncomplete
                | State::UploadedNotSubmitted
                | State::OutcomeUnknown
        )
        && !matches!(err, ExecuteError::Journal(_) | ExecuteError::Json(_))
    {
        let exec = ExecuteOutcome {
            state: row.state,
            journal_id: journal_id.into(),
            receipt_id: None,
            attribution: None,
            post_status: row.post_status,
            response_kind: row.response_kind.as_deref().and_then(|s| s.parse().ok()),
            reconcile: None,
            warning: None,
        };
        let frozen = FrozenInput {
            kind: InputKind::OnlineUpload,
            payload: canvas_core::journal::IntendedPayload::default(),
            file_paths: vec![],
        };
        return emit_submit_ok(session, &exec, &frozen, freshness);
    }
    let details = row.map_or_else(|| serde_json::json!({"journal_id":journal_id}), |row| serde_json::json!({
        "journal_id":journal_id,"state":row.state.as_str(),
        "posted":row.response_record_json.as_deref().and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok()),
        "files":serde_json::from_str::<serde_json::Value>(&row.intended_payload_json).ok().and_then(|v|v.get("files").cloned()).unwrap_or_else(||serde_json::json!([]))
    }));
    let mut env = crate::output::error_envelope("local", err.to_string(), None, details, 13);
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    Handled::new(env, move |env| {
        writeln!(io::stderr(), "{}", env.result.message)
    })
}

fn hydrate_submit_result(
    store: &canvas_core::store::Store,
    result: &mut SubmitResult,
) -> Result<(), canvas_core::submit::SubmitError> {
    let row = get_journal(store, &result.journal_id)?
        .ok_or(canvas_core::journal::JournalError::NotFound)?;
    let intent: canvas_core::journal::IntendedPayload =
        serde_json::from_str(&row.intended_payload_json)?;
    result.files = intent
        .files
        .into_iter()
        .map(|f| SubmitFileJson {
            name: f.name,
            size: f.size,
            sha256: f.sha256,
            canvas_file_id: f.canvas_file_id,
        })
        .collect();
    result.text = intent.text.map(|t| SubmitTextJson {
        input_sha256: t.input_sha256,
        transform: t.transform,
        sent_sha256: t.sent_sha256,
    });
    result.url = intent.url;
    result.error = row.error_text;
    result.posted = row
        .response_record_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    result.server_match = row
        .server_match_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(())
}

pub(super) fn render_submit(mut out: impl Write, result: &SubmitResult) -> io::Result<()> {
    writeln!(
        out,
        "submit {}  journal={}",
        result.state, result.journal_id
    )?;
    if let Some(id) = &result.receipt_id {
        writeln!(
            out,
            "receipt {id}  attribution={}",
            result.attribution.as_deref().unwrap_or("unknown")
        )?;
    }
    if let Some(posted) = &result.posted {
        writeln!(
            out,
            "attempt {}  submitted_at {}",
            posted["attempt"],
            posted["submitted_at_local"].as_str().unwrap_or("unknown")
        )?;
    }
    let mut table = crate::output::new_table();
    table.set_header(["File", "Bytes", "SHA-256", "Canvas ID"]);
    for file in &result.files {
        table.add_row(vec![
            file.name.clone(),
            file.size.to_string(),
            file.sha256.clone(),
            file.canvas_file_id
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        ]);
    }
    crate::output::apply_two_space_padding(&mut table);
    if !result.files.is_empty() {
        writeln!(out, "{table}")?;
    }
    if let Some(text) = &result.text {
        writeln!(
            out,
            "text transform={}  input_sha256={}  sent_sha256={}",
            text.transform, text.input_sha256, text.sent_sha256
        )?;
    }
    if let Some(url) = &result.url {
        writeln!(out, "url {url}")?;
    }
    for candidate in &result.candidates {
        writeln!(
            out,
            "candidate attempt {}  submitted_at {}",
            candidate.attempt,
            candidate.submitted_at_local.as_deref().unwrap_or("unknown")
        )?;
    }
    if let Some(error) = &result.error {
        writeln!(out, "{error}")?;
    }
    Ok(())
}

fn map_submit_error(session: &Session, err: SubmitError) -> Handled {
    let (code, exit, message) = match err {
        SubmitError::Validation(message) => ("usage", 2, message),
        SubmitError::Network(error) => return sync_error(session, &error.into()),
        SubmitError::InProgress { journal_id } => (
            "refused",
            8,
            journal_id.map_or_else(
                || "in_progress".into(),
                |id| format!("in_progress journal {id}"),
            ),
        ),
        SubmitError::Refused(message) => ("refused", 8, message),
        SubmitError::Recovery(message) => ("recovery", 9, message),
        SubmitError::Mismatch(message) => ("mismatch", 10, message),
        SubmitError::Unavailable(message) => ("unavailable", 12, message),
        SubmitError::StateConflict => ("local", 13, "state conflict".into()),
        SubmitError::Journal(e) => ("local", 13, e.to_string()),
        SubmitError::Io(e) => ("local", 13, e.to_string()),
        SubmitError::Json(e) => ("local", 13, e.to_string()),
    };
    selected_error(session, code, &message, exit)
}

fn selected_error(session: &Session, code: &str, message: &str, exit: u8) -> Handled {
    let mut env = crate::output::error_envelope(code, message, None, serde_json::json!({}), exit);
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    Handled::new(env, move |env| {
        writeln!(io::stderr(), "{}", env.result.message)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_submit_human_snapshot() {
        let result = serde_json::from_str(include_str!("../output/schemas/submit.json")).unwrap();
        let mut out = Vec::new();
        super::render_submit(&mut out, &result).unwrap();
        insta::assert_snapshot!("submit_files_human", String::from_utf8(out).unwrap());
    }
}

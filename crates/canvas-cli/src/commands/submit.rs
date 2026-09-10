//! `canvas submit` (class D mutation).

use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::journal::{State, get_journal};
use canvas_core::plan::{
    Admission, ApprovalChannel, PlanError, PlanState, PrepareRequest, Prepared,
};
use canvas_core::submit::{
    ExecuteError, ExecuteOutcome, FreezeError, FrozenInput, InputKind, Plan, PreflightError,
    SubmitError, TextSource, execute, freeze_files, freeze_html, freeze_text, freeze_url,
};

use super::Globals;
use super::emit::{base_envelope, emit_error, require_client, session_error, sync_error};
use super::handled::Handled;
use crate::output::{
    Freshness, Outcome, PlanJson, PlanResult, SCHEMA_PLAN, SCHEMA_SUBMIT, SubmitCandidateJson,
    SubmitFileJson, SubmitResult, SubmitTextJson,
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
pub async fn handle(globals: &Globals, args: SubmitArgs) -> Handled {
    let yes = args.yes;
    let frozen = match freeze_plan(globals, args, None).await {
        Ok(frozen) => frozen,
        Err(handled) => return handled,
    };
    let Frozen {
        session,
        prepared,
        freshness,
    } = *frozen;
    let plan_id = prepared.plan.plan_id.clone();
    print_plan(&prepared.display);

    let channel = if yes {
        // `--yes` is recorded as itself; it never claims an interactive decision.
        ApprovalChannel::YesFlag
    } else {
        match confirm_tty().await {
            Ok(true) => ApprovalChannel::Tty,
            Ok(false) => {
                cancel_plan(&session, &plan_id);
                return selected_error(&session, "cancelled", "submission cancelled", 11);
            }
            Err(message) => {
                cancel_plan(&session, &plan_id);
                return selected_error(&session, "usage", &message, 2);
            }
        }
    };
    if let Err(e) = record_approval(&session, &plan_id, channel, None) {
        return map_plan_error(&session, e);
    }

    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    // The human flow cannot reach a replay: it approves the plan it just
    // froze, so a plan that already has a journal is a refusal (§19 item 17).
    run_plan(&session, client, &plan_id, freshness, OnExisting::Refuse).await
}

/// A frozen plan, the session holding it, and the reads it cost.
struct Frozen {
    session: Session,
    prepared: Prepared,
    freshness: Vec<Freshness>,
}

/// What [`run_plan`] does with a plan that already admitted a journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnExisting {
    /// Refuse with exit 8: only `canvas submit` uses this.
    Refuse,
    /// Return the linked journal with `replayed: true` (SPEC §19 item 17).
    Replay,
}

/// Run pre-flight and freeze one plan, without approving anything.
///
/// The stored plan is `prepared` and no lock is held when this returns, so a
/// person can take as long as they need to decide (REPORT §3.5).
async fn freeze_plan(
    globals: &Globals,
    args: SubmitArgs,
    consumer: Option<&str>,
) -> Result<Box<Frozen>, Handled> {
    let SubmitArgs {
        target,
        assignment,
        files,
        text,
        html,
        url,
        comment,
        yes: _,
    } = args;
    if globals.offline {
        return Err(emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            globals.profile.clone(),
            None,
        ));
    }

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return Err(session_error(e, globals.profile.clone())),
    };
    // The plan borrows the session for its pre-flight only; the borrow ends
    // here so the session can travel with the plan.
    let mut freshness = Vec::new();
    let prepared = {
        let client = match require_client(globals, &session) {
            Ok(c) => c,
            Err(code) => return Err(code),
        };
        if let Err(e) = session.validate_network_token().await {
            return Err(sync_error(&session, &e));
        }

        // Pre-flight step 1: resolve the course and the assignment (SPEC §6).
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
            Err(code) => return Err(code),
        };
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
        // Both flows are the plan flow: freeze and store a plan, record the
        // decision as an approval, then execute the approved plan (§3.5).
        let request = PrepareRequest {
            identity_dir: &session.paths.identity_dir,
            identity_key: session.identity.key.as_str(),
            consumer,
            course_id: course.id,
            assignment_id,
            // The assignment GET omits the course include; fall back to the resolved code.
            course_code: course.code.as_deref(),
            kind,
        };
        match canvas_core::plan::prepare(
            client,
            &session.open.store,
            &request,
            move || {
                freeze_inputs(
                    &files,
                    text.as_deref(),
                    html.as_deref(),
                    url.as_deref(),
                    comment.as_deref(),
                )
            },
            crate::output::now_timestamp(),
        )
        .await
        {
            Ok(prepared) => prepared,
            Err(e) => return Err(map_plan_error(&session, e)),
        }
    };

    for (jid, state) in &prepared.display.recovered {
        let _ = writeln!(io::stderr(), "recovered journal {jid} → {state}");
    }
    Ok(Box::new(Frozen {
        session,
        prepared,
        freshness,
    }))
}

/// Issue an approval handle and spend it in the same step.
///
/// `canvas submit` holds the decision in this process, so the handle never
/// travels. An agent surface issues the handle separately, because there the
/// handle *is* the round trip.
fn record_approval(
    session: &Session,
    plan_id: &str,
    channel: ApprovalChannel,
    consumer: Option<&str>,
) -> Result<(), PlanError> {
    let handle = canvas_core::plan::issue_handle(&session.open.store, plan_id, consumer)?;
    canvas_core::plan::approve(
        &session.open.store,
        plan_id,
        &handle,
        channel,
        consumer,
        crate::output::now_timestamp(),
    )?;
    Ok(())
}

/// Admit an approved plan and dispatch it (SPEC §12.2 from step 7 on).
async fn run_plan(
    session: &Session,
    client: &canvas_api::Client,
    plan_id: &str,
    freshness: Vec<Freshness>,
    on_existing: OnExisting,
) -> Handled {
    let (journal_id, owner, frozen, past_due) = match canvas_core::plan::execute(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        session.identity.key.as_str(),
        plan_id,
        crate::output::now_timestamp(),
    )
    .await
    {
        Ok(Admission::Created {
            journal_id,
            owner,
            frozen,
            past_due,
        }) => (journal_id, owner, *frozen, past_due),
        Ok(Admission::Existing { journal_id }) => {
            return match on_existing {
                OnExisting::Refuse => selected_error(
                    session,
                    "refused",
                    &format!("plan already executed as journal {journal_id}"),
                    8,
                ),
                OnExisting::Replay => replayed_journal(session, &journal_id, freshness),
            };
        }
        Err(e) => return map_plan_error(session, e),
    };

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
            if past_due {
                exec.warning = Some(exec.warning.map_or_else(
                    || "assignment is past due".into(),
                    |w| format!("assignment is past due; {w}"),
                ));
            }
            emit_submit_ok(session, &exec, &frozen, freshness, false)
        }
        Err(e) => map_execute_error(session, &journal_id, e, freshness),
    }
}

/// The `submit@1` envelope of the journal a plan already admitted.
///
/// A replayed acceptance, a second execute in flight, and a lost response all
/// arrive here. Each one reports the journal that exists, with that journal's
/// own outcome and exit and `replayed: true`. No second journal is created,
/// and no bare refusal is returned (SPEC §19 item 17).
fn replayed_journal(session: &Session, journal_id: &str, freshness: Vec<Freshness>) -> Handled {
    let Ok(Some(row)) = get_journal(&session.open.store, journal_id) else {
        return selected_error(
            session,
            "local",
            &format!("plan is linked to journal {journal_id}, which is missing"),
            13,
        );
    };
    let receipt = canvas_core::receipts::document_from_row(&row).ok();
    let exec = ExecuteOutcome {
        state: row.state,
        journal_id: journal_id.to_owned(),
        receipt_id: receipt.as_ref().map(|doc| doc.receipt_id.clone()),
        attribution: receipt.as_ref().map(|doc| doc.attribution.clone()),
        post_status: row.post_status,
        response_kind: row.response_kind.as_deref().and_then(|s| s.parse().ok()),
        reconcile: None,
        warning: None,
    };
    emit_submit_ok(session, &exec, &empty_input(), freshness, true)
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
    freshness: Vec<Freshness>,
    replayed: bool,
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
    result.replayed = replayed;
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
        replayed: false,
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

/// Invalidate a plan the user did not approve, so it can never be executed.
pub fn cancel_plan(session: &Session, plan_id: &str) {
    let _ = canvas_core::plan::cancel(&session.open.store, plan_id);
}

/// Map a plan-layer failure onto the §14 exit codes.
///
/// Every plan refusal is exit 8 and carries the REPORT §3.2 reason
/// (`expired`, `invalidated`, or `approval_required`) in `details`.
pub fn map_plan_error(session: &Session, err: PlanError) -> Handled {
    if let Some(reason) = err.refusal_reason() {
        let reason = reason.to_owned();
        return plan_refusal(session, &reason, &err.to_string());
    }
    match err {
        PlanError::Preflight(e) => map_preflight_error(session, e),
        PlanError::InProgress { journal_id } => {
            map_submit_error(session, SubmitError::InProgress { journal_id })
        }
        PlanError::Journal(e) => map_submit_error(session, SubmitError::Journal(e)),
        PlanError::Store(e) => selected_error(session, "local", &e.to_string(), 13),
        PlanError::Json(e) => selected_error(session, "local", &e.to_string(), 13),
        PlanError::Io(e) => selected_error(session, "local", &e.to_string(), 13),
        // `refusal_reason` covers every remaining variant.
        other => plan_refusal(session, "invalidated", &other.to_string()),
    }
}

/// A plan refusal: exit 8 with the REPORT §3.2 reason in `details`.
pub fn plan_refusal(session: &Session, reason: &str, message: &str) -> Handled {
    plan_refusal_with(session, message, serde_json::json!({ "reason": reason }))
}

/// A plan refusal with caller-supplied `details`.
///
/// Exit 8 is a refusal, so the envelope says `refused`: §7 keeps `outcome`
/// and `exit` consistent, and a host reads `outcome` first.
fn plan_refusal_with(session: &Session, message: &str, details: serde_json::Value) -> Handled {
    let mut env = crate::output::error_envelope("refused", message, None, details, 8);
    env.outcome = Outcome::Refused;
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    let line = env.result.message.clone();
    Handled::error_envelope(env, line)
}

// ------------------------------------------------------- the agent surface
//
// `submission.prepare` and `submission.execute` (REPORT §3.2) are the same
// three steps `canvas submit` runs, split across two round trips so the
// approval can be a person's answer in a host instead of a TTY prompt. The
// plan core is the enforcement: a handle is server-issued, bound to one plan
// and one consumer, and spent once.

/// A prepared plan and the handle that can approve it.
pub struct Pending {
    /// The plan waiting for a decision.
    pub plan_id: String,
    /// The server-issued handle, bound to this plan and this consumer.
    pub handle: String,
    /// The plan as `plan@1` renders it, for the host to show a person.
    pub plan: PlanJson,
    /// One line naming what is about to be sent.
    pub summary: String,
}

/// What `submission.execute` produced.
pub enum Admitted {
    /// The plan ran, replayed, or was refused: this is its envelope.
    Done(Handled),
    /// The plan needs a recorded human decision first.
    NeedsApproval(Box<Pending>),
}

/// How a person ended an approval round trip without approving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The person declined this plan.
    Declined,
    /// The person cancelled the operation.
    Cancelled,
}

/// `submission.prepare`: freeze a plan and stop (REPORT §3.2).
///
/// Nothing reaches Canvas here. The stored plan is `prepared`, the envelope is
/// `plan@1`, and `plan_id` is what `submission.execute` needs next.
pub async fn agent_prepare(globals: &Globals, args: SubmitArgs, consumer: &str) -> Handled {
    let frozen = match freeze_plan(globals, args, Some(consumer)).await {
        Ok(frozen) => frozen,
        Err(handled) => return handled,
    };
    let Frozen {
        session,
        prepared,
        freshness,
    } = *frozen;
    let result = PlanResult {
        plan: PlanJson::of(&prepared.plan),
    };
    let mut envelope = base_envelope(SCHEMA_PLAN, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    envelope
        .warnings
        .push("this plan needs a recorded human approval before anything is sent".to_owned());
    Handled::new(envelope, move |envelope| {
        render_plan(io::stdout(), &envelope.result)
    })
}

/// `submission.execute`: run an approved plan, or ask for the approval.
///
/// A prepared plan yields a [`Pending`] and dispatches nothing. An executed
/// plan replays its journal. Every other state is the plan layer's refusal.
pub async fn agent_execute(globals: &Globals, plan_id: &str, consumer: &str) -> Admitted {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return Admitted::Done(session_error(e, globals.profile.clone())),
    };
    let plan = match canvas_core::plan::require(&session.open.store, plan_id) {
        Ok(plan) => plan,
        Err(e) => return Admitted::Done(map_plan_error(&session, e)),
    };
    // A plan that already admitted a journal replays it, and that is a local
    // read: it answers even when the network is gone (SPEC §19 item 17).
    if plan.state == PlanState::Executed {
        return Admitted::Done(match &plan.journal_id {
            Some(journal_id) => replayed_journal(&session, journal_id, Vec::new()),
            None => plan_refusal(&session, "invalidated", "executed plan has no journal"),
        });
    }
    if plan.state == PlanState::Prepared {
        let handle =
            match canvas_core::plan::issue_handle(&session.open.store, plan_id, Some(consumer)) {
                Ok(handle) => handle,
                Err(e) => return Admitted::Done(map_plan_error(&session, e)),
            };
        return Admitted::NeedsApproval(Box::new(Pending {
            plan_id: plan.plan_id.clone(),
            handle,
            summary: plan_summary(&plan),
            plan: PlanJson::of(&plan),
        }));
    }
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return Admitted::Done(code),
    };
    Admitted::Done(run_plan(&session, client, plan_id, Vec::new(), OnExisting::Replay).await)
}

/// Record an elicited approval and dispatch the plan.
///
/// The handle is validated against the stored row inside `approve`, so an
/// echoed request state that names a handle this server never issued cannot
/// approve anything.
pub async fn agent_approve(
    globals: &Globals,
    plan_id: &str,
    handle: &str,
    consumer: &str,
) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    if let Err(e) = canvas_core::plan::approve(
        &session.open.store,
        plan_id,
        handle,
        ApprovalChannel::Elicitation,
        Some(consumer),
        crate::output::now_timestamp(),
    ) {
        return map_plan_error(&session, e);
    }
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    run_plan(&session, client, plan_id, Vec::new(), OnExisting::Replay).await
}

/// A person declined or cancelled the plan: invalidate it and say so.
///
/// Declining and cancelling both spend every handle issued for the plan, so
/// neither the plan nor a handle can be replayed afterwards.
pub fn agent_refuse(globals: &Globals, plan_id: &str, refusal: Refusal) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let invalidated = match refusal {
        Refusal::Declined => canvas_core::plan::decline(&session.open.store, plan_id),
        Refusal::Cancelled => canvas_core::plan::cancel(&session.open.store, plan_id),
    };
    if let Err(e) = invalidated {
        return map_plan_error(&session, e);
    }
    let message = match refusal {
        Refusal::Declined => "submission declined",
        Refusal::Cancelled => "submission cancelled",
    };
    selected_error(&session, "cancelled", message, 11)
}

/// The refusal a host that cannot ask a person gets (REPORT §3.2).
///
/// Outcome `refused`, exit 8, reason `approval_required`. The handle travels
/// so the approval can still be recorded through another channel, and nothing
/// is dispatched.
pub fn agent_approval_required(globals: &Globals, pending: &Pending) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    plan_refusal_with(
        &session,
        "this submission needs a recorded human approval, and this host declared no elicitation support",
        serde_json::json!({
            "reason": "approval_required",
            "plan_id": pending.plan_id,
            "handle": pending.handle,
        }),
    )
}

/// One line naming what a plan is about to send.
fn plan_summary(plan: &canvas_core::plan::PlanRow) -> String {
    let name = plan
        .payload
        .assignment_name
        .as_deref()
        .unwrap_or("this assignment");
    let what = match plan.kind {
        InputKind::OnlineUpload => {
            let count = plan.payload.files.len();
            let bytes: u64 = plan.payload.files.iter().map(|f| f.size).sum();
            format!("{count} file(s), {bytes} bytes")
        }
        InputKind::OnlineTextEntry => "a text entry".to_owned(),
        InputKind::OnlineHtml => "an HTML entry".to_owned(),
        InputKind::OnlineUrl => "a website URL".to_owned(),
    };
    let due = plan
        .payload
        .due_at
        .as_deref()
        .map_or_else(|| "no due date".to_owned(), |due| format!("due {due}"));
    format!(
        "Submit {what} to \"{name}\" as attempt {} ({due}).",
        plan.baseline_attempt + 1
    )
}

/// Human form of `plan@1`.
fn render_plan(mut out: impl Write, result: &PlanResult) -> io::Result<()> {
    let plan = &result.plan;
    writeln!(out, "plan {}  {}", plan.plan_id, plan.state)?;
    writeln!(
        out,
        "course {}  assignment {}  kind {}",
        plan.course_id, plan.assignment_id, plan.kind
    )?;
    writeln!(
        out,
        "attempt {}  expires {}",
        plan.estimated_attempt, plan.expires_at
    )?;
    for file in &plan.files {
        writeln!(
            out,
            "file {} ({} bytes, sha256 {})",
            file.name, file.size, file.sha256
        )?;
    }
    if let Some(text) = &plan.text {
        writeln!(
            out,
            "text transform={}  input_sha256={}  sent_sha256={}",
            text.transform, text.input_sha256, text.sent_sha256
        )?;
    }
    if let Some(url) = &plan.url {
        writeln!(out, "url {url}")?;
    }
    Ok(())
}

fn map_preflight_error(session: &Session, err: PreflightError) -> Handled {
    map_submit_error(session, err.into())
}

fn map_execute_error(
    session: &Session,
    journal_id: &str,
    err: ExecuteError,
    freshness: Vec<Freshness>,
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
        return emit_submit_ok(session, &exec, &empty_input(), freshness, false);
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

/// The frozen input a journal-derived result does not need.
///
/// Every field `build_submit_result` would take from it is overwritten by
/// [`hydrate_submit_result`] from the journal row itself.
fn empty_input() -> FrozenInput {
    FrozenInput {
        kind: InputKind::OnlineUpload,
        payload: canvas_core::journal::IntendedPayload::default(),
        file_paths: vec![],
    }
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

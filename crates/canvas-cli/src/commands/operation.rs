//! `discussion reply`, `inbox send`, `inbox reply`, and `operation
//! status|reconcile` (class D mutations and their two read-backs).
//!
//! All three writes run the same three steps `canvas submit` runs — freeze a
//! plan, record an approval, execute the approved plan — on the same plan
//! layer (REPORT §3.5). What differs is only what is frozen and where it goes.
//! Nothing here dispatches without a recorded approval, nothing resends, and
//! nothing claims more than it observed.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canvas_core::operations::{
    Admitted as OpAdmitted, DiscussionReplyRequest, InboxReplyRequest, InboxSendRequest, OpState,
    OperationError, OperationRow, PreparedOperation, Reconciled, Verdict,
};
use canvas_core::plan::ApprovalChannel;

use super::Globals;
use super::emit::{base_envelope, emit_error, require_client, session_error, sync_error};
use super::handled::Handled;
use super::submit::{
    cancel_plan, confirm_tty, map_plan_error, plan_refusal, record_approval, selected_error,
};
use crate::output::{
    Freshness, OperationMatchJson, OperationReadbackJson, OperationReconcileResult,
    OperationResult, Outcome, SCHEMA_OPERATION, SCHEMA_OPERATION_RECONCILE,
};
use crate::session::Session;

/// Operands and flags of one `canvas discussion reply`.
#[derive(Debug, Clone, Default)]
pub struct DiscussionReplyArgs {
    pub course: String,
    pub discussion: String,
    pub to: Option<String>,
    pub text: Option<String>,
    pub text_file: Option<PathBuf>,
    pub attach: Vec<PathBuf>,
    pub yes: bool,
}

/// Operands and flags of one `canvas inbox send`.
#[derive(Debug, Clone, Default)]
pub struct InboxSendArgs {
    pub to: Vec<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub text_file: Option<PathBuf>,
    pub attach: Vec<PathBuf>,
    pub yes: bool,
}

/// Operands and flags of one `canvas inbox reply`.
#[derive(Debug, Clone, Default)]
pub struct InboxReplyArgs {
    pub conversation_id: String,
    pub text: Option<String>,
    pub text_file: Option<PathBuf>,
    pub attach: Vec<PathBuf>,
    pub yes: bool,
}

/// Which of the three writes an argument set describes.
#[derive(Debug, Clone)]
pub enum WriteArgs {
    /// `discussion reply`.
    DiscussionReply(Box<DiscussionReplyArgs>),
    /// `inbox send`.
    InboxSend(Box<InboxSendArgs>),
    /// `inbox reply`.
    InboxReply(Box<InboxReplyArgs>),
}

impl WriteArgs {
    /// Whether the caller already recorded the decision with `--yes`.
    const fn yes(&self) -> bool {
        match self {
            Self::DiscussionReply(a) => a.yes,
            Self::InboxSend(a) => a.yes,
            Self::InboxReply(a) => a.yes,
        }
    }
}

// ------------------------------------------------------------ the human flow

/// Run `canvas discussion reply` for the CLI: one envelope, one exit code.
pub async fn run_discussion_reply(globals: &Globals, args: DiscussionReplyArgs) -> ExitCode {
    handle_write(globals, WriteArgs::DiscussionReply(Box::new(args)))
        .await
        .emit(globals.json)
}

/// Run `canvas inbox send` for the CLI.
pub async fn run_inbox_send(globals: &Globals, args: InboxSendArgs) -> ExitCode {
    handle_write(globals, WriteArgs::InboxSend(Box::new(args)))
        .await
        .emit(globals.json)
}

/// Run `canvas inbox reply` for the CLI.
pub async fn run_inbox_reply(globals: &Globals, args: InboxReplyArgs) -> ExitCode {
    handle_write(globals, WriteArgs::InboxReply(Box::new(args)))
        .await
        .emit(globals.json)
}

/// Freeze, confirm, and dispatch one write.
pub async fn handle_write(globals: &Globals, args: WriteArgs) -> Handled {
    let yes = args.yes();
    let frozen = match freeze(globals, args, None).await {
        Ok(frozen) => frozen,
        Err(handled) => return handled,
    };
    let Frozen {
        session,
        prepared,
        freshness,
    } = *frozen;
    let plan_id = prepared.plan.plan_id.clone();
    print_plan(&prepared);

    let channel = if yes {
        // `--yes` is recorded as itself; it never claims an interactive answer.
        ApprovalChannel::YesFlag
    } else {
        match confirm_tty("Send?").await {
            Ok(true) => ApprovalChannel::Tty,
            Ok(false) => {
                cancel_plan(&session, &plan_id);
                return selected_error(&session, "cancelled", "operation cancelled", 11);
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
    // The human flow approves the plan it just froze, so a plan that already
    // admitted a journal is a refusal here (SPEC §19 item 17 covers the agent
    // surface, which replays instead).
    run_plan(&session, client, &plan_id, freshness).await
}

/// A frozen operation plan, the session holding it, and the reads it cost.
struct Frozen {
    session: Session,
    prepared: PreparedOperation,
    freshness: Vec<Freshness>,
}

/// Freeze one operation without approving anything.
///
/// Every prepare refusal in the brief happens inside `canvas-core::operations`
/// and arrives here as an [`OperationError::Refused`] with its reason.
async fn freeze(
    globals: &Globals,
    args: WriteArgs,
    consumer: Option<&str>,
) -> Result<Box<Frozen>, Handled> {
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

    let mut freshness = Vec::new();
    let prepared =
        {
            let client = match require_client(globals, &session) {
                Ok(c) => c,
                Err(code) => return Err(code),
            };
            if let Err(e) = session.validate_network_token().await {
                return Err(sync_error(&session, &e));
            }
            let now = crate::output::now_timestamp();
            let identity_dir = session.paths.identity_dir.clone();
            let identity_key = session.identity.key.clone();

            match args {
                WriteArgs::DiscussionReply(args) => {
                    let body = match read_body(args.text.as_deref(), args.text_file.as_deref()) {
                        Ok(body) => body,
                        Err(message) => return Err(usage(&session, &message)),
                    };
                    let (course, mut course_freshness, _) =
                        match super::course::resolve_with_refresh(globals, &session, &args.course)
                            .await
                        {
                            Ok(v) => v,
                            Err(code) => return Err(code),
                        };
                    freshness.append(&mut course_freshness);
                    let topic_id = match super::discussions::topic_id_of(
                        &args.discussion,
                        &session.identity.origin,
                        course.id,
                    ) {
                        Ok(id) => id,
                        Err(message) => return Err(resolution(&session, message)),
                    };
                    let parent_entry_id = match args.to.as_deref().map(parse_id).transpose() {
                        Ok(id) => id,
                        Err(message) => return Err(resolution(&session, message)),
                    };
                    let request = DiscussionReplyRequest {
                        identity_dir: &identity_dir,
                        identity_key: identity_key.as_str(),
                        consumer,
                        course_id: course.id,
                        course_code: course.code.as_deref(),
                        topic_id,
                        parent_entry_id,
                        body: &body,
                        attachments: &args.attach,
                    };
                    canvas_core::operations::prepare_discussion_reply(
                        client,
                        &session.open.store,
                        &request,
                        now,
                    )
                    .await
                }
                WriteArgs::InboxSend(args) => {
                    let body = match read_body(args.text.as_deref(), args.text_file.as_deref()) {
                        Ok(body) => body,
                        Err(message) => return Err(usage(&session, &message)),
                    };
                    let request = InboxSendRequest {
                        identity_dir: &identity_dir,
                        identity_key: identity_key.as_str(),
                        consumer,
                        recipients: &args.to,
                        subject: args.subject.as_deref(),
                        body: &body,
                        attachments: &args.attach,
                    };
                    canvas_core::operations::prepare_inbox_send(
                        client,
                        &session.open.store,
                        &request,
                        now,
                    )
                    .await
                }
                WriteArgs::InboxReply(args) => {
                    let body = match read_body(args.text.as_deref(), args.text_file.as_deref()) {
                        Ok(body) => body,
                        Err(message) => return Err(usage(&session, &message)),
                    };
                    let conversation_id = match parse_id(&args.conversation_id) {
                        Ok(id) => id,
                        Err(message) => return Err(resolution(&session, message)),
                    };
                    let request = InboxReplyRequest {
                        identity_dir: &identity_dir,
                        identity_key: identity_key.as_str(),
                        consumer,
                        conversation_id,
                        body: &body,
                        attachments: &args.attach,
                    };
                    canvas_core::operations::prepare_inbox_reply(
                        client,
                        &session.open.store,
                        &request,
                        now,
                    )
                    .await
                }
            }
        };

    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(e) => return Err(map_operation_error(&session, e)),
    };
    for (jid, state) in &prepared.recovered {
        let _ = writeln!(io::stderr(), "recovered operation {jid} → {state}");
    }
    Ok(Box::new(Frozen {
        session,
        prepared,
        freshness,
    }))
}

/// Admit an approved operation plan and dispatch it.
async fn run_plan(
    session: &Session,
    client: &canvas_api::Client,
    plan_id: &str,
    freshness: Vec<Freshness>,
) -> Handled {
    let (journal_id, owner) = match canvas_core::operations::execute(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        session.identity.key.as_str(),
        plan_id,
        crate::output::now_timestamp(),
    )
    .await
    {
        Ok(OpAdmitted::Created { journal_id, owner }) => (journal_id, owner),
        Ok(OpAdmitted::Existing { journal_id }) => {
            return selected_error(
                session,
                "refused",
                &format!("plan already executed as journal {journal_id}"),
                8,
            );
        }
        Err(e) => return map_operation_error(session, e),
    };

    match canvas_core::operations::post(client, &session.open.store, &owner, &journal_id).await {
        Ok(posted) => emit_operation(session, &posted.row, freshness, false, posted.warning),
        Err(e) => {
            // The journal outlives the failure: report the row, not the error,
            // whenever the row already says what happened.
            match canvas_core::operations::get(&session.open.store, &journal_id) {
                Ok(Some(row)) if row.state.is_terminal() => {
                    emit_operation(session, &row, freshness, false, Some(e.to_string()))
                }
                _ => map_operation_error(session, e),
            }
        }
    }
}

// ------------------------------------------------------- status and reconcile

/// Run `canvas operation status` for the CLI.
pub async fn run_status(globals: &Globals, journal_id: String) -> ExitCode {
    handle_status(globals, journal_id).await.emit(globals.json)
}

/// Run `canvas operation reconcile` for the CLI.
pub async fn run_reconcile(
    globals: &Globals,
    journal_id: String,
    assume_not_posted: bool,
) -> ExitCode {
    handle_reconcile(globals, journal_id, assume_not_posted)
        .await
        .emit(globals.json)
}

/// `operation status`: read the thread back and print the journal.
pub async fn handle_status(globals: &Globals, journal_id: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    // Offline, the journal itself is still an honest answer: it says what this
    // process observed, without pretending to have read Canvas again.
    if globals.offline {
        return match canvas_core::operations::get(&session.open.store, &journal_id) {
            Ok(Some(row)) => emit_operation(&session, &row, Vec::new(), false, None),
            Ok(None) => not_found(&session, &journal_id),
            Err(e) => map_operation_error(&session, e),
        };
    }
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match canvas_core::operations::status(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        &journal_id,
    )
    .await
    {
        Ok(done) => emit_operation(&session, &done.row, Vec::new(), false, done.warning),
        Err(e) => map_operation_error(&session, e),
    }
}

/// `operation reconcile`: resolve one journal against the thread as it stands.
pub async fn handle_reconcile(
    globals: &Globals,
    journal_id: String,
    assume_not_posted: bool,
) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let client = match require_client(globals, &session) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let now = crate::output::now_timestamp();
    match canvas_core::operations::reconcile(
        client,
        &session.open.store,
        &session.paths.identity_dir,
        &journal_id,
        assume_not_posted,
        now,
    )
    .await
    {
        Ok(done) => emit_reconcile(&session, &done, now),
        Err(e) => map_operation_error(&session, e),
    }
}

fn emit_reconcile(session: &Session, done: &Reconciled, now: jiff::Timestamp) -> Handled {
    let row = &done.row;
    let (outcome, exit) = outcome_of(row.state);
    let message = verdict_message(done);
    let result = OperationReconcileResult {
        outcome: outcome_name(outcome),
        journal_id: row.journal_id.clone(),
        kind: row.kind.as_str().to_owned(),
        state: row.state.as_str().to_owned(),
        verdict: done.verdict.as_str().to_owned(),
        owner: done.owner.as_str().to_owned(),
        attribution: row.attribution.as_str().to_owned(),
        delivery: row.delivery().to_owned(),
        receipt_id: row.receipt_id(),
        readback: done.readback.as_ref().map(OperationReadbackJson::of),
        server_match: row.server_match.as_ref().map(OperationMatchJson::of),
        assume_not_posted_available: assume_available(row, now),
        message: message.clone(),
    };
    let mut envelope = base_envelope(SCHEMA_OPERATION_RECONCILE, session, result);
    envelope.outcome = outcome;
    envelope.exit = exit;
    envelope.requests = session.requests();
    if let Some(warning) = &done.warning {
        envelope.warnings.push(warning.clone());
    }
    Handled::new(envelope, move |envelope| {
        render_reconcile(io::stdout(), &envelope.result)
    })
}

/// Whether `--assume-not-posted` can be used now.
///
/// Only an unknown outcome can be assumed away, and only after the §12.2 grace
/// window, so a caller is never invited to assert something too early.
fn assume_available(row: &OperationRow, now: jiff::Timestamp) -> bool {
    if row.state != OpState::OutcomeUnknown {
        return false;
    }
    let Some(started) = row.posting_started_at.as_deref() else {
        return false;
    };
    let Ok(started) = started.parse::<jiff::Timestamp>() else {
        return false;
    };
    now.as_nanosecond() - started.as_nanosecond()
        >= canvas_core::operations::ASSUME_AFTER.as_nanos()
}

fn verdict_message(done: &Reconciled) -> String {
    let row = &done.row;
    let what = match row.kind {
        canvas_core::operations::OperationKind::DiscussionReply => "the reply",
        _ => "the message",
    };
    match done.verdict {
        Verdict::Observed => format!(
            "{what} this journal describes is in the thread; {}",
            claim(row)
        ),
        Verdict::Matched => format!(
            "the thread holds a message with the same digest, and nothing links it to this \
             request; {}",
            claim(row)
        ),
        Verdict::NotFound => format!("the thread does not hold {what} this journal describes"),
        Verdict::NotRead => "the thread was not read".to_owned(),
        Verdict::AssumedNotPosted => {
            format!("nothing was posted: {what} is recorded as never sent")
        }
    }
}

// -------------------------------------------------------------- the envelopes

/// Emit one `operation@1` envelope.
fn emit_operation(
    session: &Session,
    row: &OperationRow,
    freshness: Vec<Freshness>,
    replayed: bool,
    warning: Option<String>,
) -> Handled {
    let (outcome, exit) = outcome_of(row.state);
    let mut result = OperationResult::of(row);
    result.replayed = replayed;
    result.outcome = outcome_name(outcome);

    let mut envelope = base_envelope(SCHEMA_OPERATION, session, result);
    envelope.freshness = freshness;
    envelope.outcome = outcome;
    envelope.exit = exit;
    envelope.requests = session.requests();
    if row.state == OpState::OutcomeUnknown {
        envelope.warnings.push(
            "the original request may still complete; operation reconcile re-checks; \
             --assume-not-posted becomes available after 30 minutes"
                .to_owned(),
        );
    }
    if let Some(warning) = warning {
        envelope.warnings.push(warning);
    }
    Handled::new(envelope, move |envelope| {
        render_operation(io::stdout(), &envelope.result)
    })
}

/// The §7 outcome and the §14 exit of one journal state.
///
/// `failed` is a refusal by Canvas and `refused` a refusal by this process:
/// both mean nothing was written, so both are exit 8. An unknown outcome is
/// exit 9, because it is exactly the case a person has to resolve.
const fn outcome_of(state: OpState) -> (Outcome, u8) {
    match state {
        OpState::Posted | OpState::Matched => (Outcome::Ok, 0),
        OpState::Refused | OpState::Failed => (Outcome::Refused, 8),
        OpState::Planned | OpState::Posting | OpState::OutcomeUnknown => (Outcome::Recovery, 9),
    }
}

fn outcome_name(outcome: Outcome) -> String {
    match outcome {
        Outcome::Ok => "ok",
        Outcome::Recovery => "recovery",
        Outcome::Refused => "refused",
        Outcome::Mismatch => "mismatch",
        Outcome::Partial => "partial",
        Outcome::Error => "error",
    }
    .to_owned()
}

/// The one line that says how much this journal can claim.
///
/// A Canvas acceptance of a conversation is never described as delivered mail.
fn claim(row: &OperationRow) -> String {
    let observable = row.delivery() == "observable";
    match row.attribution {
        canvas_core::operations::Attribution::Accepted if observable => {
            "accepted by Canvas, which named the entry it created".to_owned()
        }
        canvas_core::operations::Attribution::Accepted => {
            "accepted by Canvas; delivery is not observable".to_owned()
        }
        canvas_core::operations::Attribution::Observed if observable => {
            "accepted by Canvas, and a later read of the thread shows it".to_owned()
        }
        canvas_core::operations::Attribution::Observed => {
            "accepted by Canvas, and a later read shows it in the conversation; \
             delivery is not observable"
                .to_owned()
        }
        canvas_core::operations::Attribution::Unproven => {
            "unproven: a message with the same digest is there, and nothing links it to this \
             request"
                .to_owned()
        }
        canvas_core::operations::Attribution::None => {
            "nothing links this journal to an object in Canvas".to_owned()
        }
    }
}

// ---------------------------------------------------------------- the renders

pub(super) fn render_operation(mut out: impl Write, result: &OperationResult) -> io::Result<()> {
    writeln!(
        out,
        "{} {}  journal={}",
        result.kind, result.state, result.journal_id
    )?;
    render_target(&mut out, &result.target)?;
    if let Some(subject) = &result.subject {
        writeln!(out, "subject {subject}")?;
    }
    writeln!(
        out,
        "text transform={}  input_sha256={}  sent_sha256={}",
        result.text.transform, result.text.input_sha256, result.text.sent_sha256
    )?;
    if !result.attachments.is_empty() {
        let mut table = crate::output::new_table();
        table.set_header(["Attachment", "Bytes", "SHA-256", "Canvas ID"]);
        for attachment in &result.attachments {
            table.add_row(vec![
                attachment.name.clone(),
                attachment.size.to_string(),
                attachment.sha256.clone(),
                attachment
                    .canvas_file_id
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            ]);
        }
        crate::output::apply_two_space_padding(&mut table);
        writeln!(out, "{table}")?;
    }
    if let Some(id) = &result.receipt_id {
        writeln!(out, "receipt {id}  attribution={}", result.attribution)?;
    }
    if let Some(response) = &result.response {
        writeln!(
            out,
            "canvas id {}  created {}",
            response.id.as_deref().unwrap_or("unknown"),
            response.created_at_local.as_deref().unwrap_or("unknown")
        )?;
    }
    writeln!(out, "delivery {}", result.delivery)?;
    if let Some(evidence) = &result.not_posted_evidence {
        writeln!(out, "nothing was posted ({evidence})")?;
    }
    if let Some(error) = &result.error {
        writeln!(out, "{error}")?;
    }
    Ok(())
}

/// Print the thread or the recipients one operation names.
pub(super) fn render_target(
    mut out: impl Write,
    target: &crate::output::OperationTargetJson,
) -> io::Result<()> {
    match target.kind.as_str() {
        "discussion_reply" => {
            writeln!(
                out,
                "topic {}{}  course {}",
                target.topic_id.as_deref().unwrap_or("unknown"),
                target
                    .topic_title
                    .as_deref()
                    .map(|t| format!(" \"{t}\""))
                    .unwrap_or_default(),
                target.course_id.as_deref().unwrap_or("unknown"),
            )?;
            if let Some(entry) = &target.parent_entry_id {
                writeln!(out, "in reply to entry {entry}")?;
            }
        }
        "inbox_send" => {
            let names = if target.recipient_names.is_empty() {
                target.recipients.join(", ")
            } else {
                target
                    .recipients
                    .iter()
                    .zip(target.recipient_names.iter())
                    .map(|(id, name)| format!("{name} ({id})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            writeln!(out, "to {names}")?;
            if let Some(id) = &target.conversation_id {
                writeln!(out, "conversation {id}")?;
            }
        }
        _ => {
            writeln!(
                out,
                "conversation {}{}",
                target.conversation_id.as_deref().unwrap_or("unknown"),
                target
                    .conversation_subject
                    .as_deref()
                    .map(|s| format!(" \"{s}\""))
                    .unwrap_or_default(),
            )?;
        }
    }
    Ok(())
}

fn render_reconcile(mut out: impl Write, result: &OperationReconcileResult) -> io::Result<()> {
    writeln!(
        out,
        "{} {}  journal={}  verdict={}",
        result.kind, result.state, result.journal_id, result.verdict
    )?;
    writeln!(
        out,
        "attribution {}  delivery {}  owner {}",
        result.attribution, result.delivery, result.owner
    )?;
    if let Some(readback) = &result.readback {
        writeln!(
            out,
            "read {} object(s) at {}{}",
            readback.scanned,
            readback.read_at,
            if readback.complete { "" } else { " (partial)" }
        )?;
    }
    if let Some(found) = &result.server_match {
        writeln!(out, "match {}  sha256 {}", found.id, found.body_sha256)?;
    }
    if result.assume_not_posted_available {
        writeln!(out, "--assume-not-posted is available")?;
    }
    writeln!(out, "{}", result.message)?;
    Ok(())
}

/// Print the plan a person is being asked to approve.
fn print_plan(prepared: &PreparedOperation) {
    let plan = &prepared.plan;
    let operation = &prepared.operation;
    let mut out = io::stderr();
    let _ = writeln!(out, "{} plan", operation.kind());
    let target = crate::output::OperationTargetJson::of(&operation.target, &operation.labels, None);
    let _ = render_target(&mut out, &target);
    if let Some(subject) = &operation.subject {
        let _ = writeln!(out, "  subject {subject}");
    }
    let _ = writeln!(
        out,
        "  text transform={} input_sha256={} sent_sha256={}",
        operation.body.transform, operation.body.input_sha256, operation.body.sent_sha256
    );
    for attachment in &operation.attachments {
        let _ = writeln!(
            out,
            "  attach {} ({} bytes, sha256 {})",
            attachment.name, attachment.size, attachment.sha256
        );
    }
    let _ = writeln!(out, "  expires {}", plan.expires_at);
}

// ------------------------------------------------------------------- helpers

/// One byte past the §12.2 step 5 bound, so an over-long body is refused
/// rather than read.
const BODY_READ_LIMIT: u64 = 1_048_577;

/// Read the body from `--text`, `--text -`, or `--text-file`.
///
/// Both file and stdin are read under the §12.2 step 5 bound, so a path that
/// names a huge file — or a pipe that never ends — is refused instead of held
/// in memory. `frozen_body` applies the bound itself as well.
fn read_body(text: Option<&str>, file: Option<&std::path::Path>) -> Result<String, String> {
    if let Some(path) = file {
        let handle = std::fs::File::open(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        return bounded(handle).map_err(|e| format!("cannot read {}: {e}", path.display()));
    }
    match text {
        Some("-") => bounded(io::stdin()).map_err(|e| format!("cannot read stdin: {e}")),
        Some(text) => Ok(text.to_owned()),
        None => Err("one of --text or --text-file is required".to_owned()),
    }
}

fn bounded(source: impl Read) -> Result<String, String> {
    let mut buf = Vec::new();
    source
        .take(BODY_READ_LIMIT)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|_| "the body is not valid UTF-8".to_owned())
}

fn parse_id(raw: &str) -> Result<i64, &'static str> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or("an id must be a positive number")
}

fn usage(session: &Session, message: &str) -> Handled {
    emit_error(
        "usage",
        message,
        2,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn resolution(session: &Session, message: &str) -> Handled {
    emit_error(
        "resolution",
        message,
        6,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn not_found(session: &Session, journal_id: &str) -> Handled {
    resolution(session, &format!("operation {journal_id} not found"))
}

/// Map an operation-layer failure onto the §14 exit codes.
pub fn map_operation_error(session: &Session, err: OperationError) -> Handled {
    if let Some(reason) = err.refusal_reason() {
        return plan_refusal(session, reason, &err.to_string());
    }
    match err {
        OperationError::NotFound => resolution(session, "operation not found"),
        OperationError::Resolution(message) => resolution(session, &message),
        OperationError::InProgress => selected_error(
            session,
            "refused",
            "another process is writing to this target",
            8,
        ),
        OperationError::Plan(e) => map_plan_error(session, *e),
        OperationError::Lock(canvas_core::journal::LockError::InProgress) => {
            selected_error(session, "refused", "in_progress", 8)
        }
        OperationError::Network(e) => sync_error(session, &e.into()),
        OperationError::StateConflict => selected_error(session, "local", "state conflict", 13),
        OperationError::Lock(e) => selected_error(session, "local", &e.to_string(), 13),
        OperationError::Store(e) => selected_error(session, "local", &e.to_string(), 13),
        OperationError::Json(e) => selected_error(session, "local", &e.to_string(), 13),
        OperationError::Io(e) => selected_error(session, "local", &e.to_string(), 13),
        // `refusal_reason` answered this one at the top of the function.
        other @ OperationError::Refused { .. } => {
            plan_refusal(session, "invalidated", &other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn operation_human_snapshot() {
        let result =
            serde_json::from_str(include_str!("../output/schemas/operation.json")).unwrap();
        let mut out = Vec::new();
        super::render_operation(&mut out, &result).unwrap();
        insta::assert_snapshot!("operation_human", String::from_utf8(out).unwrap());
    }

    #[test]
    fn operation_reconcile_human_snapshot() {
        let result =
            serde_json::from_str(include_str!("../output/schemas/operation_reconcile.json"))
                .unwrap();
        let mut out = Vec::new();
        super::render_reconcile(&mut out, &result).unwrap();
        insta::assert_snapshot!("operation_reconcile_human", String::from_utf8(out).unwrap());
    }
}

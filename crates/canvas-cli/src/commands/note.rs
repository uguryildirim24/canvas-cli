//! `canvas note` — `note@1` (class C).
//!
//! One inert note, held by the broker for the attachment's lifetime and shown
//! to the person in the companion's side panel.
//!
//! What a note is not, in one place, because it is the whole point of the
//! bounds around it:
//!
//! - it is **not** a Canvas write. Nothing here reaches Canvas at all;
//! - it is **not** an approval. There is no approval operation on
//!   `bridge-ipc@1`, so no note — whatever its text or its refs say — can
//!   approve, decline, or cancel a plan;
//! - it is **not** HTML. The text travels as Markdown source, and the panel
//!   renders a sanitized subset of it that builds no script, no image, and no
//!   link to anywhere but the granted origin.

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{Body, Op, Reason};

use super::Globals;
use super::emit::session_error;
use super::handled::Handled;
use crate::bridge::client;
use crate::output::{Envelope, NoteJson, NoteResult, Outcome, SCHEMA_NOTE, generated_at_now};

/// Run `canvas note` for the CLI: one envelope, one exit code.
pub async fn run(
    globals: &Globals,
    attachment: Option<String>,
    generation: Option<u64>,
    text: String,
    source_refs: Vec<String>,
) -> ExitCode {
    handle(globals, attachment, None, generation, text, source_refs)
        .await
        .emit(globals.json)
}

/// Hold one note for the attachment.
///
/// `generation` is the navigation generation the note was written against.
/// The CLI may omit it, because a person typing a note is looking at the tab
/// while they type; the agent surfaces always pass one, because an agent
/// works from a bundle it read earlier, and that bundle may describe a page
/// the person has already left. When it is omitted the broker's current
/// generation is read first and then checked, which is a race a person can
/// see and an agent cannot.
pub async fn handle(
    globals: &Globals,
    attachment: Option<String>,
    consumer: Option<String>,
    generation: Option<u64>,
    text: String,
    source_refs: Vec<String>,
) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);

    let generation = match generation {
        Some(generation) => generation,
        None => match current_generation(&endpoint, attachment.clone(), consumer.clone()) {
            Ok(generation) => generation,
            Err(reason) => return refusal(&session, consumer, reason),
        },
    };

    let (note, attachment_id, held) = match client::call(
        &endpoint,
        Op::Note {
            attachment_id: attachment,
            consumer: consumer.clone(),
            generation,
            text,
            source_refs,
        },
    ) {
        Ok(Body::Noted {
            note,
            attachment_id,
            held,
        }) => (*note, attachment_id, held),
        Ok(_) => return refusal(&session, consumer, Reason::Protocol),
        Err(reason) => return refusal(&session, consumer, reason),
    };

    let result = NoteResult {
        attachment: Some(attachment_id),
        consumer,
        note: Some(NoteJson::from(&note)),
        held,
        reason: None,
    };
    let mut envelope = super::emit::base_envelope(SCHEMA_NOTE, &session, result);
    // A note is not a dataset read: it carries no freshness row.
    envelope.warnings = Vec::new();
    Handled::new(envelope, |envelope| {
        writeln!(io::stdout(), "{}", human(&envelope.result))
    })
}

/// The navigation generation the broker currently holds.
fn current_generation(
    endpoint: &Endpoint,
    attachment: Option<String>,
    consumer: Option<String>,
) -> Result<u64, Reason> {
    match client::call(
        endpoint,
        Op::Here {
            attachment_id: attachment,
            consumer,
            include_text: false,
        },
    )? {
        Body::Context(context) => Ok(context.navigation_generation),
        _ => Err(Reason::Protocol),
    }
}

/// A refusal: no note was held, and the reason says why.
fn refusal(session: &crate::session::Session, consumer: Option<String>, reason: Reason) -> Handled {
    let message = super::here::message_for(reason);
    let result = NoteResult {
        attachment: None,
        consumer,
        note: None,
        held: 0,
        reason: Some(reason.as_str().to_owned()),
    };
    let envelope: Envelope<NoteResult> = Envelope {
        schema: SCHEMA_NOTE.to_owned(),
        generated_at: generated_at_now(),
        profile: session.profile.clone(),
        identity: Some(session.identity_ref()),
        freshness: Vec::new(),
        requests: session.requests(),
        partial: Vec::new(),
        warnings: Vec::new(),
        outcome: Outcome::Refused,
        exit: 8,
        result,
    };
    Handled::new(envelope, move |envelope| {
        let _ = writeln!(io::stderr(), "{message}");
        writeln!(io::stdout(), "{}", human(&envelope.result))
    })
}

fn human(result: &NoteResult) -> String {
    let Some(note) = result.note.as_ref() else {
        return format!(
            "no note was held ({})",
            result.reason.as_deref().unwrap_or("unknown")
        );
    };
    format!(
        "note {} held for {} ({} in the panel)",
        note.note_id,
        result.attachment.as_deref().unwrap_or(""),
        result.held
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused note says so, and never invents a note that was not held.
    #[test]
    fn a_refusal_holds_no_note_and_names_the_reason() {
        let result = NoteResult {
            attachment: None,
            consumer: Some("mcp:host".to_owned()),
            note: None,
            held: 0,
            reason: Some("stale_generation".to_owned()),
        };
        assert_eq!(human(&result), "no note was held (stale_generation)");
    }
}

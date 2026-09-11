//! `canvas here` — `ContextBundle@1` (class C, `here@1`).
//!
//! One bundle, two sources, kept apart (REPORT §3.1):
//!
//! - `api` carries whole §7 envelopes from the shared command handlers, each
//!   with its own freshness. A browser extract never updates one of them.
//! - `browser` carries one observation of one document, with the account that
//!   was verified against this identity, and it is never cacheable.
//!
//! When there is nothing to report the bundle says why. `not_attached`,
//! `paused`, `validating`, `account_mismatch`, and `bridge_unavailable` are
//! refusals and exit 8 (REPORT §3.2). `zone_opaque` is not: the attachment is
//! healthy, and the answer is a bundle whose page content is absent.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{Body, Op, Reason};

use super::Globals;
use super::emit::session_error;
use super::handled::Handled;
use crate::bridge::client;
use crate::output::{
    Envelope, HereApiJson, HereBrowserJson, HereIdentityJson, HereResult, Outcome, SCHEMA_HERE,
    generated_at_now,
};

/// Run `canvas here` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, attachment: Option<String>, text: bool) -> ExitCode {
    handle(globals, attachment, None, text)
        .await
        .emit(globals.json)
}

/// Build one `ContextBundle@1`.
///
/// `consumer` is `None` for the CLI, which REPORT §3.2 lets select the sole
/// attachment. Every agent surface passes its own handle, and the broker
/// serves it only if that consumer attached.
pub async fn handle(
    globals: &Globals,
    attachment: Option<String>,
    consumer: Option<String>,
    include_text: bool,
) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let identity = HereIdentityJson {
        key: session.identity.key.to_string(),
        generation: session.identity.generation.to_string(),
    };
    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);

    let context = match client::call(
        &endpoint,
        Op::Here {
            attachment_id: attachment,
            consumer: consumer.clone(),
            include_text,
        },
    ) {
        Ok(Body::Context(context)) => *context,
        Ok(_) => return refusal(&session, &identity, consumer, Reason::Protocol),
        Err(reason) => return refusal(&session, &identity, consumer, reason),
    };

    let browser = HereBrowserJson::from(&context);
    let api = resolve_api(globals, &browser).await;
    let result = HereResult {
        attachment: Some(context.attachment_id.clone()),
        state: context.state.as_str().to_owned(),
        consumer,
        identity,
        api,
        browser: Some(browser),
        reason: None,
    };
    let mut envelope = super::emit::base_envelope(SCHEMA_HERE, &session, result);
    // Browser context is an observation, never a cached dataset: it carries
    // no freshness row of its own (REPORT §3.2).
    envelope.warnings = Vec::new();
    Handled::new(envelope, |envelope| {
        writeln!(io::stdout(), "{}", human(&envelope.result))
    })
}

/// The bundle a `context/<handle>` naming another consumer reads.
///
/// It is exactly the answer an unattached consumer of this session reads —
/// the same `here@1` refusal, the same reason, the same exit — so a URI
/// naming somebody else's handle cannot be used to learn whether that handle
/// attached, or even whether a broker is running.
pub fn foreign_consumer(globals: &Globals, consumer: &str) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let identity = HereIdentityJson {
        key: session.identity.key.to_string(),
        generation: session.identity.generation.to_string(),
    };
    refusal(
        &session,
        &identity,
        Some(consumer.to_owned()),
        Reason::NotAttached,
    )
}

/// Resolve the API side through the shared handlers.
///
/// Each handler produces its own §7 envelope, freshness and all, so the
/// bundle carries the same document `canvas assignment --json` prints.
async fn resolve_api(globals: &Globals, browser: &HereBrowserJson) -> HereApiJson {
    let mut api = HereApiJson::default();
    let Some(course_id) = browser.course_id.clone() else {
        return api;
    };
    api.course = Some(
        super::course::handle(globals, course_id.clone())
            .await
            .envelope()
            .to_value(),
    );
    if let Some(assignment_id) = browser.assignment_id.clone() {
        api.assignment = Some(
            super::assignment::handle(globals, course_id.clone(), Some(assignment_id))
                .await
                .envelope()
                .to_value(),
        );
    } else if let Some(topic_id) = browser.topic_id.clone() {
        // Canvas serves an announcement from the discussion route; the API
        // decides which it is, and a topic that is not one answers with its
        // own refusal inside this envelope.
        api.announcement = Some(
            super::announcement::handle(globals, course_id, Some(topic_id))
                .await
                .envelope()
                .to_value(),
        );
    }
    api
}

/// A bundle with no browser context, and the reason.
fn refusal(
    session: &crate::session::Session,
    identity: &HereIdentityJson,
    consumer: Option<String>,
    reason: Reason,
) -> Handled {
    let state = match reason {
        Reason::Paused => "paused",
        Reason::Validating => "validating",
        _ => "not_attached",
    };
    let result = HereResult {
        attachment: None,
        state: state.to_owned(),
        consumer,
        identity: identity.clone(),
        api: HereApiJson::default(),
        browser: None,
        reason: Some(reason.as_str().to_owned()),
    };
    let mut envelope: Envelope<HereResult> = Envelope {
        schema: SCHEMA_HERE.to_owned(),
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
    if !reason.is_refusal() {
        envelope.outcome = Outcome::Ok;
        envelope.exit = 0;
    }
    let message = message_for(reason);
    Handled::new(envelope, move |envelope| {
        let _ = writeln!(io::stderr(), "{message}");
        writeln!(io::stdout(), "{}", human(&envelope.result))
    })
}

/// What a person reads when a bundle is unavailable.
pub fn message_for(reason: Reason) -> String {
    match reason {
        Reason::NotAttached => "no Canvas tab is attached; open one and click the companion".into(),
        Reason::Paused => "sharing is paused; return to the tab to resume it".into(),
        Reason::Validating => "the page is being re-checked; try again in a moment".into(),
        Reason::AccountMismatch => {
            "the browser is signed in as another Canvas account; nothing was joined".into()
        }
        Reason::BridgeUnavailable => {
            "no canvas bridge host is running; run `canvas bridge status`".into()
        }
        Reason::StaleGeneration => "the page moved on; ask again".into(),
        Reason::ZoneOpaque => "this page is opaque to the companion".into(),
        Reason::Protocol => "the broker answered something this build does not understand".into(),
        Reason::NoteTooLarge => format!(
            "a note may carry at most {} bytes of text",
            canvas_core::bridge::note::MAX_NOTE_BYTES
        ),
        Reason::SourceRefRejected => {
            "a source ref must be a page of the attached Canvas, or a canvas:// reference".into()
        }
        Reason::NoteRejected => "a note needs text, and at most a handful of source refs".into(),
        Reason::OriginMismatch => {
            "that target is outside the Canvas the companion was granted".into()
        }
        Reason::NavigationTimeout => "the companion did not take the navigation; try again".into(),
    }
}

fn human(result: &HereResult) -> String {
    let Some(browser) = result.browser.as_ref() else {
        return format!(
            "not attached ({})",
            result.reason.as_deref().unwrap_or("unknown")
        );
    };
    let mut line = format!(
        "{} {} zone={}",
        result.state,
        browser.url.as_deref().unwrap_or(&browser.origin),
        browser.zone
    );
    if let Some(title) = browser.title.as_deref() {
        let _ = write!(line, "\n{title}");
    }
    if browser.text_bytes > 0 || browser.selection_bytes > 0 {
        let _ = write!(
            line,
            "\nselection {} bytes, text {} bytes{}",
            browser.selection_bytes,
            browser.text_bytes,
            if browser.truncated {
                " (truncated)"
            } else {
                ""
            }
        );
    }
    if let Some(reason) = browser.content_reason.as_deref() {
        let _ = write!(line, "\nno page content: {reason}");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M7-a acceptance: an unavailable attachment and an absent broker exit 8
    /// with the right reason.
    #[test]
    fn the_report_refusals_exit_eight_and_the_opaque_zone_does_not() {
        for (reason, exit) in [
            (Reason::NotAttached, 8),
            (Reason::Paused, 8),
            (Reason::Validating, 8),
            (Reason::AccountMismatch, 8),
            (Reason::BridgeUnavailable, 8),
            (Reason::StaleGeneration, 8),
            (Reason::ZoneOpaque, 0),
        ] {
            assert_eq!(
                u8::from(reason.is_refusal()) * 8,
                exit,
                "{reason} must exit {exit}"
            );
        }
    }

    #[test]
    fn every_reason_has_a_line_a_person_can_act_on() {
        for reason in [
            Reason::NotAttached,
            Reason::Paused,
            Reason::Validating,
            Reason::AccountMismatch,
            Reason::BridgeUnavailable,
            Reason::StaleGeneration,
            Reason::ZoneOpaque,
            Reason::Protocol,
            Reason::NoteTooLarge,
            Reason::SourceRefRejected,
            Reason::NoteRejected,
            Reason::OriginMismatch,
            Reason::NavigationTimeout,
        ] {
            let message = message_for(reason);
            assert!(!message.is_empty(), "{reason}");
            assert!(!message.contains("None"), "{reason}: {message}");
        }
    }

    #[test]
    fn the_human_form_names_the_state_and_never_invents_content() {
        let result = HereResult {
            attachment: None,
            state: "not_attached".to_owned(),
            consumer: None,
            identity: HereIdentityJson {
                key: "school.test-1-abcd1234".to_owned(),
                generation: "g".to_owned(),
            },
            api: HereApiJson::default(),
            browser: None,
            reason: Some("bridge_unavailable".to_owned()),
        };
        assert_eq!(human(&result), "not attached (bridge_unavailable)");
    }
}

//! What the side panel is shown, and the one decision it can send back.
//!
//! The panel is an extension-owned surface (REPORT §3.4). It opens no
//! database, makes no request, and has no model behind it: everything it
//! displays is computed here, by the host, and pushed over native messaging.
//!
//! Two rules run through this file:
//!
//! - **Nothing is shown as done that the journal does not say is done.** A
//!   journal state travels as its exact SPEC §12.2 name and the panel prints
//!   that name; `matched` and `outcome_unknown` reach the person as what they
//!   are, not as a submission.
//! - **A decision is checked here, never trusted.** The panel sends back the
//!   handle it was shown; this file checks the handle, the digest, the
//!   identity generation, and the consumer before `plan::approve` is called at
//!   all, and `plan::approve` checks them again inside its own transaction.
//!   `bridge-ipc@1` has no approval message, so no socket client — and no
//!   page script that could reach one — has this path available.

use canvas_core::bridge::wire::{PanelApi, PanelJournal, PanelPlan, PanelPlanFile, PanelState};
use canvas_core::events::{CursorCheck, check_cursor, high_water};
use canvas_core::plan::{ApprovalChannel, PlanState};
use canvas_core::receipts::{ListFilter, list_journals};

use crate::session::Session;

/// How much of a frozen text payload the panel shows.
///
/// Enough for a person to recognize the work, and never the whole payload:
/// the digest is what the approval binds, and §12.2 step 8 verifies the bytes
/// from the stream.
const TEXT_PREVIEW_CHARS: usize = 512;

/// The journals the panel lists for one page.
const MAX_JOURNALS: usize = 20;

/// What the person decided in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Decline,
    Cancel,
}

impl Decision {
    /// The wire spelling the extension sends.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "approve" => Some(Self::Approve),
            "decline" => Some(Self::Decline),
            "cancel" => Some(Self::Cancel),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

/// Why a decision from the panel was not applied.
///
/// Each of these is a refusal that changes nothing. They are named separately
/// so the host's stderr can say which check failed, and they never say
/// whether some other handle would have worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionRefusal {
    /// The decision word was not one of the three.
    UnknownDecision,
    /// No plan with that id.
    UnknownPlan,
    /// The plan is not waiting for a decision.
    NotPrepared,
    /// The handle is not one issued for this plan, or it is already spent.
    BadHandle,
    /// The digest the panel echoed is not this plan's.
    DigestMismatch,
    /// The plan belongs to another identity generation.
    StaleGeneration,
    /// The plan layer refused it.
    Refused,
}

impl DecisionRefusal {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownDecision => "unknown_decision",
            Self::UnknownPlan => "unknown_plan",
            Self::NotPrepared => "not_prepared",
            Self::BadHandle => "bad_handle",
            Self::DigestMismatch => "plan_digest_mismatch",
            Self::StaleGeneration => "stale_generation",
            Self::Refused => "refused",
        }
    }
}

/// Apply one decision the person made in the panel.
///
/// The order is deliberate: everything is checked before anything moves, and
/// the checks are the ones REPORT §3.5 names — handle, digest, identity
/// generation, consumer. A decision that fails any of them changes nothing at
/// all, which is what the forgery tests assert.
pub fn apply_decision(
    session: &Session,
    plan_id: &str,
    handle: &str,
    plan_sha256: &str,
    decision: Decision,
) -> Result<(), DecisionRefusal> {
    let store = &session.open.store;
    let now = crate::output::now_timestamp();

    // The handle must be one this host issued for this plan and still unspent.
    // Reading it from `awaiting_decision` rather than from the message is the
    // point: the panel's echo selects a row, it does not supply one.
    //
    // A plan can have more than one live handle — every `issue_handle` makes
    // another, and two consumers can each ask for a decision on the same
    // plan — so the row is selected by the handle, not by the plan id alone.
    // Selecting by id and then comparing would refuse the person's own
    // second row, whose handle is real and which the panel drew.
    let waiting = canvas_core::plan::awaiting_decision(store, now)
        .map_err(|_| DecisionRefusal::Refused)?
        .into_iter()
        .filter(|entry| entry.plan.plan_id == plan_id)
        .find(|entry| constant_time_eq(entry.handle.as_bytes(), handle.as_bytes()));
    let Some(entry) = waiting else {
        // Say which of the two it was, without saying anything about a
        // handle that was never shown to this panel.
        return Err(match canvas_core::plan::load(store, plan_id) {
            Ok(Some(plan)) if plan.state == PlanState::Prepared => DecisionRefusal::BadHandle,
            Ok(Some(_)) => DecisionRefusal::NotPrepared,
            _ => DecisionRefusal::UnknownPlan,
        });
    };
    if entry.plan.plan_sha256 != plan_sha256 {
        return Err(DecisionRefusal::DigestMismatch);
    }
    // The stored plan must still describe itself, and it must still belong to
    // the generation it was frozen under.
    if entry.plan.digest() != entry.plan.plan_sha256 {
        return Err(DecisionRefusal::DigestMismatch);
    }
    match canvas_core::plan::identity_generation(store) {
        Ok(generation) if generation == entry.plan.identity_generation => {}
        Ok(_) => return Err(DecisionRefusal::StaleGeneration),
        Err(_) => return Err(DecisionRefusal::Refused),
    }

    let outcome = match decision {
        // The consumer is the handle's, which is the one `approve` checks.
        Decision::Approve => canvas_core::plan::approve(
            store,
            plan_id,
            handle,
            ApprovalChannel::Panel,
            entry.consumer.as_deref(),
            now,
        )
        .map(|_| ()),
        Decision::Decline => canvas_core::plan::decline(store, plan_id).map(|_| ()),
        Decision::Cancel => canvas_core::plan::cancel(store, plan_id).map(|_| ()),
    };
    outcome.map_err(|_| DecisionRefusal::Refused)
}

/// Compare two secrets without leaking their common prefix through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The plans a person can still decide on, as the panel shows them.
///
/// Everything needed to recognize the work travels: the target, the file
/// names with sizes and hashes, a bounded text preview with its digests, and
/// the baseline. The outbound bytes and the local file paths do not.
pub fn approvals(session: &Session) -> Vec<PanelPlan> {
    let now = crate::output::now_timestamp();
    canvas_core::plan::awaiting_decision(&session.open.store, now)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let plan = entry.plan;
            PanelPlan {
                plan_id: plan.plan_id.clone(),
                handle: entry.handle,
                plan_sha256: plan.plan_sha256.clone(),
                consumer: entry.consumer,
                course_id: plan.course_id.to_string(),
                assignment_id: plan.assignment_id.to_string(),
                assignment_name: plan.payload.assignment_name.clone(),
                kind: plan.kind.as_str().to_owned(),
                files: plan
                    .payload
                    .files
                    .iter()
                    .map(|file| PanelPlanFile {
                        name: file.name.clone(),
                        bytes: file.size,
                        sha256: file.sha256.clone(),
                    })
                    .collect(),
                text_preview: plan
                    .payload
                    .text
                    .as_ref()
                    .map(|text| preview(&text.outbound_bytes)),
                input_sha256: plan.input_sha256.clone(),
                sent_sha256: plan.sent_sha256.clone(),
                url: plan.payload.url.clone(),
                comment: plan.payload.comment.as_ref().map(|c| preview(c)),
                baseline_attempt: plan.baseline_attempt,
                baseline_submission_id: plan.baseline_submission_id.map(|id| id.to_string()),
                expires_at: plan.expires_at.clone(),
            }
        })
        .collect()
}

/// The first characters of a frozen payload, on a character boundary.
fn preview(value: &str) -> String {
    if value.chars().count() <= TEXT_PREVIEW_CHARS {
        return value.to_owned();
    }
    let mut out: String = value.chars().take(TEXT_PREVIEW_CHARS).collect();
    out.push('…');
    out
}

/// The journals for the course and assignment the person is looking at.
///
/// With no course in view the list is every recent journal, because a person
/// on the dashboard still wants to know what is outstanding. `state` is the
/// SPEC §12.2 name and nothing else: no mapping, no friendlier word.
pub fn journals(session: &Session, course_id: Option<&str>) -> Vec<PanelJournal> {
    let filter = ListFilter {
        course_id: course_id.and_then(|id| id.parse::<i64>().ok()),
        state: None,
    };
    list_journals(&session.open.store, &session.paths.identity_dir, &filter)
        .unwrap_or_default()
        .into_iter()
        .take(MAX_JOURNALS)
        .map(|row| PanelJournal {
            journal_id: row.journal_id,
            state: row.state,
            course_id: row.course_id,
            assignment_id: row.assignment_id,
            assignment_name: row.assignment_name,
            kind: row.kind,
            updated_at: row.updated_at,
            receipt_id: row.receipt_id,
            superseded: row.superseded,
            acknowledged: row.acknowledged_at.is_some(),
        })
        .collect()
}

/// Where the event log stands, and whether the panel's position still works.
///
/// The panel is one more consumer of the M6-c log and follows its cursor
/// rules: a position the log can no longer replay is not quietly restarted,
/// it is reported so the panel can say "refresh" and start again.
pub fn cursor(session: &Session, since: i64) -> (i64, bool) {
    let generation = session.identity.generation.to_string();
    session
        .open
        .store
        .call_blocking(move |conns| {
            let mark = high_water(&conns.state)?;
            let check = check_cursor(&conns.state, since, &generation)?;
            Ok((mark, check == CursorCheck::Resync))
        })
        .unwrap_or((since, false))
}

/// The API side of the panel, read from the local cache and nowhere else.
///
/// Each handler produces its own §7 envelope with its own freshness, exactly
/// as `here@1` carries them, so a row the cache has not refreshed is shown as
/// stale rather than refreshed behind the person's back.
///
/// `offline` is not a preference here. The panel is drawn whenever the log
/// moves or the person opens it, and a surface that fetched on every redraw
/// would make the browser the reason Canvas is called (REPORT §3.2).
pub async fn api(
    globals: &crate::commands::Globals,
    browser: Option<&canvas_core::bridge::ipc::Context>,
) -> PanelApi {
    let mut api = PanelApi::default();
    let Some(course_id) = browser.and_then(|c| c.course_id.clone()) else {
        return api;
    };
    let local = crate::commands::Globals {
        json: true,
        offline: true,
        fresh: false,
        quiet: true,
        ..globals.clone()
    };
    api.course = Some(
        crate::commands::course::handle(&local, course_id.clone())
            .await
            .envelope()
            .to_value(),
    );
    if let Some(assignment_id) = browser.and_then(|c| c.assignment_id.clone()) {
        api.assignment = Some(
            crate::commands::assignment::handle(&local, course_id, Some(assignment_id))
                .await
                .envelope()
                .to_value(),
        );
    } else if let Some(topic_id) = browser.and_then(|c| c.topic_id.clone()) {
        api.announcement = Some(
            crate::commands::announcement::handle(&local, course_id, Some(topic_id))
                .await
                .envelope()
                .to_value(),
        );
    }
    api
}

/// Assemble everything the panel displays.
pub fn state(
    session: &Session,
    browser: Option<&canvas_core::bridge::ipc::Context>,
    since: i64,
) -> PanelState {
    let (mark, resync_required) = cursor(session, since);
    let course_id = browser.and_then(|c| c.course_id.clone());
    PanelState {
        attachment_state: browser
            .map_or("not_attached", |c| c.state.as_str())
            .to_owned(),
        consumers: browser.map(|c| c.consumers.clone()).unwrap_or_default(),
        origin: browser.map_or_else(|| session.identity.origin.clone(), |c| c.origin.clone()),
        zone: browser.map(|c| c.zone),
        route: canvas_core::bridge::wire::Route {
            kind: browser.and_then(|c| c.page_kind),
            course_id: course_id.clone(),
            assignment_id: browser.and_then(|c| c.assignment_id.clone()),
            topic_id: browser.and_then(|c| c.topic_id.clone()),
            quiz_id: browser.and_then(|c| c.quiz_id.clone()),
            page_url: browser.and_then(|c| c.page_url.clone()),
        },
        // Filled by `api()`, which is async and runs off this thread.
        api: PanelApi::default(),
        url: browser.and_then(|c| c.url.clone()),
        title: browser.and_then(|c| c.title.clone()),
        observed_at: browser.map(|c| c.observed_at.clone()),
        ttl_ms: canvas_core::bridge::state::BROWSER_TTL_MS,
        journals: journals(session, course_id.as_deref()),
        approvals: approvals(session),
        notes: browser.map(|c| c.notes.clone()).unwrap_or_default(),
        follow: browser.and_then(|c| c.follow.clone()),
        cursor: mark,
        resync_required,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_bounded_on_a_character_boundary() {
        let short = "héllo";
        assert_eq!(preview(short), short);
        let long = "é".repeat(TEXT_PREVIEW_CHARS + 10);
        let cut = preview(&long);
        assert_eq!(cut.chars().count(), TEXT_PREVIEW_CHARS + 1, "plus the mark");
        assert!(cut.ends_with('…'));
        assert!(std::str::from_utf8(cut.as_bytes()).is_ok());
    }

    #[test]
    fn only_the_three_decisions_parse() {
        for (raw, decision) in [
            ("approve", Decision::Approve),
            ("decline", Decision::Decline),
            ("cancel", Decision::Cancel),
        ] {
            assert_eq!(Decision::parse(raw), Some(decision));
            assert_eq!(decision.as_str(), raw);
        }
        for raw in ["", "APPROVE", "yes", "true", "submit", "approve "] {
            assert_eq!(Decision::parse(raw), None, "{raw}");
        }
    }

    #[test]
    fn comparing_a_handle_does_not_stop_at_the_first_difference() {
        assert!(constant_time_eq(b"abcd", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abce"));
        assert!(!constant_time_eq(b"abcd", b"abcde"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn every_refusal_has_a_name() {
        for refusal in [
            DecisionRefusal::UnknownDecision,
            DecisionRefusal::UnknownPlan,
            DecisionRefusal::NotPrepared,
            DecisionRefusal::BadHandle,
            DecisionRefusal::DigestMismatch,
            DecisionRefusal::StaleGeneration,
            DecisionRefusal::Refused,
        ] {
            assert!(!refusal.as_str().is_empty());
        }
    }
}

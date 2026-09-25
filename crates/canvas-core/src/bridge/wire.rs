//! What the companion and the native host say to each other.
//!
//! The extension classifies the route and the zone in its own isolated world
//! and sends the classification; the host derives the same route from the
//! sanitized path and refuses a message whose classification is more
//! permissive than the route allows. Neither side trusts the other: the
//! extension is the only side that can see the page, and the host is the only
//! side that knows the CLI identity.

use serde::{Deserialize, Serialize};

/// How much of a page the companion may expose.
///
/// `assessment`, `external`, and `unknown` expose nothing: no title, no URL
/// beyond the origin, no route ids, and no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    /// Ordinary coursework pages.
    Open,
    /// Graded coursework and graded discussions.
    Graded,
    /// A quiz or an active assessment.
    Assessment,
    /// An embedded external tool.
    External,
    /// Anything this release cannot classify.
    Unknown,
}

impl Zone {
    /// Whether any page content at all may leave the browser.
    #[must_use]
    pub fn is_opaque(self) -> bool {
        matches!(self, Self::Assessment | Self::External | Self::Unknown)
    }

    /// The stricter of two classifications.
    ///
    /// The route says what the URL is; the document says which frames are on
    /// it. A route that reads `open` under a quiz frame is an assessment.
    #[must_use]
    pub fn strictest(self, other: Self) -> Self {
        // The declaration order runs from most to least exposed, and the
        // opaque zones sort last, so the maximum is the stricter reading.
        self.max(other)
    }
}

/// What a Canvas route names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    Dashboard,
    Course,
    Assignment,
    Announcement,
    Discussion,
    Quiz,
    Grades,
    Modules,
    Files,
    Page,
    Calendar,
    /// A Canvas path this release does not classify.
    Other,
}

/// The ids a route carries. Every one of them is a hint until the API
/// confirms it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub kind: Option<PageKind>,
    pub course_id: Option<String>,
    pub assignment_id: Option<String>,
    pub topic_id: Option<String>,
    pub quiz_id: Option<String>,
    /// The `pages/<url>` slug, when the route names a wiki page.
    pub page_url: Option<String>,
}

impl Route {
    /// Everything a route may carry, erased.
    #[must_use]
    pub fn opaque() -> Self {
        Self::default()
    }

    /// Whether this route names nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Classify a Canvas URL path into a route.
///
/// Only the path takes part: a query string can carry a capability
/// (`verifier`, `Signature`, …) and never identifies a route.
#[must_use]
pub fn classify_route(path: &str) -> Route {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let numeric = |value: &str| value.bytes().all(|b| b.is_ascii_digit()) && !value.is_empty();
    let mut route = Route::default();
    match parts.as_slice() {
        [] | ["dashboard"] => route.kind = Some(PageKind::Dashboard),
        ["calendar"] => route.kind = Some(PageKind::Calendar),
        ["courses", id, rest @ ..] if numeric(id) => {
            route.course_id = Some((*id).to_owned());
            route.kind = Some(match rest {
                [] | ["assignments"] => PageKind::Course,
                ["assignments", assignment, ..] if numeric(assignment) => {
                    route.assignment_id = Some((*assignment).to_owned());
                    PageKind::Assignment
                }
                ["quizzes", ..] => {
                    if let Some(quiz) = rest.get(1).filter(|q| numeric(q)) {
                        route.quiz_id = Some((*quiz).to_owned());
                    }
                    PageKind::Quiz
                }
                ["announcements"] => PageKind::Announcement,
                ["discussion_topics", topic, ..] if numeric(topic) => {
                    route.topic_id = Some((*topic).to_owned());
                    // Canvas serves an announcement from the discussion route;
                    // only the API can tell the two apart, so the route says
                    // discussion and `announcement.get` decides.
                    PageKind::Discussion
                }
                ["pages", slug, ..] if !slug.is_empty() => {
                    route.page_url = Some((*slug).to_owned());
                    PageKind::Page
                }
                ["modules", ..] => PageKind::Modules,
                ["files", ..] => PageKind::Files,
                ["grades", ..] => PageKind::Grades,
                _ => PageKind::Other,
            });
        }
        ["files", ..] => route.kind = Some(PageKind::Files),
        _ => route.kind = Some(PageKind::Other),
    }
    route
}

/// The zone a route alone implies, before the document is looked at.
#[must_use]
pub fn zone_for_route(route: &Route) -> Zone {
    match route.kind {
        Some(PageKind::Quiz) => Zone::Assessment,
        Some(PageKind::Grades) => Zone::Graded,
        Some(
            PageKind::Dashboard
            | PageKind::Course
            | PageKind::Assignment
            | PageKind::Announcement
            | PageKind::Discussion
            | PageKind::Modules
            | PageKind::Files
            | PageKind::Page
            | PageKind::Calendar,
        ) => Zone::Open,
        Some(PageKind::Other) | None => Zone::Unknown,
    }
}

/// The account the extension probed, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The `id` of `GET /api/v1/users/self`, reduced to this one field
    /// inside the extension.
    pub user_id: String,
    pub observed_at: String,
}

/// What the extension observed about one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// The origin the browser reports for the tab.
    pub origin: String,
    pub tab_id: i64,
    /// Chrome's stable per-document id.
    pub document_id: String,
    pub frame_id: i64,
    /// Incremented by the extension on every committed navigation.
    pub navigation_generation: u64,
    pub route: Route,
    pub zone: Zone,
    /// Absent when the probe failed or was redirected: no text is released
    /// and no attachment is made without it.
    pub account: Option<Account>,
    /// The URL with every capability-bearing parameter removed. Absent in an
    /// opaque zone.
    pub url: Option<String>,
    /// Absent in an opaque zone.
    pub title: Option<String>,
    pub observed_at: String,
}

/// The page text the extension extracted, on request only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extract {
    pub selection: Option<String>,
    pub text: Option<String>,
    /// True when the 64 KiB bound cut the payload.
    pub truncated: bool,
}

/// Why the companion stopped sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseCause {
    /// The tab was hidden for longer than `bridge.pause_hidden_after`.
    Hidden,
    /// The document entered an assessment.
    Assessment,
    /// The tab was closed.
    TabClosed,
    /// The person detached from the panel or the toolbar action.
    UserDetached,
    /// The document navigated across origins: the `activeTab` grant is gone.
    CrossOrigin,
}

/// The protocol version both sides declare.
pub const NATIVE_PROTOCOL: &str = "bridge-native@1";

/// Extension → host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionMessage {
    /// The first message on a port. Chrome supplies the caller origin on the
    /// command line; this repeats what the extension believes it is, so a
    /// disagreement is visible.
    Hello {
        protocol: String,
        extension_id: String,
        /// Identifies one browser-profile instance for the port's lifetime.
        profile_instance: String,
    },
    /// The person invoked the companion on this document.
    Attach {
        observation: Box<Observation>,
        consumer: Option<String>,
    },
    /// A committed navigation, a visibility change, or a re-probe.
    Update { observation: Box<Observation> },
    /// The answer to [`HostMessage::RequestText`].
    Text {
        request_id: String,
        document_id: String,
        navigation_generation: u64,
        /// A fresh probe accompanies every extract.
        account: Option<Account>,
        zone: Zone,
        extract: Extract,
    },
    /// Sharing stopped.
    Pause { cause: PauseCause },
    /// The attachment is over; a new gesture is required.
    Detach { cause: PauseCause },
    /// The companion accepted a navigation, or refused it.
    ///
    /// This is the **dispatch acknowledgement**. It says the extension took
    /// the request, and nothing about whether the page loaded.
    NavigateAck {
        request_id: String,
        accepted: bool,
        /// Why it was not accepted, when it was not.
        #[serde(default)]
        reason: Option<String>,
    },
    /// What became of an accepted navigation, once the tab settled.
    NavigateOutcome {
        request_id: String,
        /// `loaded`, `failed`, or `unknown`.
        outcome: String,
    },
    /// A decision the person made in the panel.
    ///
    /// This is the only approval path in the whole design. It carries the
    /// same handle the panel was shown, and the host checks the handle, the
    /// digest, the identity generation, and the consumer before it believes
    /// any of it. A page script cannot reach this channel: it runs in the
    /// extension's own surface, and `bridge-ipc@1` has no approval message at
    /// all.
    Decision {
        plan_id: String,
        handle: String,
        plan_sha256: String,
        /// `approve`, `decline`, or `cancel`.
        decision: String,
    },
    /// The panel opened, or asked for the state again after a reload.
    PanelHello { protocol: String },
}

/// Host → extension.
///
/// Not `Eq`: [`PanelState`] carries whole §7 envelopes as JSON, and a JSON
/// document has no total equality to offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// The host accepted the port and named the identity it owns.
    Ready {
        protocol: String,
        identity_key: String,
        origin: String,
        /// `bridge.pause_hidden_after`, in milliseconds. The extension owns
        /// the timer, because only it can see the tab.
        pause_hidden_after_ms: u64,
    },
    /// The attachment exists.
    ///
    /// The opaque id stays in the host: the extension has no use for it, and
    /// a value the service worker never holds is one a page can never reach.
    Attached { state: String },
    /// A consumer asked for text; re-probe and extract.
    RequestText {
        request_id: String,
        document_id: String,
        navigation_generation: u64,
    },
    /// The host will not accept this document.
    Refused { reason: String },
    /// Drop the attachment and erase what was shared.
    Detach { reason: String },
    /// Navigate the attached tab to this URL, inside the granted origin.
    Navigate { request_id: String, url: String },
    /// One new note for the panel to display.
    ///
    /// The panel renders it as text through a Markdown subset that builds no
    /// HTML and no link to anywhere but the granted origin. It is inert: the
    /// panel has no model backend and a note decides nothing.
    Note {
        note: Box<crate::bridge::note::Note>,
    },
    /// Everything the panel shows about the attachment, the journals, the
    /// receipts, and the plans awaiting approval.
    Panel { state: Box<PanelState> },
}

/// The panel's whole view, as the host computes it.
///
/// The panel never reads Canvas and never opens a database. Every field here
/// comes from the host, which is the only side that holds the identity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PanelState {
    /// `attached`, `validating`, `paused`, or `not_attached`.
    pub attachment_state: String,
    /// The consumers that opted in.
    pub consumers: Vec<String>,
    /// The origin the attachment is bound to.
    pub origin: String,
    /// The zone of the current document.
    pub zone: Option<Zone>,
    /// The route the host derived, as API facts and nothing more.
    pub route: Route,
    /// The whole §7 envelopes for what the route resolves to, each with its
    /// own freshness, exactly as `here@1` carries them.
    ///
    /// They are read from the local cache and nothing else: the panel must
    /// never be the reason a Canvas request happens, and a stale row is shown
    /// as stale rather than refreshed behind the person's back. No page text
    /// is here — metadata only.
    pub api: PanelApi,
    /// The sanitized URL, absent in an opaque zone.
    pub url: Option<String>,
    /// The page title, absent in an opaque zone.
    pub title: Option<String>,
    /// When the observation was made, and how old the panel may treat it.
    pub observed_at: Option<String>,
    /// Always `0`: browser context is never cacheable.
    pub ttl_ms: u64,
    /// The journals for the current course and assignment, newest first.
    pub journals: Vec<PanelJournal>,
    /// The plans waiting for a decision in this panel.
    pub approvals: Vec<PanelPlan>,
    /// The notes held for this attachment, oldest first.
    pub notes: Vec<crate::bridge::note::Note>,
    /// The last navigation, with its load outcome as it stands.
    pub follow: Option<crate::bridge::ipc::FollowStatus>,
    /// The event-log position the panel has been shown.
    pub cursor: i64,
    /// The log could not be replayed from the panel's position: start again.
    pub resync_required: bool,
}

/// The API side of the panel: whole envelopes, never merged with the browser
/// observation beside them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PanelApi {
    pub course: Option<serde_json::Value>,
    pub assignment: Option<serde_json::Value>,
    pub announcement: Option<serde_json::Value>,
}

/// One journal row, exactly as SPEC §12.2 names it.
///
/// The panel prints `state` verbatim. It never maps a state to a friendlier
/// word, and it never shows anything as done that the journal does not say is
/// done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelJournal {
    pub journal_id: String,
    /// A SPEC §12.2 state name.
    pub state: String,
    /// Absent for an operation that names no course.
    pub course_id: Option<String>,
    /// Absent for an operation that is not a submission.
    pub assignment_id: Option<String>,
    pub assignment_name: Option<String>,
    pub kind: String,
    pub updated_at: String,
    /// A receipt exists for this journal.
    pub receipt_id: Option<String>,
    /// Another attempt superseded it.
    pub superseded: bool,
    /// The unknown outcome was accepted by a person.
    pub acknowledged: bool,
}

/// One frozen plan, as the panel shows it before a decision.
///
/// Everything a person needs to recognize the work is here, and nothing that
/// would let the panel reconstruct the outbound bytes: files carry a size and
/// a hash, text carries a bounded preview and its digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelPlan {
    pub plan_id: String,
    /// The server-issued handle this panel was shown. A decision must carry
    /// it back; echoing a digest is not a substitute.
    pub handle: String,
    pub plan_sha256: String,
    pub consumer: Option<String>,
    pub course_id: String,
    pub assignment_id: String,
    pub assignment_name: Option<String>,
    pub kind: String,
    pub files: Vec<PanelPlanFile>,
    /// The first bytes of the text, for recognition, never the whole payload.
    pub text_preview: Option<String>,
    pub input_sha256: Option<String>,
    pub sent_sha256: Option<String>,
    pub url: Option<String>,
    pub comment: Option<String>,
    pub baseline_attempt: i64,
    pub baseline_submission_id: Option<String>,
    pub expires_at: String,
}

/// One file of a frozen plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelPlanFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_classify_the_pages_the_companion_supports() {
        let route = classify_route("/courses/45679/assignments/98765");
        assert_eq!(route.kind, Some(PageKind::Assignment));
        assert_eq!(route.course_id.as_deref(), Some("45679"));
        assert_eq!(route.assignment_id.as_deref(), Some("98765"));

        let quiz = classify_route("/courses/45679/quizzes/321/take");
        assert_eq!(quiz.kind, Some(PageKind::Quiz));
        assert_eq!(quiz.quiz_id.as_deref(), Some("321"));
        assert_eq!(zone_for_route(&quiz), Zone::Assessment);

        let topic = classify_route("/courses/1/discussion_topics/7");
        assert_eq!(topic.kind, Some(PageKind::Discussion));
        assert_eq!(topic.topic_id.as_deref(), Some("7"));

        let page = classify_route("/courses/1/pages/week-one");
        assert_eq!(page.kind, Some(PageKind::Page));
        assert_eq!(page.page_url.as_deref(), Some("week-one"));

        assert_eq!(classify_route("/").kind, Some(PageKind::Dashboard));
        assert_eq!(
            zone_for_route(&classify_route("/courses/1/grades")),
            Zone::Graded
        );
        assert_eq!(
            zone_for_route(&classify_route("/some/unknown/tool")),
            Zone::Unknown
        );
    }

    /// A non-numeric id is not an id: it must never become an API operand.
    #[test]
    fn a_route_id_is_always_numeric() {
        let route = classify_route("/courses/not-a-number/assignments/7");
        assert_eq!(route.course_id, None);
        assert_eq!(route.assignment_id, None);
        assert_eq!(route.kind, Some(PageKind::Other));
    }

    #[test]
    fn the_stricter_zone_wins() {
        assert_eq!(Zone::Open.strictest(Zone::Assessment), Zone::Assessment);
        assert_eq!(Zone::Unknown.strictest(Zone::Open), Zone::Unknown);
        assert_eq!(Zone::Open.strictest(Zone::Graded), Zone::Graded);
        assert!(Zone::External.is_opaque());
        assert!(!Zone::Graded.is_opaque());
    }
}

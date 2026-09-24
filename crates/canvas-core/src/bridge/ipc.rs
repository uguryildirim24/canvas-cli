//! `bridge-ipc@1`: the broker protocol on the private endpoint.
//!
//! Newline-delimited JSON, one request per line and one response per line.
//! The transport is a Unix socket at mode `0600` inside a `0700` directory,
//! or a user-restricted named pipe on Windows: it carries no authentication
//! of its own, because the operating system's file permissions are the
//! boundary (the design note, "within Rolf's OS trust domain").
//!
//! Four operations are the surface the design note names, plus `release`, which
//! is how `identity remove` asks a live owner to let go (the design note,
//! "cooperative release").

use serde::{Deserialize, Serialize};

use crate::bridge::note::Note;
use crate::bridge::wire::{PageKind, Zone};

/// The protocol version both ends declare on every line.
pub const IPC_PROTOCOL: &str = "bridge-ipc@1";

/// The largest request line the broker reads, in bytes.
///
/// It is enforced before the line is assembled. Until M7-b a request carried
/// a consumer name and an attachment id and nothing else; a `note` carries up
/// to [`note::MAX_NOTE_BYTES`] of text and [`note::MAX_SOURCE_REFS`] refs, and
/// JSON escaping can double the text, so the line bound has to be the larger
/// of the two. Sharing one number would make a note at its own limit come
/// back as `protocol` rather than as the named refusal `note_too_large`, and
/// a person cannot act on `protocol`.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Why content is unavailable (the design note and §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No attachment exists for this identity, or none for this consumer.
    NotAttached,
    /// An attachment exists but sharing is suspended.
    Paused,
    /// A new document is being re-checked; old text is already erased.
    Validating,
    /// The document is in an assessment, external, or unknown zone.
    ZoneOpaque,
    /// The probed browser account is not this identity's user.
    AccountMismatch,
    /// No broker is running for this identity.
    BridgeUnavailable,
    /// The message named a document or navigation generation that is gone.
    StaleGeneration,
    /// The request was not `bridge-ipc@1`.
    Protocol,
    /// The note is larger than `note::MAX_NOTE_BYTES`.
    NoteTooLarge,
    /// A source ref is not the granted origin and not `canvas://`.
    SourceRefRejected,
    /// The note is empty, or carries more refs than one note may.
    NoteRejected,
    /// The target is outside the granted origin, so the tab is not sent there.
    OriginMismatch,
    /// The companion did not acknowledge the navigation in time.
    NavigationTimeout,
}

impl Reason {
    /// The wire spelling, for messages and for the `reason` field of a §7
    /// refusal.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAttached => "not_attached",
            Self::Paused => "paused",
            Self::Validating => "validating",
            Self::ZoneOpaque => "zone_opaque",
            Self::AccountMismatch => "account_mismatch",
            Self::BridgeUnavailable => "bridge_unavailable",
            Self::StaleGeneration => "stale_generation",
            Self::Protocol => "protocol",
            Self::NoteTooLarge => "note_too_large",
            Self::SourceRefRejected => "source_ref_rejected",
            Self::NoteRejected => "note_rejected",
            Self::OriginMismatch => "origin_mismatch",
            Self::NavigationTimeout => "navigation_timeout",
        }
    }

    /// Whether this reason is one of the the design note refusals that exit 8.
    ///
    /// `zone_opaque` is not: the attachment is healthy and the answer is a
    /// bundle with no page content in it.
    #[must_use]
    pub fn is_refusal(self) -> bool {
        matches!(
            self,
            Self::NotAttached
                | Self::Paused
                | Self::Validating
                | Self::AccountMismatch
                | Self::BridgeUnavailable
                | Self::StaleGeneration
                | Self::Protocol
                | Self::NoteTooLarge
                | Self::SourceRefRejected
                | Self::NoteRejected
                | Self::OriginMismatch
                | Self::NavigationTimeout
        )
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one attachment is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    /// A new document: account and zone are being re-checked.
    Validating,
    /// Live.
    Attached,
    /// Suspended; nothing is served until it resumes.
    Paused,
}

impl AttachmentState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validating => "validating",
            Self::Attached => "attached",
            Self::Paused => "paused",
        }
    }
}

/// One request line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Must equal [`IPC_PROTOCOL`].
    pub v: String,
    /// Correlates the response. Opaque to the broker.
    pub id: String,
    #[serde(flatten)]
    pub op: Op,
}

impl Request {
    /// Build a well-formed request.
    #[must_use]
    pub fn new(id: impl Into<String>, op: Op) -> Self {
        Self {
            v: IPC_PROTOCOL.to_owned(),
            id: id.into(),
            op,
        }
    }
}

/// The operations the broker answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    /// What attachments exist, with no capability and no page content.
    #[serde(rename = "attachments.list")]
    AttachmentsList,
    /// Opt one consumer in, and return the opaque attachment id.
    #[serde(rename = "attach")]
    Attach {
        consumer: String,
        #[serde(default)]
        attachment_id: Option<String>,
    },
    /// Read the browser side of the bundle.
    ///
    /// `attachment_id` is the capability. `consumer` names an opted-in
    /// consumer, which the adapter sets and a model cannot. With neither,
    /// only the sole attachment is served, and only to the CLI.
    #[serde(rename = "here")]
    Here {
        #[serde(default)]
        attachment_id: Option<String>,
        #[serde(default)]
        consumer: Option<String>,
        #[serde(default)]
        include_text: bool,
    },
    /// Drop the attachment, or one consumer's share of it.
    ///
    /// `attachment_id` follows the same rule as `here`: it is the capability,
    /// and its absence selects the sole attachment, which the design note permits
    /// the CLI and nothing else.
    ///
    /// `consumer` scopes the operation to the caller. A named consumer only
    /// gives up its own opt-in; the tab stays attached for the person and for
    /// every other consumer. Ending the attachment itself is `canvas bridge
    /// detach`, a human act.
    #[serde(rename = "detach")]
    Detach {
        #[serde(default)]
        attachment_id: Option<String>,
        #[serde(default)]
        consumer: Option<String>,
    },
    /// Hold one inert note for the attachment and show it in the panel.
    ///
    /// `generation` is the navigation generation the note was written
    /// against. A note for a page the person has already left is refused, so
    /// the panel never shows a note beside the wrong document.
    ///
    /// A note approves nothing. There is deliberately no approval operation
    /// on this protocol at all: the only path to `plan::approve` with channel
    /// `panel` runs from the extension over native messaging.
    #[serde(rename = "note")]
    Note {
        #[serde(default)]
        attachment_id: Option<String>,
        #[serde(default)]
        consumer: Option<String>,
        generation: u64,
        text: String,
        #[serde(default)]
        source_refs: Vec<String>,
    },
    /// Ask the companion to navigate the attached tab inside its origin.
    ///
    /// The caller resolved `url` through the ordinary `open` resolver, so it
    /// is already a canonical Canvas URL; the broker checks the granted
    /// origin again before it asks the browser for anything.
    #[serde(rename = "follow")]
    Follow {
        #[serde(default)]
        attachment_id: Option<String>,
        #[serde(default)]
        consumer: Option<String>,
        generation: u64,
        url: String,
    },
    /// Let go of the identity so `identity remove` can take the exclusive
    /// lock, then exit.
    #[serde(rename = "release")]
    Release { reason: String },
}

/// One response line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub v: String,
    pub id: String,
    #[serde(flatten)]
    pub body: Body,
}

impl Response {
    /// A successful answer.
    #[must_use]
    pub fn ok(id: impl Into<String>, body: Body) -> Self {
        Self {
            v: IPC_PROTOCOL.to_owned(),
            id: id.into(),
            body,
        }
    }

    /// A refusal with an explicit reason.
    #[must_use]
    pub fn refused(id: impl Into<String>, reason: Reason) -> Self {
        Self::ok(id, Body::Refused { reason })
    }
}

/// What a response carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Body {
    /// `attachments.list`.
    Attachments { attachments: Vec<AttachmentSummary> },
    /// `attach`: the consumer is opted in and holds the capability.
    Attached {
        attachment_id: String,
        state: AttachmentState,
    },
    /// `here`.
    Context(Box<Context>),
    /// `detach`.
    Detached { detached: bool },
    /// `note`: the note is held and the panel was told about it.
    Noted {
        note: Box<Note>,
        /// The attachment it belongs to.
        attachment_id: String,
        /// How many notes the panel now holds for that attachment.
        held: u64,
    },
    /// `follow`: the companion accepted the navigation, and the attachment
    /// whose tab moved.
    ///
    /// This is the **dispatch acknowledgement** and nothing more. Whether the
    /// page loaded is a later state, which arrives on the bundle's
    /// `follow` section (the design note, "report loaded/failed separately").
    Followed {
        follow: Box<FollowStatus>,
        attachment_id: String,
    },
    /// `release`.
    Released { released: bool },
    /// Any operation that could not be served.
    Refused { reason: Reason },
}

/// One attachment as a bystander may see it: no id, no page content.
///
/// `bridge status` prints this, so it names the state and the origin and
/// nothing that identifies the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSummary {
    pub state: AttachmentState,
    pub origin: String,
    pub account_user_id: String,
    pub zone: Zone,
    pub consumers: Vec<String>,
    pub attached_at: String,
    pub navigation_generation: u64,
}

/// The `browser` section of `ContextBundle@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub attachment_id: String,
    pub state: AttachmentState,
    pub consumers: Vec<String>,
    pub origin: String,
    /// The account the extension probed, verified against the CLI identity.
    pub account: VerifiedAccount,
    pub zone: Zone,
    pub page_kind: Option<PageKind>,
    pub course_id: Option<String>,
    pub assignment_id: Option<String>,
    pub topic_id: Option<String>,
    pub quiz_id: Option<String>,
    pub page_url: Option<String>,
    /// Absent in an opaque zone.
    pub url: Option<String>,
    /// Absent in an opaque zone.
    pub title: Option<String>,
    pub document_id: String,
    pub frame_id: i64,
    pub navigation_generation: u64,
    pub observed_at: String,
    /// How long this observation may be treated as current, in milliseconds.
    pub ttl_ms: u64,
    pub selection: Option<String>,
    pub text: Option<String>,
    pub selection_bytes: u64,
    pub text_bytes: u64,
    pub truncated: bool,
    /// Why `selection` and `text` are absent, when they are.
    pub content_reason: Option<Reason>,
    /// The last navigation this consumer's `context.follow` asked for, with
    /// the load outcome as it stands now. `null` when none was asked for.
    pub follow: Option<FollowStatus>,
    /// The notes held for this attachment, oldest first.
    pub notes: Vec<Note>,
}

/// How a navigation ended, once the browser knows.
///
/// Dispatch and load are different facts and this enum is only the second of
/// them: `Unknown` is the honest answer while the tab is still going, and it
/// stays the answer if the companion never says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadOutcome {
    /// The tab committed the navigation and finished loading.
    Loaded,
    /// The tab did not get there.
    Failed,
    /// Still going, or the companion never reported it.
    Unknown,
}

impl LoadOutcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

/// One `context.follow`, from dispatch to load.
///
/// `dispatched` says the companion acknowledged the navigation; `load` says
/// what became of it. They are separate fields because they are separate
/// facts, and the second one is not known when the first one is answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowStatus {
    /// Correlates the acknowledgement with the later load outcome.
    pub request_id: String,
    /// The canonical Canvas URL the tab was sent to.
    pub url: String,
    /// The companion accepted the navigation.
    pub dispatched: bool,
    /// When the acknowledgement arrived.
    pub dispatched_at: String,
    /// How long the acknowledgement took, in milliseconds.
    pub dispatch_ms: u64,
    /// The navigation generation the request was bound to.
    pub generation: u64,
    /// What became of the navigation.
    pub load: LoadOutcome,
    /// When the load outcome was reported, when it was.
    pub load_at: Option<String>,
}

/// A browser account that matched the CLI identity's user id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedAccount {
    pub user_id: String,
    pub observed_at: String,
}

/// Parse one request line, refusing anything that is not `bridge-ipc@1`.
pub fn parse_request(line: &str) -> Result<Request, Reason> {
    if line.len() > MAX_REQUEST_BYTES {
        return Err(Reason::Protocol);
    }
    let request: Request = serde_json::from_str(line).map_err(|_| Reason::Protocol)?;
    if request.v != IPC_PROTOCOL {
        return Err(Reason::Protocol);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_operations_round_trip() {
        for op in [
            Op::AttachmentsList,
            Op::Attach {
                consumer: "mcp:claude-code".to_owned(),
                attachment_id: None,
            },
            Op::Here {
                attachment_id: Some("0123456789abcdef0123456789abcdef".to_owned()),
                consumer: None,
                include_text: true,
            },
            Op::Detach {
                attachment_id: Some("0123456789abcdef0123456789abcdef".to_owned()),
                consumer: None,
            },
            Op::Detach {
                attachment_id: None,
                consumer: Some("mcp:claude-code".to_owned()),
            },
        ] {
            let line = serde_json::to_string(&Request::new("1", op.clone())).expect("encode");
            assert_eq!(parse_request(&line).expect("decode").op, op);
        }
    }

    #[test]
    fn the_operation_names_are_the_report_names() {
        let line = serde_json::to_string(&Request::new("1", Op::AttachmentsList)).unwrap();
        assert!(line.contains("\"op\":\"attachments.list\""), "{line}");
        assert!(line.contains("\"v\":\"bridge-ipc@1\""), "{line}");
    }

    #[test]
    fn a_line_that_is_not_this_protocol_is_refused() {
        for line in [
            "",
            "{}",
            "not json",
            r#"{"v":"bridge-ipc@2","id":"1","op":"attach","consumer":"x"}"#,
            r#"{"v":"bridge-ipc@1","id":"1","op":"exec","command":"rm"}"#,
        ] {
            assert_eq!(parse_request(line).unwrap_err(), Reason::Protocol, "{line}");
        }
    }

    #[test]
    fn an_oversize_line_is_refused_without_parsing() {
        let line = format!(
            r#"{{"v":"bridge-ipc@1","id":"1","op":"attach","consumer":"{}"}}"#,
            "x".repeat(MAX_REQUEST_BYTES)
        );
        assert_eq!(parse_request(&line).unwrap_err(), Reason::Protocol);
    }

    #[test]
    fn here_defaults_to_metadata_only() {
        let line = r#"{"v":"bridge-ipc@1","id":"7","op":"here"}"#;
        assert_eq!(
            parse_request(line).unwrap().op,
            Op::Here {
                attachment_id: None,
                consumer: None,
                include_text: false,
            }
        );
    }

    /// Only `zone_opaque` leaves the bundle a success (§3.2 exit mapping).
    #[test]
    fn the_report_refusals_are_the_ones_that_refuse() {
        for reason in [
            Reason::NotAttached,
            Reason::Paused,
            Reason::Validating,
            Reason::BridgeUnavailable,
            Reason::AccountMismatch,
            Reason::StaleGeneration,
        ] {
            assert!(reason.is_refusal(), "{reason}");
        }
        assert!(!Reason::ZoneOpaque.is_refusal());
    }

    #[test]
    fn a_refusal_serializes_with_its_reason() {
        let line = serde_json::to_string(&Response::refused("3", Reason::NotAttached)).unwrap();
        assert!(line.contains(r#""result":"refused""#), "{line}");
        assert!(line.contains(r#""reason":"not_attached""#), "{line}");
    }
}

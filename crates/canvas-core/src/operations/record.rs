//! The frozen operation, the journal row, and the allowlisted records (M8-b).
//!
//! Nothing here holds a body. A plan holds the outbound bytes because it has
//! to send them; a journal holds their digests, and a response record holds
//! the ids and timestamps Canvas answered with. §12.2's allowlist rule is the
//! same for an operation as for a submission: an id, a time, an author, a
//! digest — never the message, never a signed URL.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::journal::IntendedText;

/// Which operation a plan or a journal describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// `POST /courses/:cid/discussion_topics/:tid/entries` (or `…/entries/:eid/replies`).
    DiscussionReply,
    /// `POST /conversations`.
    InboxSend,
    /// `POST /conversations/:id/add_message`.
    InboxReply,
}

impl OperationKind {
    /// Stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiscussionReply => "discussion_reply",
            Self::InboxSend => "inbox_send",
            Self::InboxReply => "inbox_reply",
        }
    }

    /// The plan kind that admits this operation.
    #[must_use]
    pub const fn plan_kind(self) -> crate::plan::PlanKind {
        match self {
            Self::DiscussionReply => crate::plan::PlanKind::DiscussionReply,
            Self::InboxSend => crate::plan::PlanKind::InboxSend,
            Self::InboxReply => crate::plan::PlanKind::InboxReply,
        }
    }
}

impl fmt::Display for OperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OperationKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "discussion_reply" => Self::DiscussionReply,
            "inbox_send" => Self::InboxSend,
            "inbox_reply" => Self::InboxReply,
            _ => return Err(()),
        })
    }
}

/// Exactly where an operation will be written.
///
/// The target is part of the plan digest, so approving a plan approves this
/// thread and these recipients and no others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationTarget {
    /// One topic, and one entry inside it when the reply is threaded.
    DiscussionReply {
        /// Course the topic belongs to.
        course_id: i64,
        /// Topic id.
        topic_id: i64,
        /// The entry replied to, when `--to` named one.
        parent_entry_id: Option<i64>,
    },
    /// A new conversation to these recipient ids, in the order given.
    InboxSend {
        /// Canvas user ids, as strings, deduplicated and in the order given.
        recipients: Vec<String>,
    },
    /// One existing conversation.
    InboxReply {
        /// Conversation id.
        conversation_id: i64,
    },
}

impl OperationTarget {
    /// Which operation this target names.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::DiscussionReply { .. } => OperationKind::DiscussionReply,
            Self::InboxSend { .. } => OperationKind::InboxSend,
            Self::InboxReply { .. } => OperationKind::InboxReply,
        }
    }

    /// The course this target belongs to, when it belongs to one.
    #[must_use]
    pub const fn course_id(&self) -> Option<i64> {
        match self {
            Self::DiscussionReply { course_id, .. } => Some(*course_id),
            _ => None,
        }
    }

    /// The admission lock name for this target (the brief's Target lock column).
    ///
    /// A new conversation has no server-side identity yet, so its lock is named
    /// after the plan: two executes of the *same* plan serialize on it, and two
    /// different plans never block each other over a conversation that does not
    /// exist.
    #[must_use]
    pub fn admission_name(&self, plan_id: &str) -> String {
        match self {
            Self::DiscussionReply { topic_id, .. } => format!("topic-{topic_id}"),
            Self::InboxSend { .. } => format!("conversation-new-{}", sanitize(plan_id)),
            Self::InboxReply { conversation_id } => format!("conversation-{conversation_id}"),
        }
    }

    /// The event-log scope this target writes under.
    #[must_use]
    pub fn event_scope(&self) -> String {
        match self {
            Self::DiscussionReply { topic_id, .. } => format!("topic:{topic_id}"),
            Self::InboxSend { .. } => "conversation:new".to_owned(),
            Self::InboxReply { conversation_id } => format!("conversation:{conversation_id}"),
        }
    }

    /// The cache scopes a success invalidates (§10 mutation epochs).
    #[must_use]
    pub fn epoch_scopes(&self) -> Vec<String> {
        match self {
            Self::DiscussionReply {
                course_id,
                topic_id,
                ..
            } => vec![
                format!("discussion:topic:{topic_id}"),
                format!("discussion:topic:{topic_id}:replies"),
                format!("discussions:course:{course_id}"),
            ],
            Self::InboxSend { .. } => {
                vec!["inbox:*".to_owned(), "inbox_unread:*".to_owned()]
            }
            Self::InboxReply { conversation_id } => vec![
                format!("conversation:conversation:{conversation_id}"),
                "inbox:*".to_owned(),
                "inbox_unread:*".to_owned(),
            ],
        }
    }
}

/// Keep only the characters an admission lock name may hold.
fn sanitize(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect()
}

/// One local file frozen as an attachment.
///
/// The path travels so execute can re-read the bytes; the digest is what the
/// approval binds. Changed bytes invalidate the plan and nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationAttachment {
    /// File name as Canvas will store it.
    pub name: String,
    /// Size in bytes at prepare time.
    pub size: u64,
    /// SHA-256 of the bytes at prepare time.
    pub sha256: String,
    /// Absolute local path.
    pub path: String,
    /// Canvas file id, once the upload transport returns one.
    #[serde(default)]
    pub canvas_file_id: Option<String>,
}

/// Human labels observed at prepare, for the confirmation line only.
///
/// Nothing here is compared at execute and nothing here is authority: it is
/// what a person needs in order to recognize what they are approving.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationLabels {
    /// Course code of the topic's course.
    #[serde(default)]
    pub course_code: Option<String>,
    /// Topic title.
    #[serde(default)]
    pub topic_title: Option<String>,
    /// Subject of the conversation being replied to.
    #[serde(default)]
    pub conversation_subject: Option<String>,
    /// Recipient display names, in the order of the frozen recipient ids.
    #[serde(default)]
    pub recipients: Vec<String>,
}

/// The frozen half of an operation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationPlan {
    /// Where it goes.
    pub target: OperationTarget,
    /// The body, with the digest of the input and the digest of what is sent.
    pub body: IntendedText,
    /// Conversation subject, when the caller gave one.
    #[serde(default)]
    pub subject: Option<String>,
    /// Attachments, in the order they will be sent.
    #[serde(default)]
    pub attachments: Vec<OperationAttachment>,
    /// Labels for the confirmation line.
    #[serde(default)]
    pub labels: OperationLabels,
}

impl OperationPlan {
    /// Which operation this plan is.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.target.kind()
    }
}

/// Operation journal lifecycle (M8-b, the §12.2 discipline).
///
/// `posted` and `matched` are the two ways an operation ends as done, and they
/// carry different attribution. `refused` is the terminal for an operation that
/// was never sent, `failed` for one Canvas answered with a non-2xx, and
/// `outcome_unknown` for one whose outcome was never observed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpState {
    /// Row published; nothing sent. Attachments upload in this state.
    Planned,
    /// The `POST` is in flight or its outcome is not yet classified.
    Posting,
    /// A 2xx was observed.
    Posted,
    /// A later readback matched a digest, with no id link (unproven).
    Matched,
    /// The `POST` outcome was never observed. Only reconcile moves it.
    OutcomeUnknown,
    /// Nothing was sent. `not_posted_evidence` says how that is known.
    Refused,
    /// Canvas answered with a non-2xx.
    Failed,
}

impl OpState {
    /// Stored and wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Posting => "posting",
            Self::Posted => "posted",
            Self::Matched => "matched",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }

    /// Terminal states never report a live owner.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Posted | Self::Matched | Self::OutcomeUnknown | Self::Refused | Self::Failed
        )
    }

    /// Whether this state means the operation reached Canvas.
    #[must_use]
    pub const fn is_done(self) -> bool {
        matches!(self, Self::Posted | Self::Matched)
    }
}

impl fmt::Display for OpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OpState {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "planned" => Self::Planned,
            "posting" => Self::Posting,
            "posted" => Self::Posted,
            "matched" => Self::Matched,
            "outcome_unknown" => Self::OutcomeUnknown,
            "refused" => Self::Refused,
            "failed" => Self::Failed,
            _ => return Err(()),
        })
    }
}

/// How much this process can honestly claim about where the operation went.
///
/// The ladder is evidence, not confidence. `accepted` is the weakest rung that
/// still names an object: Canvas answered 2xx and gave an id. `observed` adds a
/// readback that shows that same id in the thread. `unproven` is a readback
/// that shows a message whose digest matches and whose author is this identity,
/// with no id to link it to a request this process made. `none` is everything
/// else, and it is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// Nothing links the operation to an object.
    None,
    /// A 2xx with an object id this process observed.
    Accepted,
    /// A later readback shows that id.
    Observed,
    /// A readback shows a matching digest, with no id link.
    Unproven,
}

impl Attribution {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Accepted => "accepted",
            Self::Observed => "observed",
            Self::Unproven => "unproven",
        }
    }
}

impl fmt::Display for Attribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Attribution {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "none" => Self::None,
            "accepted" => Self::Accepted,
            "observed" => Self::Observed,
            "unproven" => Self::Unproven,
            _ => return Err(()),
        })
    }
}

/// Why an operation is known not to have been posted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotPostedEvidence {
    /// Recovered from `planned` with the owner absent: the `POST` never ran.
    NeverSent,
    /// A person asserted it after a clean readback and the thirty-minute wait.
    Assumed,
}

impl NotPostedEvidence {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeverSent => "never_sent",
            Self::Assumed => "assumed",
        }
    }
}

impl FromStr for NotPostedEvidence {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "never_sent" => Self::NeverSent,
            "assumed" => Self::Assumed,
            _ => return Err(()),
        })
    }
}

/// The allowlisted record of what Canvas answered.
///
/// The body never travels; `body_sha256` is the digest of whatever body field
/// the response carried, which is what a later readback is compared against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseRecord {
    /// The created object's id, when the response named one.
    #[serde(default)]
    pub id: Option<String>,
    /// Conversation id, for both inbox operations.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Creation timestamp Canvas reported.
    #[serde(default)]
    pub created_at: Option<String>,
    /// The timestamp in the identity's zone.
    #[serde(default)]
    pub created_at_local: Option<String>,
    /// The author Canvas attributed the object to.
    #[serde(default)]
    pub user_id: Option<String>,
    /// SHA-256 of the body Canvas echoed back.
    #[serde(default)]
    pub body_sha256: Option<String>,
    /// Attachment ids Canvas recorded on the object.
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    /// SHA-256 of the raw HTTP response body.
    #[serde(default)]
    pub response_sha256: Option<String>,
}

/// What a readback of the thread shows about this operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationReadback {
    /// When the readback ran.
    pub read_at: String,
    /// The object id the readback found, when it found one.
    #[serde(default)]
    pub id: Option<String>,
    /// Its creation timestamp.
    #[serde(default)]
    pub created_at: Option<String>,
    /// The timestamp in the identity's zone.
    #[serde(default)]
    pub created_at_local: Option<String>,
    /// Its author.
    #[serde(default)]
    pub user_id: Option<String>,
    /// SHA-256 of the body the readback shows.
    #[serde(default)]
    pub body_sha256: Option<String>,
    /// Attachment ids on it.
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    /// How many objects of the thread the readback covered.
    pub scanned: u32,
    /// Whether the readback saw the whole thread.
    pub complete: bool,
}

/// The candidate a readback matched by digest, with no id link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerMatch {
    /// The candidate's id.
    pub id: String,
    /// Its creation timestamp.
    #[serde(default)]
    pub created_at: Option<String>,
    /// The timestamp in the identity's zone.
    #[serde(default)]
    pub created_at_local: Option<String>,
    /// Its author.
    #[serde(default)]
    pub user_id: Option<String>,
    /// The digest that matched.
    pub body_sha256: String,
}

/// The `operation` block of a `receipt@1` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationReceipt {
    /// Which operation.
    pub kind: String,
    /// Course id, when the target has one.
    #[serde(default)]
    pub course_id: Option<String>,
    /// Topic id, for a discussion reply.
    #[serde(default)]
    pub topic_id: Option<String>,
    /// Parent entry id, for a threaded discussion reply.
    #[serde(default)]
    pub parent_entry_id: Option<String>,
    /// Conversation id, for an inbox reply and for a send Canvas accepted.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Recipient ids, for a send.
    #[serde(default)]
    pub recipients: Vec<String>,
    /// Subject, for a send.
    #[serde(default)]
    pub subject: Option<String>,
    /// Terminal state.
    pub state: String,
    /// Digest of the raw input.
    pub input_sha256: String,
    /// Transform applied before sending (`plain` or `html`).
    pub transform: String,
    /// Digest of the bytes sent.
    pub sent_sha256: String,
    /// Digest of the body Canvas echoed, when it echoed one.
    #[serde(default)]
    pub server_body_sha256: Option<String>,
    /// Frozen attachments.
    #[serde(default)]
    pub attachments: Vec<OperationAttachment>,
    /// The allowlisted response record.
    #[serde(default)]
    pub posted: Option<ResponseRecord>,
    /// The readback, when one ran.
    #[serde(default)]
    pub readback: Option<OperationReadback>,
    /// The digest-only match, when reconcile found one.
    #[serde(default)]
    pub server_match: Option<ServerMatch>,
    /// How much this receipt can claim.
    pub attribution: String,
    /// Whether the outcome can be observed as delivery at all.
    ///
    /// Canvas accepts a conversation; it never reports that a person received
    /// it. This field is `not_observable` for both inbox operations, so no
    /// reader can turn an acceptance into delivered mail.
    pub delivery: String,
}

/// One operation journal row.
#[derive(Debug, Clone)]
pub struct OperationRow {
    /// Journal id.
    pub journal_id: String,
    /// Identity the journal belongs to.
    pub identity_key: String,
    /// The plan this journal was admitted from.
    pub plan_id: String,
    /// Which operation.
    pub kind: OperationKind,
    /// The frozen operation, exactly as the plan held it.
    pub intended: OperationPlan,
    /// The approval audit copied in at admission.
    pub approval: Option<crate::plan::Approval>,
    /// Lifecycle state.
    pub state: OpState,
    /// HTTP status of the `POST`, when one was observed.
    pub post_status: Option<i64>,
    /// How the response was classified.
    pub response_kind: Option<String>,
    /// Why the operation is known not to have been posted.
    pub not_posted_evidence: Option<NotPostedEvidence>,
    /// How much the journal can claim.
    pub attribution: Attribution,
    /// The allowlisted response record.
    pub response: Option<ResponseRecord>,
    /// The readback, when one ran.
    pub readback: Option<OperationReadback>,
    /// The digest-only match reconcile found.
    pub server_match: Option<ServerMatch>,
    /// The stored receipt document.
    pub receipt: Option<Value>,
    /// Canvas file ids of the uploaded attachments.
    pub uploaded_file_ids: Vec<String>,
    /// Redacted error text.
    pub error_text: Option<String>,
    /// Row creation time.
    pub created_at: String,
    /// When the `POST` was started.
    pub posting_started_at: Option<String>,
    /// The latest state timestamp.
    pub terminal_at: Option<String>,
    /// When an unknown outcome was acknowledged.
    pub acknowledged_at: Option<String>,
}

impl OperationRow {
    /// The receipt id of the stored receipt, when there is one.
    #[must_use]
    pub fn receipt_id(&self) -> Option<String> {
        self.receipt
            .as_ref()?
            .get("receipt_id")?
            .as_str()
            .map(str::to_owned)
    }

    /// Whether an acceptance of this operation can be observed as delivery.
    ///
    /// Never, for either inbox operation: Canvas accepts a conversation and
    /// says nothing about whether a person received it. A discussion reply is
    /// a public post, so a readback of the thread does observe it.
    #[must_use]
    pub const fn delivery(&self) -> &'static str {
        match self.kind {
            OperationKind::DiscussionReply => "observable",
            OperationKind::InboxSend | OperationKind::InboxReply => "not_observable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_kinds_and_attribution_round_trip_through_their_wire_names() {
        for state in [
            OpState::Planned,
            OpState::Posting,
            OpState::Posted,
            OpState::Matched,
            OpState::OutcomeUnknown,
            OpState::Refused,
            OpState::Failed,
        ] {
            assert_eq!(OpState::from_str(state.as_str()), Ok(state));
        }
        for kind in [
            OperationKind::DiscussionReply,
            OperationKind::InboxSend,
            OperationKind::InboxReply,
        ] {
            assert_eq!(OperationKind::from_str(kind.as_str()), Ok(kind));
            assert_eq!(kind.plan_kind().as_str(), kind.as_str());
        }
        for attribution in [
            Attribution::None,
            Attribution::Accepted,
            Attribution::Observed,
            Attribution::Unproven,
        ] {
            assert_eq!(Attribution::from_str(attribution.as_str()), Ok(attribution));
        }
        assert!(OpState::from_str("submitted").is_err());
    }

    #[test]
    fn every_target_names_its_own_admission_lock() {
        let topic = OperationTarget::DiscussionReply {
            course_id: 5,
            topic_id: 55,
            parent_entry_id: None,
        };
        assert_eq!(topic.admission_name("plan-1"), "topic-55");
        let reply = OperationTarget::InboxReply {
            conversation_id: 701,
        };
        assert_eq!(reply.admission_name("plan-1"), "conversation-701");
        let send = OperationTarget::InboxSend {
            recipients: vec!["31".into()],
        };
        // The plan id is the only name a conversation that does not exist has.
        assert_eq!(
            send.admission_name("ab/cd-1"),
            "conversation-new-abcd-1",
            "a lock name never holds a path separator"
        );
        assert_ne!(topic.admission_name("p"), reply.admission_name("p"));
    }

    #[test]
    fn a_conversation_acceptance_is_never_delivery() {
        let row = |kind| OperationRow {
            journal_id: "j".into(),
            identity_key: "k".into(),
            plan_id: "p".into(),
            kind,
            intended: OperationPlan {
                target: OperationTarget::InboxReply { conversation_id: 1 },
                body: IntendedText {
                    input_sha256: "a".into(),
                    transform: "plain".into(),
                    sent_sha256: "a".into(),
                    outbound_bytes: String::new(),
                },
                subject: None,
                attachments: Vec::new(),
                labels: OperationLabels::default(),
            },
            approval: None,
            state: OpState::Posted,
            post_status: Some(201),
            response_kind: None,
            not_posted_evidence: None,
            attribution: Attribution::Accepted,
            response: None,
            readback: None,
            server_match: None,
            receipt: None,
            uploaded_file_ids: Vec::new(),
            error_text: None,
            created_at: "2026-09-10T00:00:00Z".into(),
            posting_started_at: None,
            terminal_at: None,
            acknowledged_at: None,
        };
        assert_eq!(row(OperationKind::InboxSend).delivery(), "not_observable");
        assert_eq!(row(OperationKind::InboxReply).delivery(), "not_observable");
        assert_eq!(row(OperationKind::DiscussionReply).delivery(), "observable");
    }
}

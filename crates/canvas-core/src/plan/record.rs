//! Plan, observation, and approval records (REPORT §3.5).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::journal::IntendedPayload;
use crate::submit::InputKind;

/// Lifecycle of a plan. `executed` means journal-linked, not successful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    /// Frozen and stored; no approval yet.
    Prepared,
    /// A human approved this exact plan.
    Approved,
    /// Linked to a journal. Says nothing about the submission's outcome.
    Executed,
    /// Admission expired before it was executed.
    Expired,
    /// A meaningful fact changed, or the plan was declined or cancelled.
    Invalidated,
}

impl PlanState {
    /// Wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Approved => "approved",
            Self::Executed => "executed",
            Self::Expired => "expired",
            Self::Invalidated => "invalidated",
        }
    }

    /// Parse a stored value.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "prepared" => Some(Self::Prepared),
            "approved" => Some(Self::Approved),
            "executed" => Some(Self::Executed),
            "expired" => Some(Self::Expired),
            "invalidated" => Some(Self::Invalidated),
            _ => None,
        }
    }
}

impl std::fmt::Display for PlanState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a human decision arrived from.
///
/// `yes_flag` records an explicit CLI `--yes`. It never claims an interactive
/// decision, and the agent adapters never present it as one (REPORT §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalChannel {
    /// A terminal prompt answered by the person at the keyboard.
    Tty,
    /// An MCP host's keyed elicitation response.
    Elicitation,
    /// The companion's private approval panel.
    Panel,
    /// An explicit CLI `--yes`.
    YesFlag,
}

impl ApprovalChannel {
    /// Wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tty => "tty",
            Self::Elicitation => "elicitation",
            Self::Panel => "panel",
            Self::YesFlag => "yes-flag",
        }
    }

    /// Parse a stored value.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "tty" => Some(Self::Tty),
            "elicitation" => Some(Self::Elicitation),
            "panel" => Some(Self::Panel),
            "yes-flag" => Some(Self::YesFlag),
            _ => None,
        }
    }
}

/// The approval audit stored on the plan and copied into the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// How the decision arrived.
    pub channel: ApprovalChannel,
    /// RFC 3339 UTC timestamp of the decision.
    pub at: String,
    /// Which consumer asked, when one did.
    #[serde(default)]
    pub consumer: Option<String>,
    /// Digest of the exact plan that was approved.
    pub plan_sha256: String,
}

/// The facts execute compares against; a change means a fresh plan is needed.
///
/// These are the "meaningful eligibility and date observations" of REPORT §3.5
/// and the inputs to SPEC §12.2 pre-flight steps 3 and 4. `allowed_extensions`
/// is included because §3.5 names allowed extensions among the facts to
/// revalidate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observations {
    /// Canvas' own eligibility answer when it supplies one.
    #[serde(default)]
    pub can_submit: Option<bool>,
    /// `null` or `-1` mean unlimited.
    #[serde(default)]
    pub allowed_attempts: Option<i64>,
    /// Extra attempts granted to this student.
    #[serde(default)]
    pub extra_attempts: Option<i64>,
    /// Non-null means a group assignment, which stays refused.
    #[serde(default)]
    pub group_category_id: Option<i64>,
    /// Accepted submission kinds, sorted so ordering alone is not a change.
    #[serde(default)]
    pub submission_types: Vec<String>,
    /// Accepted upload extensions, sorted.
    #[serde(default)]
    pub allowed_extensions: Vec<String>,
    /// Whether Canvas reports the assignment locked for this user.
    #[serde(default)]
    pub locked_for_user: Option<bool>,
    /// Due date.
    #[serde(default)]
    pub due_at: Option<String>,
    /// Lock date.
    #[serde(default)]
    pub lock_at: Option<String>,
    /// Unlock date.
    #[serde(default)]
    pub unlock_at: Option<String>,
}

impl Observations {
    /// Read the facts execute compares from a fresh assignment.
    ///
    /// The two list fields are sorted, so a reordered Canvas response is not
    /// mistaken for a changed assignment.
    #[must_use]
    pub fn of(assignment: &canvas_api::models::Assignment) -> Self {
        let mut submission_types = assignment
            .submission_types
            .as_value()
            .cloned()
            .unwrap_or_default();
        submission_types.sort();
        let mut allowed_extensions = assignment
            .allowed_extensions
            .as_value()
            .cloned()
            .unwrap_or_default();
        allowed_extensions.sort();
        Self {
            can_submit: assignment.can_submit.as_value().copied(),
            allowed_attempts: assignment.allowed_attempts.as_value().copied(),
            extra_attempts: assignment.submission.as_ref().and_then(|s| s.extra_attempts),
            group_category_id: assignment.group_category_id,
            submission_types,
            allowed_extensions,
            locked_for_user: assignment.locked_for_user,
            due_at: assignment.due_at.as_value().map(ToString::to_string),
            lock_at: assignment.lock_at.as_value().map(ToString::to_string),
            unlock_at: assignment.unlock_at.as_value().map(ToString::to_string),
        }
    }

    /// Name the first field that differs, for the `invalidated` reason.
    ///
    /// One name is enough: any single change requires a fresh plan, and the
    /// message stays readable. The order is the order SPEC §12.2 checks in.
    #[must_use]
    pub fn first_difference(&self, fresh: &Self) -> Option<&'static str> {
        let checks: [(&'static str, bool); 10] = [
            ("group_category_id", self.group_category_id == fresh.group_category_id),
            ("submission_types", self.submission_types == fresh.submission_types),
            ("allowed_extensions", self.allowed_extensions == fresh.allowed_extensions),
            ("can_submit", self.can_submit == fresh.can_submit),
            ("locked_for_user", self.locked_for_user == fresh.locked_for_user),
            ("allowed_attempts", self.allowed_attempts == fresh.allowed_attempts),
            ("extra_attempts", self.extra_attempts == fresh.extra_attempts),
            ("unlock_at", self.unlock_at == fresh.unlock_at),
            ("lock_at", self.lock_at == fresh.lock_at),
            ("due_at", self.due_at == fresh.due_at),
        ];
        checks
            .into_iter()
            .find_map(|(name, same)| (!same).then_some(name))
    }
}

/// One plan row.
#[derive(Debug, Clone)]
pub struct PlanRow {
    /// Plan id.
    pub plan_id: String,
    /// Identity this plan belongs to.
    pub identity_key: String,
    /// Identity generation at prepare time.
    pub identity_generation: String,
    /// Consumer that asked for the plan, when one did.
    pub consumer: Option<String>,
    /// Course id.
    pub course_id: i64,
    /// Assignment id.
    pub assignment_id: i64,
    /// Submission kind.
    pub kind: InputKind,
    /// Frozen payload, including the outbound bytes.
    pub payload: IntendedPayload,
    /// Absolute paths of the frozen files, in payload order.
    pub file_paths: Vec<String>,
    /// Digest of the raw text input, when the kind carries text.
    pub input_sha256: Option<String>,
    /// Digest of the outbound bytes, when the kind carries text.
    pub sent_sha256: Option<String>,
    /// Baseline attempt at prepare time.
    pub baseline_attempt: i64,
    /// Baseline submission id at prepare time.
    pub baseline_submission_id: Option<i64>,
    /// Facts to compare at execute.
    pub observations: Observations,
    /// Digest of the canonical plan document.
    pub plan_sha256: String,
    /// Lifecycle state.
    pub state: PlanState,
    /// When the plan was frozen.
    pub created_at: String,
    /// `created_at` + [`super::EXPIRY`].
    pub expires_at: String,
    /// The approval audit, `null` before approval.
    pub approval: Option<Approval>,
    /// The journal this plan admitted, once executed.
    pub journal_id: Option<String>,
    /// Why the plan was invalidated.
    pub invalidated_reason: Option<String>,
}

impl PlanRow {
    /// True when `now` is at or after the admission deadline.
    ///
    /// Expiry gates first admission only. A status read of an executed plan
    /// never expires it (REPORT §3.5).
    #[must_use]
    pub fn is_expired(&self, now: jiff::Timestamp) -> bool {
        self.expires_at
            .parse::<jiff::Timestamp>()
            .is_ok_and(|deadline| now >= deadline)
    }

    /// Recompute the digest of this plan's canonical document.
    #[must_use]
    pub fn digest(&self) -> String {
        canonical_digest(&CanonicalPlan::of(self))
    }
}

/// What `plan_sha256` covers.
///
/// The document names the identity, the target, the exact payload by digest,
/// the baseline, the observations, and the admission window. It deliberately
/// leaves out the outbound bytes and the local file paths: the bytes are
/// pinned by `sent_sha256` and the files by their `sha256`, which §12.2 step 8
/// re-verifies against the streamed upload. Approving a digest therefore
/// approves exact content, not a path that could later name other bytes.
#[derive(Debug, Serialize)]
struct CanonicalPlan<'a> {
    identity_key: &'a str,
    identity_generation: &'a str,
    consumer: Option<&'a str>,
    course_id: i64,
    assignment_id: i64,
    kind: &'a str,
    files: Vec<CanonicalFile<'a>>,
    text: Option<CanonicalText<'a>>,
    url: Option<&'a str>,
    comment_sha256: Option<String>,
    baseline_attempt: i64,
    baseline_submission_id: Option<i64>,
    observations: &'a Observations,
    created_at: &'a str,
    expires_at: &'a str,
}

#[derive(Debug, Serialize)]
struct CanonicalFile<'a> {
    name: &'a str,
    size: u64,
    sha256: &'a str,
}

#[derive(Debug, Serialize)]
struct CanonicalText<'a> {
    input_sha256: &'a str,
    transform: &'a str,
    sent_sha256: &'a str,
}

impl<'a> CanonicalPlan<'a> {
    fn of(row: &'a PlanRow) -> Self {
        Self {
            identity_key: &row.identity_key,
            identity_generation: &row.identity_generation,
            consumer: row.consumer.as_deref(),
            course_id: row.course_id,
            assignment_id: row.assignment_id,
            kind: row.kind.plan_name(),
            files: row
                .payload
                .files
                .iter()
                .map(|f| CanonicalFile {
                    name: &f.name,
                    size: f.size,
                    sha256: &f.sha256,
                })
                .collect(),
            text: row.payload.text.as_ref().map(|t| CanonicalText {
                input_sha256: &t.input_sha256,
                transform: &t.transform,
                sent_sha256: &t.sent_sha256,
            }),
            url: row.payload.url.as_deref(),
            comment_sha256: row.payload.comment.as_deref().map(sha256_hex),
            baseline_attempt: row.baseline_attempt,
            baseline_submission_id: row.baseline_submission_id,
            observations: &row.observations,
            created_at: &row.created_at,
            expires_at: &row.expires_at,
        }
    }
}

/// SHA-256 of a string, lowercase hex.
#[must_use]
pub fn sha256_hex(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

/// Digest a value through its canonical JSON form.
///
/// Serializing to a [`serde_json::Value`] first puts every object key in
/// sorted order, so the digest does not depend on the order a struct happens
/// to declare its fields in.
fn canonical_digest<T: Serialize>(value: &T) -> String {
    let canonical = serde_json::to_value(value)
        .and_then(|v| serde_json::to_string(&v))
        .unwrap_or_default();
    sha256_hex(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{IntendedFile, IntendedText};

    fn row() -> PlanRow {
        PlanRow {
            plan_id: "plan-1".into(),
            identity_key: "canvas.example-7-abc".into(),
            identity_generation: "gen-1".into(),
            consumer: None,
            course_id: 1,
            assignment_id: 2,
            kind: InputKind::OnlineTextEntry,
            payload: IntendedPayload {
                text: Some(IntendedText {
                    input_sha256: "aaa".into(),
                    transform: "html".into(),
                    sent_sha256: "bbb".into(),
                    outbound_bytes: "<p>hello</p>".into(),
                }),
                ..IntendedPayload::default()
            },
            file_paths: Vec::new(),
            input_sha256: Some("aaa".into()),
            sent_sha256: Some("bbb".into()),
            baseline_attempt: 0,
            baseline_submission_id: None,
            observations: Observations {
                can_submit: Some(true),
                submission_types: vec!["online_text_entry".into()],
                ..Observations::default()
            },
            plan_sha256: String::new(),
            state: PlanState::Prepared,
            created_at: "2026-09-10T00:00:00Z".into(),
            expires_at: "2026-09-10T00:15:00Z".into(),
            approval: None,
            journal_id: None,
            invalidated_reason: None,
        }
    }

    #[test]
    fn the_digest_covers_the_payload_target_and_window() {
        let base = row().digest();
        assert_eq!(base.len(), 64);
        assert_eq!(row().digest(), base, "the same plan digests the same");

        // Every field a human would be approving changes the digest.
        for mutate in [
            (|r: &mut PlanRow| r.assignment_id = 3) as fn(&mut PlanRow),
            |r| r.course_id = 9,
            |r| r.identity_generation = "gen-2".into(),
            |r| r.baseline_attempt = 1,
            |r| r.expires_at = "2026-09-10T00:20:00Z".into(),
            |r| r.consumer = Some("mcp".into()),
            |r| r.payload.comment = Some("note".into()),
            |r| r.observations.due_at = Some("2026-10-01T00:00:00Z".into()),
            |r| {
                r.payload.text.as_mut().unwrap().sent_sha256 = "ccc".into();
            },
        ] {
            let mut changed = row();
            mutate(&mut changed);
            assert_ne!(changed.digest(), base);
        }
    }

    #[test]
    fn the_digest_ignores_the_outbound_bytes_and_the_local_paths() {
        // The bytes are pinned by `sent_sha256`, the files by their hash, and
        // §12.2 step 8 re-verifies both from the stream.
        let base = row().digest();
        let mut moved = row();
        moved.file_paths = vec!["/somewhere/else".into()];
        moved.payload.text.as_mut().unwrap().outbound_bytes = "<p>other</p>".into();
        assert_eq!(moved.digest(), base);
    }

    #[test]
    fn a_file_hash_change_changes_the_digest() {
        let mut files = row();
        files.kind = InputKind::OnlineUpload;
        files.payload.text = None;
        files.payload.files = vec![IntendedFile {
            name: "ps3.pdf".into(),
            size: 10,
            sha256: "aaa".into(),
            canvas_file_id: None,
        }];
        let base = files.digest();
        let mut swapped = files.clone();
        swapped.payload.files[0].sha256 = "bbb".into();
        assert_ne!(swapped.digest(), base);
        let mut renamed = files.clone();
        renamed.payload.files[0].name = "other.pdf".into();
        assert_ne!(renamed.digest(), base);
    }

    #[test]
    fn a_changed_observation_is_named() {
        let base = Observations {
            can_submit: Some(true),
            allowed_attempts: Some(3),
            submission_types: vec!["online_upload".into()],
            due_at: Some("2026-10-01T00:00:00Z".into()),
            ..Observations::default()
        };
        assert_eq!(base.first_difference(&base), None);
        let mut locked = base.clone();
        locked.can_submit = Some(false);
        assert_eq!(base.first_difference(&locked), Some("can_submit"));
        let mut later = base.clone();
        later.due_at = Some("2026-11-01T00:00:00Z".into());
        assert_eq!(base.first_difference(&later), Some("due_at"));
        let mut kinds = base.clone();
        kinds.submission_types = vec!["online_url".into()];
        assert_eq!(base.first_difference(&kinds), Some("submission_types"));
    }

    #[test]
    fn states_and_channels_round_trip_through_their_wire_names() {
        for state in [
            PlanState::Prepared,
            PlanState::Approved,
            PlanState::Executed,
            PlanState::Expired,
            PlanState::Invalidated,
        ] {
            assert_eq!(PlanState::parse(state.as_str()), Some(state));
        }
        for channel in [
            ApprovalChannel::Tty,
            ApprovalChannel::Elicitation,
            ApprovalChannel::Panel,
            ApprovalChannel::YesFlag,
        ] {
            assert_eq!(ApprovalChannel::parse(channel.as_str()), Some(channel));
        }
        assert_eq!(ApprovalChannel::YesFlag.as_str(), "yes-flag");
        assert_eq!(PlanState::parse("nonsense"), None);
    }
}

//! Baseline comparison (agent-UX design).
//!
//! Only a complete membership of the same scope is compared, and only the
//! allowlisted §12.2 fields are carried. A payload never holds a full message,
//! DOM text, a token, or a signed URL.

use serde_json::{Map, Value};

use super::kind::EventKind;

/// The allowlisted record of one entity, keyed by field name.
pub type Member = Map<String, Value>;

/// Every member of one complete scope, keyed by entity key.
pub type Members = std::collections::BTreeMap<String, Member>;

/// What a dataset contributes to the event log.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// `fetch_log.dataset`.
    pub dataset: &'static str,
    /// The cache table the entity rows live in.
    pub table: &'static str,
    /// Allowlisted columns of that table.
    pub columns: &'static [&'static str],
    /// Allowlisted top-level keys of that table's `data_json`.
    pub json_keys: &'static [&'static str],
    /// Emitted for an entity that joined the membership.
    pub added: EventKind,
    /// Emitted for an entity that left it, when the dataset has removals.
    pub removed: Option<EventKind>,
    /// Emitted when an allowlisted field of a member changed, for a dataset
    /// that compares fields at all.
    ///
    /// `None` turns field comparison off for the whole shape: `missing` and
    /// `announcements` report that something joined a list, and the design note
    /// names no kind for any change inside one of their rows, not even the
    /// `due_at` `missing` allowlists for its payload. A dataset that does
    /// compare fields names the kind its remaining field changes carry, and
    /// `due_at`, the score, and the grade keep their own kinds inside it.
    pub changed: Option<EventKind>,
}

/// Fields that carry their own kind, so `assignment.changed` never repeats them.
const DUE_FIELDS: &[&str] = &["due_at"];
const GRADE_FIELDS: &[&str] = &["score", "grade", "posted_at"];

/// The datasets this package observes.
///
/// `files`, `folders`, `modules`, `courses`, the grade datasets, and the M8-a
/// read datasets other than `inbox_unread` are not listed: the design note names
/// no kind for them, and the rule for an undefined case is to emit fewer
/// events, never to invent one.
pub const SHAPES: &[Shape] = &[
    Shape {
        dataset: "assignments",
        table: "assignments",
        columns: &[
            "name",
            "due_at",
            "points_possible",
            "submitted",
            "graded",
            "score",
            "missing",
            "workflow_state",
            "attempt",
        ],
        json_keys: &["grade", "posted_at"],
        added: EventKind::AssignmentAdded,
        removed: Some(EventKind::AssignmentRemoved),
        changed: Some(EventKind::AssignmentChanged),
    },
    Shape {
        dataset: "missing",
        table: "assignments",
        columns: &["name", "due_at", "points_possible"],
        json_keys: &[],
        added: EventKind::MissingNew,
        // An assignment that leaves the missing list was submitted or excused;
        // §3.6 names no kind for that, so none is emitted.
        removed: None,
        changed: None,
    },
    Shape {
        dataset: "announcements",
        table: "announcements",
        columns: &["title", "posted_at"],
        json_keys: &[],
        added: EventKind::AnnouncementNew,
        removed: None,
        changed: None,
    },
    Shape {
        // The unread count is one row (M8-a `inbox_unread` / `all`). The
        // payload is the count and nothing else: a conversation subject, a
        // participant, and a message body never reach the log.
        dataset: "inbox_unread",
        table: "conversation_unread",
        columns: &["unread_count"],
        json_keys: &[],
        // `added` is unreachable here, and is named for completeness. The
        // first complete observation is silent, and `report_gap` deletes the
        // baseline rather than emptying it, so the observation after a gap is
        // silent too; the refresh always writes the single row, so no complete
        // observation ever compares against a baseline that lacks it. It names
        // the change kind rather than a second one, because a count that
        // became known again is the same news to a consumer as a count that
        // changed.
        added: EventKind::InboxUnreadCount,
        removed: None,
        changed: Some(EventKind::InboxUnreadCount),
    },
];

/// The shape for a dataset, when it produces events.
#[must_use]
pub fn shape_for(dataset: &str) -> Option<&'static Shape> {
    SHAPES.iter().find(|s| s.dataset == dataset)
}

/// One event the comparison produced, before it reaches the log.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEvent {
    pub kind: EventKind,
    pub entity_key: Option<String>,
    pub before: Value,
    pub after: Value,
}

/// Compare a new complete membership against the baseline.
///
/// The caller guarantees both sides are complete memberships of the same
/// scope: a partial or failed page never reaches here, so nothing it omits is
/// reported as a removal.
#[must_use]
pub fn diff(shape: &Shape, baseline: &Members, observed: &Members) -> Vec<PendingEvent> {
    let mut events = Vec::new();
    for (key, after) in observed {
        let Some(before) = baseline.get(key) else {
            events.push(PendingEvent {
                kind: shape.added,
                entity_key: Some(key.clone()),
                before: Value::Object(Map::new()),
                after: Value::Object(after.clone()),
            });
            continue;
        };
        if let Some(changed) = shape.changed {
            events.extend(field_events(changed, key, before, after));
        }
    }
    if let Some(removed) = shape.removed {
        for (key, before) in baseline {
            if !observed.contains_key(key) {
                events.push(PendingEvent {
                    kind: removed,
                    entity_key: Some(key.clone()),
                    before: Value::Object(before.clone()),
                    after: Value::Object(Map::new()),
                });
            }
        }
    }
    events.sort_by(|a, b| (&a.entity_key, a.kind).cmp(&(&b.entity_key, b.kind)));
    events
}

/// Field-level events for one entity present on both sides.
///
/// `changed` is the kind the fields that carry no kind of their own report
/// under. A shape that allowlists neither `due_at` nor a grade field — the
/// unread count is one — reaches only that last bucket.
fn field_events(
    changed_kind: EventKind,
    key: &str,
    before: &Member,
    after: &Member,
) -> Vec<PendingEvent> {
    let mut events = Vec::new();
    if changed(before, after, DUE_FIELDS) {
        events.push(subset(
            EventKind::DueChanged,
            key,
            before,
            after,
            DUE_FIELDS,
        ));
    }
    // `grade.posted` needs publication evidence. Without it a changed score is
    // `grade.changed`, and a published grade is reported once, not twice.
    let published = is_null(before.get("posted_at")) && !is_null(after.get("posted_at"));
    if published {
        events.push(subset(
            EventKind::GradePosted,
            key,
            before,
            after,
            GRADE_FIELDS,
        ));
    } else if changed(before, after, GRADE_FIELDS) {
        events.push(subset(
            EventKind::GradeChanged,
            key,
            before,
            after,
            GRADE_FIELDS,
        ));
    }
    let rest: Vec<&str> = after
        .keys()
        .chain(before.keys())
        .map(String::as_str)
        .filter(|f| !DUE_FIELDS.contains(f) && !GRADE_FIELDS.contains(f))
        .collect();
    if changed(before, after, &rest) {
        let mut names: Vec<&str> = rest
            .into_iter()
            .filter(|f| before.get(*f) != after.get(*f))
            .collect();
        names.sort_unstable();
        names.dedup();
        events.push(subset(changed_kind, key, before, after, &names));
    }
    events
}

fn changed(before: &Member, after: &Member, fields: &[&str]) -> bool {
    fields.iter().any(|f| before.get(*f) != after.get(*f))
}

fn is_null(value: Option<&Value>) -> bool {
    value.is_none_or(Value::is_null)
}

fn subset(
    kind: EventKind,
    key: &str,
    before: &Member,
    after: &Member,
    fields: &[&str],
) -> PendingEvent {
    let pick = |source: &Member| {
        let mut out = Map::new();
        for field in fields {
            out.insert(
                (*field).to_owned(),
                source.get(*field).cloned().unwrap_or(Value::Null),
            );
        }
        Value::Object(out)
    };
    PendingEvent {
        kind,
        entity_key: Some(key.to_owned()),
        before: pick(before),
        after: pick(after),
    }
}

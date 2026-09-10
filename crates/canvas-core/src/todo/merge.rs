//! Merge planner + missing sources into todo items (§12.1).

#![allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::collapsible_if,
    clippy::uninlined_format_args
)]

use std::collections::BTreeMap;

use jiff::{Timestamp, civil::Date};
use serde_json::Value;

use super::buckets::{AssignmentBucket, in_bucket};

/// Todo item kind after planner type mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoKind {
    Assignment,
    Quiz,
    Discussion,
    Checkpoint,
    PeerReview,
    Note,
    Event,
    Page,
    Announcement,
    Unknown,
}

impl TodoKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Assignment => "assignment",
            Self::Quiz => "quiz",
            Self::Discussion => "discussion",
            Self::Checkpoint => "checkpoint",
            Self::PeerReview => "peer_review",
            Self::Note => "note",
            Self::Event => "event",
            Self::Page => "page",
            Self::Announcement => "announcement",
            Self::Unknown => "unknown",
        }
    }
}

/// Map planner `plannable_type` to a kind + raw type.
#[must_use]
pub fn map_plannable_kind(raw: &str) -> (TodoKind, String) {
    let lower = raw.to_ascii_lowercase();
    let kind = match lower.as_str() {
        "assignment" => TodoKind::Assignment,
        "quiz" => TodoKind::Quiz,
        "discussion_topic" => TodoKind::Discussion,
        "sub_assignment" => TodoKind::Checkpoint,
        "peer_review_sub_assignment" | "assessment_request" => TodoKind::PeerReview,
        "planner_note" => TodoKind::Note,
        "calendar_event" => TodoKind::Event,
        "wiki_page" => TodoKind::Page,
        "announcement" => TodoKind::Announcement,
        _ => TodoKind::Unknown,
    };
    (kind, raw.to_owned())
}

/// Submission / availability status fields.
#[derive(Debug, Clone, Default)]
pub struct TodoStatus {
    pub submitted: Option<bool>,
    pub graded: Option<bool>,
    pub score: Option<f64>,
    pub late: Option<bool>,
    pub missing: bool,
    pub excused: Option<bool>,
    pub locked: Option<bool>,
    pub pending: bool,
}

/// Availability / submittable fields.
#[derive(Debug, Clone, Default)]
pub struct TodoAvailability {
    pub submittable: Option<bool>,
    pub external: Option<bool>,
    pub unlock_at: Option<Timestamp>,
    pub lock_at: Option<Timestamp>,
    pub lock_explanation: Option<String>,
}

/// One merged todo / assignment list item.
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub key: String,
    pub kind: TodoKind,
    pub raw_type: String,
    pub id: i64,
    pub assignment_id: Option<i64>,
    pub parent_assignment_id: Option<i64>,
    pub course_id: Option<i64>,
    pub course_code: Option<String>,
    pub title: String,
    pub due_at: Option<Timestamp>,
    pub scheduled_at: Option<Timestamp>,
    pub points_possible: Option<f64>,
    pub status: TodoStatus,
    pub availability: TodoAvailability,
    pub marked_complete: bool,
    pub dismissed: bool,
    pub html_url: Option<String>,
}

/// Window metadata for the todo envelope.
#[derive(Debug, Clone)]
pub struct TodoWindow {
    pub start: Date,
    pub end: Date,
    pub days: u32,
}

/// Aggregate counts.
#[derive(Debug, Clone, Default)]
pub struct TodoCounts {
    pub missing: u64,
    pub due_today: u64,
    pub due_week: u64,
    pub hidden: u64,
}

/// Filter flags for the merged list.
#[derive(Debug, Clone, Default)]
pub struct TodoFilters {
    pub all: bool,
    pub missing_only: bool,
    pub course_id: Option<i64>,
    pub bucket: Option<AssignmentBucket>,
    pub search: Option<String>,
}

/// Planner row as loaded from cache.
#[derive(Debug, Clone)]
pub struct PlannerSourceRow {
    pub id: String,
    pub plannable_id: Option<i64>,
    pub plannable_type: Option<String>,
    pub course_id: Option<i64>,
    pub title: Option<String>,
    pub data_json: String,
}

/// Missing-assignment row as loaded from cache.
#[derive(Debug, Clone)]
pub struct MissingSourceRow {
    pub id: i64,
    pub course_id: Option<i64>,
    pub name: Option<String>,
    pub due_at: Option<String>,
    pub points_possible: Option<f64>,
    pub html_url: Option<String>,
    pub submitted: Option<i64>,
    pub graded: Option<i64>,
    pub score: Option<f64>,
    pub late: Option<i64>,
    pub missing: Option<i64>,
    pub excused: Option<i64>,
    pub can_submit: Option<i64>,
    pub unlock_at: Option<String>,
    pub lock_at: Option<String>,
    pub data_json: String,
}

/// Build the merged todo list.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build_todo(
    planner: &[PlannerSourceRow],
    missing: &[MissingSourceRow],
    course_codes: &BTreeMap<i64, String>,
    pending: &BTreeMap<i64, bool>,
    now: Timestamp,
    today: Date,
    window: &TodoWindow,
    filters: &TodoFilters,
    ttl_assignments: jiff::Span,
    can_submit_observed: &BTreeMap<i64, Timestamp>,
) -> (Vec<TodoItem>, TodoCounts) {
    let mut by_key: BTreeMap<String, TodoItem> = BTreeMap::new();

    for row in planner {
        let Some(item) = planner_to_item(
            row,
            course_codes,
            pending,
            now,
            ttl_assignments,
            can_submit_observed,
        ) else {
            continue;
        };
        by_key.insert(item.key.clone(), item);
    }

    for row in missing {
        let item = missing_to_item(
            row,
            course_codes,
            pending,
            now,
            ttl_assignments,
            can_submit_observed,
        );
        match by_key.get_mut(&item.key) {
            Some(existing) => merge_missing_into(existing, &item),
            None => {
                by_key.insert(item.key.clone(), item);
            }
        }
    }

    let mut hidden = 0u64;
    let mut items: Vec<TodoItem> = by_key.into_values().collect();

    // Apply filters.
    items.retain(|item| {
        if let Some(cid) = filters.course_id {
            if item.course_id != Some(cid) {
                return false;
            }
        }
        if let Some(ref needle) = filters.search {
            let n = needle.to_lowercase();
            if !item.title.to_lowercase().contains(&n) {
                return false;
            }
        }
        if let Some(bucket) = filters.bucket {
            if !in_bucket(item, bucket, now) {
                return false;
            }
        }
        if filters.missing_only {
            return item.status.missing;
        }
        if filters.all {
            return true;
        }
        // Default hide rules.
        let submitted_and_graded =
            item.status.submitted.unwrap_or(false) && item.status.graded.unwrap_or(false);
        let hide = (item.marked_complete || item.dismissed || submitted_and_graded)
            && !item.status.missing;
        if hide {
            hidden += 1;
            return false;
        }
        true
    });

    items.sort_by(|a, b| {
        let a_day = a.scheduled_at.or(a.due_at);
        let b_day = b.scheduled_at.or(b.due_at);
        match (a_day, b_day) {
            (None, None) => {}
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (Some(_), None) => return std::cmp::Ordering::Less,
            (Some(x), Some(y)) => {
                let ord = x.cmp(&y);
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
        }
        a.course_code
            .cmp(&b.course_code)
            .then_with(|| a.id.cmp(&b.id))
    });

    let week_end = today
        .checked_add(jiff::Span::new().days(7))
        .unwrap_or(today);
    let mut counts = TodoCounts {
        hidden,
        ..TodoCounts::default()
    };
    for item in &items {
        if item.status.missing {
            counts.missing += 1;
        }
        if let Some(due) = item.scheduled_at.or(item.due_at) {
            let due_day = due.to_zoned(jiff::tz::TimeZone::UTC).date();
            if due_day == today {
                counts.due_today += 1;
            }
            if due_day >= today && due_day < week_end {
                counts.due_week += 1;
            }
        }
    }
    let _ = window;
    (items, counts)
}

fn planner_to_item(
    row: &PlannerSourceRow,
    course_codes: &BTreeMap<i64, String>,
    pending: &BTreeMap<i64, bool>,
    now: Timestamp,
    ttl_assignments: jiff::Span,
    can_submit_observed: &BTreeMap<i64, Timestamp>,
) -> Option<TodoItem> {
    let raw_type = row
        .plannable_type
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let (kind, raw_type) = map_plannable_kind(&raw_type);
    let plannable_id = row.plannable_id?;
    let data: Value = serde_json::from_str(&row.data_json).unwrap_or(Value::Null);
    let assignment_id = match kind {
        TodoKind::Assignment => Some(plannable_id),
        TodoKind::Quiz | TodoKind::Discussion => data
            .get("assignment_id")
            .and_then(Value::as_i64)
            .or_else(|| {
                data.get("assignment_id")
                    .and_then(|v| v.as_str()?.parse().ok())
            }),
        _ => data.get("assignment_id").and_then(Value::as_i64),
    };
    let parent_assignment_id = data.get("parent_assignment_id").and_then(Value::as_i64);
    let key = match kind {
        TodoKind::Assignment | TodoKind::Quiz | TodoKind::Discussion => {
            format!("assignment:{}", assignment_id.unwrap_or(plannable_id))
        }
        TodoKind::Checkpoint | TodoKind::PeerReview => {
            format!("{}:{plannable_id}", raw_type)
        }
        _ => format!("{}:{plannable_id}", raw_type),
    };
    let due_at = parse_opt_ts(data.get("due_at"));
    let scheduled_at = parse_opt_ts(data.get("plannable_date"))
        .or_else(|| parse_opt_ts(data.get("start_at")))
        .or_else(|| parse_opt_ts(data.get("todo_date")));
    let marked_complete = data
        .get("marked_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dismissed = data
        .get("dismissed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pending_flag = assignment_id
        .and_then(|id| pending.get(&id).copied())
        .unwrap_or(false);
    let mut status = TodoStatus {
        missing: false,
        pending: pending_flag,
        ..TodoStatus::default()
    };
    if let Some(subs) = data.get("submissions_json") {
        apply_submissions_blob(&mut status, subs);
    }
    let unlock_at = parse_opt_ts(data.get("unlock_at"));
    let lock_at = parse_opt_ts(data.get("lock_at"));
    let availability = TodoAvailability {
        submittable: submittable_for(
            assignment_id,
            None,
            None,
            unlock_at,
            lock_at,
            now,
            ttl_assignments,
            can_submit_observed,
        ),
        external: None,
        unlock_at,
        lock_at,
        lock_explanation: None,
    };
    Some(TodoItem {
        key,
        kind,
        raw_type,
        id: plannable_id,
        assignment_id,
        parent_assignment_id,
        course_id: row.course_id,
        course_code: row.course_id.and_then(|id| course_codes.get(&id).cloned()),
        title: row.title.clone().unwrap_or_default(),
        due_at,
        scheduled_at,
        points_possible: data.get("points_possible").and_then(Value::as_f64),
        status,
        availability,
        marked_complete,
        dismissed,
        html_url: data
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn missing_to_item(
    row: &MissingSourceRow,
    course_codes: &BTreeMap<i64, String>,
    pending: &BTreeMap<i64, bool>,
    now: Timestamp,
    ttl_assignments: jiff::Span,
    can_submit_observed: &BTreeMap<i64, Timestamp>,
) -> TodoItem {
    let key = format!("assignment:{}", row.id);
    let unlock_at = row.unlock_at.as_deref().and_then(|s| s.parse().ok());
    let lock_at = row.lock_at.as_deref().and_then(|s| s.parse().ok());
    let data: Value = serde_json::from_str(&row.data_json).unwrap_or(Value::Null);
    let locked = data.get("locked_for_user").and_then(Value::as_bool);
    TodoItem {
        key,
        kind: TodoKind::Assignment,
        raw_type: "assignment".into(),
        id: row.id,
        assignment_id: Some(row.id),
        parent_assignment_id: None,
        course_id: row.course_id,
        course_code: row.course_id.and_then(|id| course_codes.get(&id).cloned()),
        title: row.name.clone().unwrap_or_default(),
        due_at: row.due_at.as_deref().and_then(|s| s.parse().ok()),
        scheduled_at: None,
        points_possible: row.points_possible,
        status: TodoStatus {
            submitted: row.submitted.map(|v| v != 0),
            graded: row.graded.map(|v| v != 0),
            score: row.score,
            late: row.late.map(|v| v != 0),
            missing: true,
            excused: row.excused.map(|v| v != 0),
            locked,
            pending: pending.get(&row.id).copied().unwrap_or(false),
        },
        availability: TodoAvailability {
            submittable: submittable_for(
                Some(row.id),
                row.can_submit.map(|v| v != 0),
                locked,
                unlock_at,
                lock_at,
                now,
                ttl_assignments,
                can_submit_observed,
            ),
            external: None,
            unlock_at,
            lock_at,
            lock_explanation: data
                .get("lock_explanation")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        marked_complete: false,
        dismissed: false,
        html_url: row.html_url.clone(),
    }
}

fn merge_missing_into(existing: &mut TodoItem, missing: &TodoItem) {
    existing.status.missing = true;
    // Prefer planner override flags already on existing.
    if existing.title.is_empty() {
        existing.title.clone_from(&missing.title);
    }
    if existing.due_at.is_none() {
        existing.due_at = missing.due_at;
    }
    if existing.points_possible.is_none() {
        existing.points_possible = missing.points_possible;
    }
    if existing.html_url.is_none() {
        existing.html_url.clone_from(&missing.html_url);
    }
    if existing.status.submitted.is_none() {
        existing.status.submitted = missing.status.submitted;
    }
    if existing.status.graded.is_none() {
        existing.status.graded = missing.status.graded;
    }
    if existing.status.score.is_none() {
        existing.status.score = missing.status.score;
    }
    if existing.availability.submittable.is_none() {
        existing.availability.submittable = missing.availability.submittable;
    }
    existing.status.pending = existing.status.pending || missing.status.pending;
}

fn apply_submissions_blob(status: &mut TodoStatus, value: &Value) {
    if let Some(obj) = value.as_object() {
        if let Some(v) = obj.get("submitted").and_then(Value::as_bool) {
            status.submitted = Some(v);
        }
        if let Some(v) = obj.get("graded").and_then(Value::as_bool) {
            status.graded = Some(v);
        }
        if let Some(v) = obj.get("excused").and_then(Value::as_bool) {
            status.excused = Some(v);
        }
        if let Some(v) = obj.get("late").and_then(Value::as_bool) {
            status.late = Some(v);
        }
        if let Some(v) = obj.get("missing").and_then(Value::as_bool) {
            status.missing = v;
        }
    }
}

fn parse_opt_ts(value: Option<&Value>) -> Option<Timestamp> {
    value.and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
}

#[allow(clippy::too_many_arguments)]
fn submittable_for(
    assignment_id: Option<i64>,
    can_submit: Option<bool>,
    locked_for_user: Option<bool>,
    unlock_at: Option<Timestamp>,
    lock_at: Option<Timestamp>,
    now: Timestamp,
    ttl_assignments: jiff::Span,
    can_submit_observed: &BTreeMap<i64, Timestamp>,
) -> Option<bool> {
    if locked_for_user == Some(true) {
        return Some(false);
    }
    if lock_at.is_some_and(|t| t < now) {
        return Some(false);
    }
    if unlock_at.is_some_and(|t| t > now) {
        return Some(false);
    }
    let Some(aid) = assignment_id else {
        return can_submit;
    };
    let Some(observed) = can_submit_observed.get(&aid).copied() else {
        return can_submit;
    };
    let fresh = observed
        .checked_add(ttl_assignments)
        .is_ok_and(|expiry| now <= expiry);
    if !fresh {
        return None;
    }
    can_submit
}

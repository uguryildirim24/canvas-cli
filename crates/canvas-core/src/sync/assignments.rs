//! `assignments` dataset (`course:<id>`).

#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::match_same_arms,
    clippy::doc_markdown
)]

use canvas_api::Supplied as ApiSupplied;
use canvas_api::models::{Assignment, Submission};
use jiff::{Span, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{
    parse_opt_bool, push_api_f64, push_api_str, push_api_to_string, push_opt_bool, push_opt_i64,
    push_opt_str,
};

/// Default TTL: `ttl_assignments` = 30 minutes.
#[must_use]
pub fn default_ttl_assignments() -> Span {
    Span::new().minutes(30)
}

/// Assignments list for one course (`include[]=submission`).
#[derive(Debug, Clone)]
pub struct AssignmentsDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl AssignmentsDataset {
    #[must_use]
    pub fn new(course_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            ttl,
            scope_key: format!("course:{course_id}"),
        }
    }

    #[must_use]
    pub fn with_default_ttl(course_id: i64) -> Self {
        Self::new(course_id, default_ttl_assignments())
    }
}

impl Dataset for AssignmentsDataset {
    fn name(&self) -> &'static str {
        "assignments"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "assignment"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_assignment(tx, Some(self.course_id), entity, fetched_at)
    }
}

/// Path for the assignments list with submission include.
#[must_use]
pub fn assignments_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/assignments?include[]=submission&per_page=100")
}

/// Path for one assignment with `can_submit` and submission.
#[must_use]
pub fn assignment_detail_path(course_id: i64, assignment_id: i64) -> String {
    format!(
        "/api/v1/courses/{course_id}/assignments/{assignment_id}\
         ?include[]=submission&include[]=can_submit"
    )
}

/// Convert assignment list items into an ingest page.
#[must_use]
pub fn assignments_to_ingest_page(
    assignments: &[Assignment],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: assignments
            .iter()
            .map(|a| assignment_to_entity(a, Some(course_id)))
            .collect(),
    }
}

/// Convert one assignment (list or detail) into an entity ingest row.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn assignment_to_entity(assignment: &Assignment, course_hint: Option<i64>) -> EntityIngest {
    let mut fields = Vec::new();
    push_api_str(&mut fields, "name", FieldGroup::Core, &assignment.name);
    push_api_to_string(&mut fields, "due_at", FieldGroup::Core, &assignment.due_at);
    push_api_to_string(
        &mut fields,
        "unlock_at",
        FieldGroup::Core,
        &assignment.unlock_at,
    );
    push_api_to_string(
        &mut fields,
        "lock_at",
        FieldGroup::Core,
        &assignment.lock_at,
    );
    push_api_f64(
        &mut fields,
        "points_possible",
        FieldGroup::Core,
        &assignment.points_possible,
    );
    push_api_to_string(
        &mut fields,
        "html_url",
        FieldGroup::Core,
        &assignment.html_url,
    );
    push_api_str(
        &mut fields,
        "description",
        FieldGroup::Detail,
        &assignment.description,
    );
    push_submission_types(&mut fields, &assignment.submission_types);
    push_allowed_extensions(&mut fields, &assignment.allowed_extensions);
    push_api_to_string(
        &mut fields,
        "allowed_attempts",
        FieldGroup::Detail,
        &assignment.allowed_attempts,
    );
    push_rubric(&mut fields, &assignment.rubric);
    // `can_submit` is Detail and only written when the payload supplies it.
    match &assignment.can_submit {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name: "can_submit",
            group: FieldGroup::Detail,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name: "can_submit",
            group: FieldGroup::Detail,
            value: Some(if *v { "true" } else { "false" }.into()),
        }),
    }
    push_opt_bool(
        &mut fields,
        "locked_for_user",
        FieldGroup::Detail,
        assignment.locked_for_user,
    );
    push_opt_str(
        &mut fields,
        "lock_explanation",
        FieldGroup::Detail,
        assignment.lock_explanation.as_deref(),
    );
    let course_id = assignment
        .course_id
        .or(course_hint)
        .or_else(|| assignment.course.as_ref().map(|c| c.id));
    push_opt_i64(
        &mut fields,
        "group_category_id",
        FieldGroup::Detail,
        assignment.group_category_id,
    );
    if let Some(o) = &assignment.planner_override {
        push_opt_bool(
            &mut fields,
            "marked_complete",
            FieldGroup::Status,
            o.marked_complete,
        );
        push_opt_bool(&mut fields, "dismissed", FieldGroup::Status, o.dismissed);
    }
    push_opt_i64(&mut fields, "course_id", FieldGroup::Core, course_id);
    if let Some(sub) = assignment.submission.as_ref() {
        push_submission_status(&mut fields, sub);
    }

    EntityIngest {
        entity_key: assignment.id.to_string(),
        fields,
    }
}

fn push_submission_types(fields: &mut Vec<FieldWrite>, supplied: &ApiSupplied<Vec<String>>) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name: "submission_types",
            group: FieldGroup::Detail,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name: "submission_types",
            group: FieldGroup::Detail,
            value: Some(serde_json::to_string(v).unwrap_or_else(|_| "[]".into())),
        }),
    }
}

fn push_allowed_extensions(fields: &mut Vec<FieldWrite>, supplied: &ApiSupplied<Vec<String>>) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name: "allowed_extensions",
            group: FieldGroup::Detail,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name: "allowed_extensions",
            group: FieldGroup::Detail,
            value: Some(serde_json::to_string(v).unwrap_or_else(|_| "[]".into())),
        }),
    }
}

fn push_rubric(fields: &mut Vec<FieldWrite>, supplied: &ApiSupplied<Value>) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name: "rubric_json",
            group: FieldGroup::Detail,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name: "rubric_json",
            group: FieldGroup::Detail,
            value: Some(
                Value::Array(
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .map(criterion_json)
                        .collect(),
                )
                .to_string(),
            ),
        }),
    }
}

/// One rubric criterion, with the rating scale an agent needs to read a score.
///
/// M8-a adds `long_description`, `criterion_use_range`, and `ratings[]`; the
/// v1 keys keep their names and types.
fn criterion_json(c: &Value) -> Value {
    serde_json::json!({
        "id": opt_id(c.get("id")),
        "description": c.get("description").and_then(Value::as_str),
        "long_description": c.get("long_description").and_then(Value::as_str),
        "points": c.get("points").and_then(Value::as_f64),
        "criterion_use_range": c
            .get("criterion_use_range")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "ratings": Value::Array(
            c.get("ratings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|r| serde_json::json!({
                    "id": opt_id(r.get("id")),
                    "description": r.get("description").and_then(Value::as_str),
                    "long_description": r.get("long_description").and_then(Value::as_str),
                    "points": r.get("points").and_then(Value::as_f64),
                }))
                .collect(),
        ),
    })
}

/// Canvas sends a rubric id as a string or as a number.
fn opt_id(raw: Option<&Value>) -> Option<String> {
    raw.and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}

pub(super) fn push_submission_status(fields: &mut Vec<FieldWrite>, sub: &Submission) {
    match &sub.workflow_state {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => {
            for name in ["submitted", "graded", "workflow_state"] {
                fields.push(FieldWrite {
                    name,
                    group: FieldGroup::Status,
                    value: None,
                });
            }
        }
        ApiSupplied::Value(state) => {
            let submitted = state != "unsubmitted";
            let graded = state == "graded";
            fields.push(FieldWrite {
                name: "submitted",
                group: FieldGroup::Status,
                value: Some(if submitted { "true" } else { "false" }.into()),
            });
            fields.push(FieldWrite {
                name: "graded",
                group: FieldGroup::Status,
                value: Some(if graded { "true" } else { "false" }.into()),
            });
            fields.push(FieldWrite {
                name: "workflow_state",
                group: FieldGroup::Status,
                value: Some(state.clone()),
            });
        }
    }
    push_api_str(fields, "grade", FieldGroup::Status, &sub.grade);
    push_api_to_string(
        fields,
        "submitted_at",
        FieldGroup::Status,
        &sub.submitted_at,
    );
    push_opt_str(
        fields,
        "posted_at",
        FieldGroup::Status,
        sub.posted_at.map(|t| t.to_string()).as_deref(),
    );
    push_opt_i64(
        fields,
        "extra_attempts",
        FieldGroup::Detail,
        sub.extra_attempts,
    );
    push_api_f64(fields, "score", FieldGroup::Status, &sub.score);
    push_api_to_string(fields, "late", FieldGroup::Status, &sub.late);
    push_api_to_string(fields, "missing", FieldGroup::Status, &sub.missing);
    push_api_to_string(fields, "excused", FieldGroup::Status, &sub.excused);
    push_api_to_string(fields, "attempt", FieldGroup::Status, &sub.attempt);
}

const COLUMN_FIELDS: &[&str] = &[
    "course_id",
    "name",
    "due_at",
    "unlock_at",
    "lock_at",
    "points_possible",
    "html_url",
    "description",
    "submission_types",
    "allowed_extensions",
    "allowed_attempts",
    "rubric_json",
    "can_submit",
    "submitted",
    "graded",
    "score",
    "late",
    "missing",
    "excused",
    "workflow_state",
    "attempt",
];

const EXTRA_FIELDS: &[&str] = &[
    "locked_for_user",
    "lock_explanation",
    "group_category_id",
    "extra_attempts",
    "grade",
    "submitted_at",
    "posted_at",
    "external_tool_name",
    "marked_complete",
    "dismissed",
];

/// Upsert an assignment entity into `assignments` (+ field_obs).
pub fn upsert_assignment(
    tx: &Transaction<'_>,
    course_hint: Option<i64>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_assignment_id(&entity.entity_key)?;
    validate_fields(&entity.fields)?;
    let course_id = entity
        .fields
        .iter()
        .find(|f| f.name == "course_id")
        .and_then(|f| f.value.as_ref())
        .and_then(|v| v.parse().ok())
        .or(course_hint);
    tx.execute(
        "INSERT INTO assignments (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, course_id],
    )?;
    let applied = apply_field_writes(
        tx,
        "assignment",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    apply_assignment_columns(tx, id, &entity.fields, &applied.fields)?;
    merge_assignment_extra(tx, id, &entity.fields, &applied.fields)?;
    touch_assignment_observed(
        tx,
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

fn parse_assignment_id(entity_key: &str) -> Result<i64, IngestError> {
    let id: i64 = entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad assignment id: {e}"),
            ),
        )))
    })?;
    if entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(id)
}

fn validate_fields(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !(COLUMN_FIELDS.contains(&field.name) || EXTRA_FIELDS.contains(&field.name))
            || !seen.insert(field.name)
        {
            return Err(DbError::Message("unsupported or duplicate entity field".into()).into());
        }
    }
    Ok(())
}

fn apply_assignment_columns(
    tx: &Transaction<'_>,
    id: i64,
    fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in fields {
        if !applied.contains(&field.name) || !COLUMN_FIELDS.contains(&field.name) {
            continue;
        }
        match field.name {
            "course_id" | "name" | "due_at" | "unlock_at" | "lock_at" | "html_url"
            | "description" | "submission_types" | "allowed_extensions" | "rubric_json"
            | "workflow_state" => {
                tx.execute(
                    &format!("UPDATE assignments SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            "points_possible" | "score" => {
                let num: Option<f64> = match &field.value {
                    None => None,
                    Some(s) => Some(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                };
                tx.execute(
                    &format!("UPDATE assignments SET {} = ?1 WHERE id = ?2", field.name),
                    params![num, id],
                )?;
            }
            "allowed_attempts" | "attempt" => {
                let num: Option<i64> = match &field.value {
                    None => None,
                    Some(s) => Some(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                };
                tx.execute(
                    &format!("UPDATE assignments SET {} = ?1 WHERE id = ?2", field.name),
                    params![num, id],
                )?;
            }
            "can_submit" | "submitted" | "graded" | "late" | "missing" | "excused" => {
                let bit: Option<i64> = match parse_opt_bool(field.value.as_ref()) {
                    None if field.value.is_none() => None,
                    None => Some(0),
                    Some(true) => Some(1),
                    Some(false) => Some(0),
                };
                // Explicit null clears the column.
                let bit = if field.value.is_none() { None } else { bit };
                tx.execute(
                    &format!("UPDATE assignments SET {} = ?1 WHERE id = ?2", field.name),
                    params![bit, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_assignment_extra(
    tx: &Transaction<'_>,
    id: i64,
    fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    let mut data = load_assignment_data_json(tx, id)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("assignments data_json must be an object".into()))?;
    for field in fields {
        if !applied.contains(&field.name) || !EXTRA_FIELDS.contains(&field.name) {
            continue;
        }
        match &field.value {
            None => {
                obj.insert(field.name.into(), Value::Null);
            }
            Some(v)
                if matches!(
                    field.name,
                    "locked_for_user" | "marked_complete" | "dismissed"
                ) =>
            {
                obj.insert(field.name.into(), Value::Bool(v == "true" || v == "1"));
            }
            Some(v) => {
                obj.insert(field.name.into(), Value::String(v.clone()));
            }
        }
    }
    tx.execute(
        "UPDATE assignments SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(())
}

fn load_assignment_data_json(tx: &Transaction<'_>, id: i64) -> Result<Value, IngestError> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT data_json FROM assignments WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        None => Ok(Value::Object(Map::new())),
        Some(s) if s.is_empty() => Ok(Value::Object(Map::new())),
        Some(s) => serde_json::from_str(&s)
            .map_err(|_| DbError::Message("invalid assignments data_json".into()).into()),
    }
}

fn touch_assignment_observed(
    tx: &Transaction<'_>,
    id: i64,
    fetched_at: Timestamp,
    core: bool,
    detail: bool,
    status: bool,
) -> Result<(), IngestError> {
    let ts = fetched_at.to_string();
    if core {
        tx.execute(
            "UPDATE assignments SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            "UPDATE assignments SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            "UPDATE assignments SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

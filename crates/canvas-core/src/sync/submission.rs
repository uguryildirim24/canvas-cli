//! `submission` dataset (`assignment:<id>`).

#![allow(clippy::too_many_lines, clippy::match_same_arms)]

use canvas_api::Supplied as ApiSupplied;
use canvas_api::models::Submission;
use jiff::{Span, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::assignments::default_ttl_assignments;
use super::fields::{push_api_f64, push_api_str, push_api_to_string, push_opt_i64};

/// One submission object for an assignment (`submissions/self` with history).
#[derive(Debug, Clone)]
pub struct SubmissionDataset {
    pub assignment_id: i64,
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl SubmissionDataset {
    #[must_use]
    pub fn new(course_id: i64, assignment_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            assignment_id,
            ttl,
            scope_key: format!("assignment:{assignment_id}"),
        }
    }

    #[must_use]
    pub fn with_default_ttl(course_id: i64, assignment_id: i64) -> Self {
        Self::new(course_id, assignment_id, default_ttl_assignments())
    }
}

impl Dataset for SubmissionDataset {
    fn name(&self) -> &'static str {
        "submission"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "submission"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_submission(tx, entity, fetched_at)
    }
}

/// Path for the self submission with history/comments/rubric.
#[must_use]
pub fn submission_path(course_id: i64, assignment_id: i64) -> String {
    format!(
        "/api/v1/courses/{course_id}/assignments/{assignment_id}/submissions/self\
         ?include[]=submission_history&include[]=submission_comments&include[]=rubric_assessment"
    )
}

/// Convert one submission object into an ingest page (single entity).
#[must_use]
pub fn submission_to_ingest_page(submission: &Submission, fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: vec![submission_to_entity(submission)],
    }
}

/// Convert a submission into an entity ingest row.
#[must_use]
pub fn submission_to_entity(submission: &Submission) -> EntityIngest {
    let entity_key = submission
        .id
        .or_else(|| submission.assignment_id.and_then(i64::checked_neg))
        .unwrap_or(0)
        .to_string();
    let mut fields = Vec::new();
    push_opt_i64(
        &mut fields,
        "assignment_id",
        FieldGroup::Core,
        submission.assignment_id,
    );
    push_opt_i64(&mut fields, "user_id", FieldGroup::Core, submission.user_id);
    push_api_to_string(
        &mut fields,
        "attempt",
        FieldGroup::Status,
        &submission.attempt,
    );
    push_api_f64(&mut fields, "score", FieldGroup::Status, &submission.score);
    push_api_str(&mut fields, "grade", FieldGroup::Status, &submission.grade);
    push_api_to_string(
        &mut fields,
        "submitted_at",
        FieldGroup::Status,
        &submission.submitted_at,
    );
    push_api_str(
        &mut fields,
        "workflow_state",
        FieldGroup::Status,
        &submission.workflow_state,
    );
    push_api_to_string(&mut fields, "late", FieldGroup::Status, &submission.late);
    push_api_to_string(
        &mut fields,
        "missing",
        FieldGroup::Status,
        &submission.missing,
    );
    push_api_to_string(
        &mut fields,
        "excused",
        FieldGroup::Status,
        &submission.excused,
    );
    if let Some(ref history) = submission.submission_history {
        let rows: Vec<Value> = history
            .iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id.map(|id| id.to_string()),
                    "attachments": attachments_json(h.attachments.as_deref().unwrap_or_default()),
                    "attempt": match &h.attempt {
                        ApiSupplied::Value(v) => Value::from(*v),
                        ApiSupplied::Null => Value::Null,
                        ApiSupplied::Absent => Value::Null,
                    },
                    "submitted_at": match &h.submitted_at {
                        ApiSupplied::Value(v) => Value::String(v.to_string()),
                        ApiSupplied::Null => Value::Null,
                        ApiSupplied::Absent => Value::Null,
                    },
                    "workflow_state": match &h.workflow_state {
                        ApiSupplied::Value(v) => Value::String(v.clone()),
                        ApiSupplied::Null => Value::Null,
                        ApiSupplied::Absent => Value::Null,
                    },
                    "score": match &h.score {
                        ApiSupplied::Value(v) => Value::from(*v),
                        ApiSupplied::Null => Value::Null,
                        ApiSupplied::Absent => Value::Null,
                    },
                })
            })
            .collect();
        fields.push(FieldWrite {
            name: "submission_history_json",
            group: FieldGroup::Detail,
            value: Some(Value::Array(rows).to_string()),
        });
    }
    if let Some(ref comments) = submission.submission_comments {
        let rows: Vec<Value> = comments
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id.map(|id| id.to_string()),
                    "comment": c.comment,
                    "author_name": c.author_name,
                    "created_at": c.created_at.map(|t| t.to_string()),
                })
            })
            .collect();
        fields.push(FieldWrite {
            name: "submission_comments_json",
            group: FieldGroup::Detail,
            value: Some(Value::Array(rows).to_string()),
        });
    }
    if let Some(ref rubric) = submission.rubric_assessment {
        fields.push(FieldWrite {
            name: "rubric_assessment_json",
            group: FieldGroup::Detail,
            value: Some(rubric_json(rubric).to_string()),
        });
    }
    push_opt_i64(&mut fields, "id", FieldGroup::Core, submission.id);
    super::fields::push_opt_str(
        &mut fields,
        "submission_type",
        FieldGroup::Detail,
        submission.submission_type.as_deref(),
    );
    super::fields::push_opt_str(
        &mut fields,
        "url",
        FieldGroup::Detail,
        submission.url.as_ref().map(reqwest::Url::as_str),
    );
    super::fields::push_opt_str(
        &mut fields,
        "posted_at",
        FieldGroup::Status,
        submission.posted_at.map(|t| t.to_string()).as_deref(),
    );
    if let Some(body) = &submission.body {
        use sha2::{Digest, Sha256};
        fields.push(FieldWrite {
            name: "body_sha256",
            group: FieldGroup::Detail,
            value: Some(format!("{:x}", Sha256::digest(body.as_bytes()))),
        });
    }
    if let Some(attachments) = &submission.attachments {
        fields.push(FieldWrite {
            name: "attachments_json",
            group: FieldGroup::Detail,
            value: Some(attachments_json(attachments).to_string()),
        });
    }
    EntityIngest { entity_key, fields }
}

const COLUMN_FIELDS: &[&str] = &[
    "assignment_id",
    "user_id",
    "attempt",
    "score",
    "grade",
    "submitted_at",
    "workflow_state",
    "late",
    "missing",
    "excused",
];

const EXTRA_FIELDS: &[&str] = &[
    "submission_history_json",
    "submission_comments_json",
    "rubric_assessment_json",
    "id",
    "submission_type",
    "url",
    "posted_at",
    "body_sha256",
    "attachments_json",
];

fn upsert_submission(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    validate_fields(&entity.fields)?;
    let id = entity
        .entity_key
        .parse::<i64>()
        .map_err(|_| DbError::Message("invalid submission key".into()))?;
    tx.execute(
        "INSERT INTO submissions (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(
        tx,
        "submission",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    apply_submission_columns(tx, id, &entity.fields, &applied.fields)?;
    merge_submission_extra(tx, id, &entity.fields, &applied.fields)?;
    touch_submission_observed(
        tx,
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
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

fn apply_submission_columns(
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
            "grade" | "submitted_at" | "workflow_state" => {
                tx.execute(
                    &format!("UPDATE submissions SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            "assignment_id" | "user_id" | "attempt" => {
                let num: Option<i64> = match &field.value {
                    None => None,
                    Some(s) => Some(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                };
                tx.execute(
                    &format!("UPDATE submissions SET {} = ?1 WHERE id = ?2", field.name),
                    params![num, id],
                )?;
            }
            "score" => {
                let num: Option<f64> = match &field.value {
                    None => None,
                    Some(s) => Some(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                };
                tx.execute(
                    "UPDATE submissions SET score = ?1 WHERE id = ?2",
                    params![num, id],
                )?;
            }
            "late" | "missing" | "excused" => {
                let bit: Option<i64> = match &field.value {
                    None => None,
                    Some(s) if s == "true" || s == "1" => Some(1),
                    Some(_) => Some(0),
                };
                tx.execute(
                    &format!("UPDATE submissions SET {} = ?1 WHERE id = ?2", field.name),
                    params![bit, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_submission_extra(
    tx: &Transaction<'_>,
    id: i64,
    fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    let mut data = load_submission_data_json(tx, id)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("submissions data_json must be an object".into()))?;
    for field in fields {
        if !applied.contains(&field.name) || !EXTRA_FIELDS.contains(&field.name) {
            continue;
        }
        match &field.value {
            None => {
                obj.insert(field.name.into(), Value::Null);
            }
            Some(v) => {
                let parsed: Value = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
                obj.insert(field.name.into(), parsed);
            }
        }
    }
    tx.execute(
        "UPDATE submissions SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(())
}

fn load_submission_data_json(tx: &Transaction<'_>, id: i64) -> Result<Value, IngestError> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT data_json FROM submissions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        None => Ok(Value::Object(Map::new())),
        Some(s) if s.is_empty() => Ok(Value::Object(Map::new())),
        Some(s) => serde_json::from_str(&s)
            .map_err(|_| DbError::Message("invalid submissions data_json".into()).into()),
    }
}

fn touch_submission_observed(
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
            "UPDATE submissions SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            "UPDATE submissions SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            "UPDATE submissions SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

/// Projection shared by current and historical attachments; capability URLs are excluded.
pub fn attachments_json(attachments: &[canvas_api::models::SubmissionAttachment]) -> Value {
    Value::Array(attachments.iter().map(|a| serde_json::json!({
        "id": a.id.to_string(), "display_name": a.display_name.as_ref().or(a.filename.as_ref()),
        "size": a.size, "content_type": a.content_type,
    })).collect())
}

/// Keep only criterion feedback, excluding unknown response fields.
pub fn rubric_json(raw: &Value) -> Value {
    Value::Array(
        raw.as_object()
            .into_iter()
            .flat_map(|o| o.iter())
            .map(|(id, v)| {
                serde_json::json!({
                    "criterion_id": id, "points": v.get("points").and_then(Value::as_f64),
                    "comments": v.get("comments").and_then(Value::as_str),
                    // M8-a: which rating the grader picked, when Canvas says.
                    "rating_id": v.get("rating_id").and_then(|r| r
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| r.as_i64().map(|n| n.to_string()))),
                })
            })
            .collect(),
    )
}

/// Preserve absence/null/value while storing only the submission's allowlisted projection.
pub(super) fn observed_submission(
    raw: &Value,
    assignment_id: i64,
) -> Result<EntityIngest, super::SyncError> {
    let mut model: Submission =
        serde_json::from_value(raw.clone()).map_err(|_| canvas_api::Error::Decode)?;
    if model.assignment_id.is_some_and(|id| id != assignment_id) || assignment_id <= 0 {
        return Err(canvas_api::Error::Decode.into());
    }
    model.assignment_id = Some(assignment_id);
    let mut entity = submission_to_entity(&model);
    for (wire, stored) in [
        ("attachments", "attachments_json"),
        ("body", "body_sha256"),
        ("url", "url"),
        ("submission_type", "submission_type"),
        ("posted_at", "posted_at"),
        ("submission_history", "submission_history_json"),
        ("submission_comments", "submission_comments_json"),
        ("rubric_assessment", "rubric_assessment_json"),
    ] {
        if raw.get(wire).is_some_and(Value::is_null) {
            entity.fields.retain(|f| f.name != stored);
            entity.fields.push(FieldWrite {
                name: stored,
                group: FieldGroup::Detail,
                value: None,
            });
        }
    }
    if raw.get("user_id").is_some_and(Value::is_null) {
        entity.fields.retain(|f| f.name != "user_id");
        entity.fields.push(FieldWrite {
            name: "user_id",
            group: FieldGroup::Core,
            value: None,
        });
    }
    Ok(entity)
}

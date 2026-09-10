//! `assignment_groups` dataset (`course:<id>:period:<id|none>`).
//!
//! The group entity (name, position, weight, rules) does not vary by grading
//! period, so its tracked columns are written under the plain group id and share
//! one observation clock. The assignment list *does* vary by period, so it is
//! stored per period inside `data_json.assignments_by_period`, each entry
//! carrying its own `observed_at`. A period-P fetch therefore can never
//! overwrite or freshen the list observed for period Q (SPEC §10).

use canvas_api::models::{Assignment, AssignmentGroup};
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::course_totals::default_ttl_grades;
use super::enrollment_grades::PeriodKey;
use super::fields::{push_api_f64, push_api_str, push_api_to_string};

/// Assignment groups for one course under one grading-period qualifier.
#[derive(Debug, Clone)]
pub struct AssignmentGroupsDataset {
    pub course_id: i64,
    pub period: PeriodKey,
    pub ttl: Span,
    scope_key: String,
    period_value: String,
}

impl AssignmentGroupsDataset {
    #[must_use]
    pub fn new(course_id: i64, period: PeriodKey, ttl: Span) -> Self {
        Self {
            course_id,
            period,
            ttl,
            scope_key: format!("course:{course_id}:{}", period.scope_key()),
            period_value: period.period_value(),
        }
    }

    #[must_use]
    pub fn with_default_ttl(course_id: i64, period: PeriodKey) -> Self {
        Self::new(course_id, period, default_ttl_grades())
    }
}

impl Dataset for AssignmentGroupsDataset {
    fn name(&self) -> &'static str {
        "assignment_groups"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "assignment_group"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_assignment_group(tx, self.course_id, &self.period_value, entity, fetched_at)
    }
}

/// Appendix B assignment-groups path, with `grading_period_id` for an explicit period.
#[must_use]
pub fn assignment_groups_path(course_id: i64, period: PeriodKey) -> String {
    let mut path = format!(
        "/api/v1/courses/{course_id}/assignment_groups\
         ?include[]=assignments&include[]=submission&override_assignment_dates=true&per_page=100"
    );
    if let PeriodKey::Id(id) = period {
        use std::fmt::Write as _;
        let _ = write!(path, "&grading_period_id={id}");
    }
    path
}

/// Convert assignment-group pages into an ingest page.
#[must_use]
pub fn assignment_groups_to_ingest_page(
    groups: &[AssignmentGroup],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: groups
            .iter()
            .map(|g| assignment_group_to_entity(g, course_id, fetched_at))
            .collect(),
    }
}

/// Convert one assignment group into an entity ingest row.
///
/// `fetched_at` stamps the per-period assignment list so its freshness is
/// independent of the group's own observation clocks.
#[must_use]
pub fn assignment_group_to_entity(
    group: &AssignmentGroup,
    course_id: i64,
    fetched_at: Timestamp,
) -> EntityIngest {
    let mut fields = Vec::new();
    push_api_str(&mut fields, "name", FieldGroup::Core, &group.name);
    push_api_to_string(&mut fields, "position", FieldGroup::Core, &group.position);
    push_api_f64(
        &mut fields,
        "group_weight",
        FieldGroup::Core,
        &group.group_weight,
    );
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(group.course_id.unwrap_or(course_id).to_string()),
    });
    // Absent `rules` is Canvas saying nothing; an empty object is "no rules".
    if let Some(ref rules) = group.rules {
        fields.push(FieldWrite {
            name: "rules_json",
            group: FieldGroup::Detail,
            value: Some(rules_json(rules).to_string()),
        });
    }
    // Absent `assignments` leaves the stored list for this period untouched.
    if let Some(ref assignments) = group.assignments {
        fields.push(FieldWrite {
            name: "assignments_payload",
            group: FieldGroup::Detail,
            value: Some(
                json!({
                    "observed_at": fetched_at.to_string(),
                    "assignments": assignments.iter().map(assignment_projection).collect::<Vec<_>>(),
                })
                .to_string(),
            ),
        });
    }
    EntityIngest {
        entity_key: group.id.to_string(),
        fields,
    }
}

/// Allowlisted drop rules (`never_drop` is never null in JSON).
fn rules_json(rules: &canvas_api::models::AssignmentGroupRules) -> Value {
    json!({
        "drop_lowest": rules.drop_lowest,
        "drop_highest": rules.drop_highest,
        "never_drop": rules
            .never_drop
            .as_ref()
            .map(|ids| ids.iter().map(ToString::to_string).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

/// Allowlisted per-assignment projection for the course view (SPEC §12.4).
fn assignment_projection(assignment: &Assignment) -> Value {
    let submission = assignment.submission.as_ref();
    json!({
        "id": assignment.id.to_string(),
        "name": assignment.name.as_value(),
        "due_at": assignment.due_at.as_value().map(ToString::to_string),
        "points_possible": assignment.points_possible.as_value(),
        "omit_from_final_grade": assignment.omit_from_final_grade.unwrap_or(false),
        "score": submission.and_then(|s| s.score.as_value().copied()),
        "grade": submission.and_then(|s| s.grade.as_value().cloned()),
        "excused": submission.and_then(|s| s.excused.as_value().copied()),
        "late": submission.and_then(|s| s.late.as_value().copied()),
        "missing": submission.and_then(|s| s.missing.as_value().copied()),
        "posted_at": submission.and_then(|s| s.posted_at.map(|t| t.to_string())),
        "workflow_state": submission.and_then(|s| s.workflow_state.as_value().cloned()),
        "submitted_at": submission.and_then(|s| s.submitted_at.as_value().map(ToString::to_string)),
        "attempt": submission.and_then(|s| s.attempt.as_value().copied()),
    })
}

fn upsert_assignment_group(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    period_value: &str,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_group_id(&entity.entity_key)?;
    let course_id = entity
        .fields
        .iter()
        .find(|f| f.name == "course_id")
        .and_then(|f| f.value.as_deref())
        .map_or(Ok(dataset_course_id), str::parse)
        .map_err(|e: std::num::ParseIntError| {
            DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;
    tx.execute(
        "INSERT INTO assignment_groups (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, course_id],
    )?;

    // The per-period list is merged separately; it must not go through a column.
    let (payload, column_fields): (Vec<_>, Vec<_>) = entity
        .fields
        .iter()
        .cloned()
        .partition(|f| f.name == "assignments_payload");
    validate_group_fields(&column_fields)?;

    let applied = apply_field_writes(
        tx,
        "assignment_group",
        &entity.entity_key,
        fetched_at,
        &column_fields,
    )?;
    apply_group_columns(tx, id, &column_fields, &applied.fields)?;
    let wrote_payload = match payload.first() {
        Some(entry) => {
            merge_period_assignments(tx, id, period_value, entry.value.as_deref(), fetched_at)?
        }
        None => false,
    };
    touch_group_observed(
        tx,
        id,
        fetched_at,
        applied.core,
        applied.detail || wrote_payload,
        applied.status,
    )?;
    Ok(())
}

fn parse_group_id(entity_key: &str) -> Result<i64, IngestError> {
    let id: i64 = entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad assignment group id: {e}"),
            ),
        )))
    })?;
    if entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(id)
}

fn validate_group_fields(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let allowed = [
        "name",
        "position",
        "group_weight",
        "rules_json",
        "course_id",
    ];
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(
                DbError::Message("unsupported or duplicate assignment_group field".into()).into(),
            );
        }
    }
    Ok(())
}

fn apply_group_columns(
    tx: &Transaction<'_>,
    id: i64,
    tracked: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in tracked {
        if !applied.contains(&field.name) {
            continue;
        }
        match field.name {
            "name" | "rules_json" => {
                tx.execute(
                    &format!(
                        "UPDATE assignment_groups SET {} = ?1 WHERE id = ?2",
                        field.name
                    ),
                    params![field.value, id],
                )?;
            }
            "course_id" | "position" => {
                let n: Option<i64> = match field.value {
                    Some(ref v) => Some(v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                    None => None,
                };
                tx.execute(
                    &format!(
                        "UPDATE assignment_groups SET {} = ?1 WHERE id = ?2",
                        field.name
                    ),
                    params![n, id],
                )?;
            }
            "group_weight" => {
                let n: Option<f64> = match field.value {
                    Some(ref v) => Some(v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                    None => None,
                };
                tx.execute(
                    "UPDATE assignment_groups SET group_weight = ?1 WHERE id = ?2",
                    params![n, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Replace only this period's entry, keeping every other period's list and
/// clock. Returns whether the entry was written.
fn merge_period_assignments(
    tx: &Transaction<'_>,
    id: i64,
    period_value: &str,
    payload: Option<&str>,
    fetched_at: Timestamp,
) -> Result<bool, IngestError> {
    let Some(payload) = payload else {
        return Ok(false);
    };
    let entry: Value = serde_json::from_str(payload).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    let raw: String = tx.query_row(
        "SELECT data_json FROM assignment_groups WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    let mut data: Value = serde_json::from_str(&raw).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("assignment_groups data_json must be an object".into()))?;
    let by_period = obj
        .entry("assignments_by_period")
        .or_insert_with(|| Value::Object(Map::new()));
    let by_period = by_period
        .as_object_mut()
        .ok_or_else(|| DbError::Message("assignments_by_period must be an object".into()))?;
    // An older arrival never replaces a newer list for the same period.
    let newer = by_period
        .get(period_value)
        .and_then(|e| e.get("observed_at"))
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<Timestamp>().ok())
        .is_some_and(|prev| prev > fetched_at);
    if newer {
        return Ok(false);
    }
    by_period.insert(period_value.to_owned(), entry);
    tx.execute(
        "UPDATE assignment_groups SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(true)
}

fn touch_group_observed(
    tx: &Transaction<'_>,
    id: i64,
    fetched_at: Timestamp,
    core: bool,
    detail: bool,
    status: bool,
) -> Result<(), IngestError> {
    let ts = fetched_at.to_string();
    for (flag, column) in [
        (core, "observed_at_core"),
        (detail, "observed_at_detail"),
        (status, "observed_at_status"),
    ] {
        if flag {
            tx.execute(
                &format!("UPDATE assignment_groups SET {column} = ?1 WHERE id = ?2"),
                params![ts, id],
            )?;
        }
    }
    Ok(())
}

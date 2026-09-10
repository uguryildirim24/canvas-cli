//! `course_totals` dataset (usually written as a side effect of `courses`).

use canvas_api::Supplied as ApiSupplied;
use canvas_api::models::CourseEnrollment;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, apply_field_writes,
};

use super::fields::{parse_opt_f64, push_api_f64, push_api_str};

/// Default TTL mirrors `ttl_grades`.
#[must_use]
pub fn default_ttl_grades() -> Span {
    Span::new().minutes(10)
}

/// Course-total mode qualifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CourseTotalMode {
    All,
    Current,
}

impl CourseTotalMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Current => "current",
        }
    }
}

/// Lookup-oriented `course_totals` dataset (`scope = course:<id>`).
#[derive(Debug, Clone)]
pub struct CourseTotalsDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl CourseTotalsDataset {
    #[must_use]
    pub fn new(course_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            ttl,
            scope_key: format!("course:{course_id}"),
        }
    }
}

impl Dataset for CourseTotalsDataset {
    fn name(&self) -> &'static str {
        "course_totals"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "course_totals"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_course_totals_entity(tx, entity, fetched_at)
    }
}

/// Grade fields used to build one mode row.
struct ModeScores<'a> {
    current_score: &'a ApiSupplied<f64>,
    final_score: &'a ApiSupplied<f64>,
    current_grade: &'a ApiSupplied<String>,
    final_grade: &'a ApiSupplied<String>,
}

/// Build `all` / `current` entities from a student enrollment summary.
#[must_use]
pub fn totals_from_enrollment(course_id: i64, enrollment: &CourseEnrollment) -> Vec<EntityIngest> {
    let meta = (
        enrollment.current_grading_period_id,
        enrollment.current_grading_period_title.as_deref(),
    );
    vec![
        totals_entity(
            course_id,
            CourseTotalMode::All,
            &ModeScores {
                current_score: &enrollment.computed_current_score,
                final_score: &enrollment.computed_final_score,
                current_grade: &enrollment.computed_current_grade,
                final_grade: &enrollment.computed_final_grade,
            },
            (None, None),
        ),
        totals_entity(
            course_id,
            CourseTotalMode::Current,
            &ModeScores {
                current_score: &enrollment.current_period_computed_current_score,
                final_score: &enrollment.current_period_computed_final_score,
                current_grade: &enrollment.current_period_computed_current_grade,
                final_grade: &enrollment.current_period_computed_final_grade,
            },
            meta,
        ),
    ]
}

fn totals_entity(
    course_id: i64,
    mode: CourseTotalMode,
    scores: &ModeScores<'_>,
    meta: (Option<i64>, Option<&str>),
) -> EntityIngest {
    let mut fields = Vec::new();
    push_api_f64(
        &mut fields,
        "current_score",
        FieldGroup::Status,
        scores.current_score,
    );
    push_api_f64(
        &mut fields,
        "final_score",
        FieldGroup::Status,
        scores.final_score,
    );
    push_api_str(
        &mut fields,
        "current_grade",
        FieldGroup::Status,
        scores.current_grade,
    );
    push_api_str(
        &mut fields,
        "final_grade",
        FieldGroup::Status,
        scores.final_grade,
    );
    if mode == CourseTotalMode::All {
        fields.push(FieldWrite {
            name: "period_id",
            group: FieldGroup::Detail,
            value: None,
        });
        fields.push(FieldWrite {
            name: "period_title",
            group: FieldGroup::Detail,
            value: None,
        });
    }
    if let Some(id) = meta.0 {
        fields.push(FieldWrite {
            name: "period_id",
            group: FieldGroup::Detail,
            value: Some(id.to_string()),
        });
    }
    if let Some(title) = meta.1 {
        fields.push(FieldWrite {
            name: "period_title",
            group: FieldGroup::Detail,
            value: Some(title.to_owned()),
        });
    }
    EntityIngest {
        entity_key: format!("{course_id}|{}", mode.as_str()),
        fields,
    }
}

/// Upsert one `course_totals` row from a composite entity key.
pub fn upsert_course_totals_entity(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let (course_id, mode) = parse_totals_key(&entity.entity_key)?;
    upsert_course_totals(tx, course_id, mode, fetched_at, &entity.fields)
}

/// Upsert `course_totals` for one `(course_id, mode)`.
pub fn upsert_course_totals(
    tx: &Transaction<'_>,
    course_id: i64,
    mode: &str,
    fetched_at: Timestamp,
    fields: &[FieldWrite],
) -> Result<(), IngestError> {
    if mode != "all" && mode != "current" {
        return Err(DbError::Message(format!("invalid course_totals mode: {mode}")).into());
    }
    validate_totals_fields(fields)?;
    let entity_key = format!("{course_id}|{mode}");
    tx.execute(
        "INSERT INTO course_totals (course_id, mode) VALUES (?1, ?2)
         ON CONFLICT(course_id, mode) DO NOTHING",
        params![course_id, mode],
    )?;
    let value_fields = fields.to_vec();
    let applied = apply_field_writes(tx, "course_totals", &entity_key, fetched_at, fields)?;
    for field in &value_fields {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        match field.name {
            "current_score" | "final_score" => {
                let v = parse_opt_f64(field.value.as_ref()).map_err(|e| {
                    DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                })?;
                tx.execute(
                    &format!(
                        "UPDATE course_totals SET {} = ?1 WHERE course_id = ?2 AND mode = ?3",
                        field.name
                    ),
                    params![v, course_id, mode],
                )?;
            }
            "current_grade" | "final_grade" => {
                tx.execute(
                    &format!(
                        "UPDATE course_totals SET {} = ?1 WHERE course_id = ?2 AND mode = ?3",
                        field.name
                    ),
                    params![field.value, course_id, mode],
                )?;
            }
            _ => {}
        }
    }
    let winning: Vec<_> = fields
        .iter()
        .filter(|f| applied.fields.contains(&f.name))
        .cloned()
        .collect();
    merge_period_meta(tx, course_id, mode, &winning)?;
    let ts = fetched_at.to_string();
    if applied.core {
        tx.execute(
            "UPDATE course_totals SET observed_at_core = ?1 WHERE course_id = ?2 AND mode = ?3",
            params![ts, course_id, mode],
        )?;
    }
    if applied.detail {
        tx.execute(
            "UPDATE course_totals SET observed_at_detail = ?1 WHERE course_id = ?2 AND mode = ?3",
            params![ts, course_id, mode],
        )?;
    }
    if applied.status {
        tx.execute(
            "UPDATE course_totals SET observed_at_status = ?1 WHERE course_id = ?2 AND mode = ?3",
            params![ts, course_id, mode],
        )?;
    }
    Ok(())
}

fn merge_period_meta(
    tx: &Transaction<'_>,
    course_id: i64,
    mode: &str,
    fields: &[FieldWrite],
) -> Result<(), IngestError> {
    let mut data = load_data_json(tx, course_id, mode)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("course_totals data_json must be an object".into()))?;
    for field in fields {
        match field.name {
            "period_id" | "period_title" => {
                obj.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => serde_json::Value::String(v.clone()),
                        None => serde_json::Value::Null,
                    },
                );
            }
            _ => {}
        }
    }
    tx.execute(
        "UPDATE course_totals SET data_json = ?1 WHERE course_id = ?2 AND mode = ?3",
        params![data.to_string(), course_id, mode],
    )?;
    Ok(())
}

fn load_data_json(
    tx: &Transaction<'_>,
    course_id: i64,
    mode: &str,
) -> Result<serde_json::Value, IngestError> {
    let raw: String = tx.query_row(
        "SELECT data_json FROM course_totals WHERE course_id = ?1 AND mode = ?2",
        params![course_id, mode],
        |r| r.get(0),
    )?;
    serde_json::from_str(&raw).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
        .into()
    })
}

fn parse_totals_key(key: &str) -> Result<(i64, &str), IngestError> {
    let (id, mode) = key
        .split_once('|')
        .ok_or_else(|| DbError::Message("course_totals key must be course_id|mode".into()))?;
    let course_id: i64 = id.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad course id: {e}"),
            ),
        )))
    })?;
    if mode != "all" && mode != "current" {
        return Err(DbError::Message(format!("invalid course_totals mode: {mode}")).into());
    }
    if key != format!("{course_id}|{mode}") {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok((course_id, mode))
}

fn validate_totals_fields(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let allowed = [
        "current_score",
        "final_score",
        "current_grade",
        "final_grade",
        "period_id",
        "period_title",
    ];
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(
                DbError::Message("unsupported or duplicate course_totals field".into()).into(),
            );
        }
    }
    Ok(())
}

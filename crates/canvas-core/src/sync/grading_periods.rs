//! `grading_periods` dataset (`course:<id>`).

use canvas_api::models::GradingPeriod;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::course_totals::default_ttl_grades;
use super::fields::{push_opt_bool, push_opt_str, push_opt_ts};

/// Grading periods for one course.
#[derive(Debug, Clone)]
pub struct GradingPeriodsDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl GradingPeriodsDataset {
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
        Self::new(course_id, default_ttl_grades())
    }
}

impl Dataset for GradingPeriodsDataset {
    fn name(&self) -> &'static str {
        "grading_periods"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "grading_period"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_grading_period(tx, self.course_id, entity, fetched_at)
    }
}

/// Path for wrapped grading-period pages.
#[must_use]
pub fn grading_periods_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/grading_periods")
}

/// Convert grading-period items into an ingest page.
#[must_use]
pub fn grading_periods_to_ingest_page(
    periods: &[GradingPeriod],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: periods
            .iter()
            .map(|p| grading_period_to_entity(p, course_id))
            .collect(),
    }
}

/// Convert one grading period into an entity ingest row.
#[must_use]
pub fn grading_period_to_entity(period: &GradingPeriod, course_id: i64) -> EntityIngest {
    let mut fields = Vec::new();
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        period.title.as_deref(),
    );
    push_opt_ts(
        &mut fields,
        "start_date",
        FieldGroup::Core,
        period.start_date,
    );
    push_opt_ts(&mut fields, "end_date", FieldGroup::Core, period.end_date);
    push_opt_ts(
        &mut fields,
        "close_date",
        FieldGroup::Detail,
        period.close_date,
    );
    if let Some(w) = period.weight {
        fields.push(FieldWrite {
            name: "weight",
            group: FieldGroup::Detail,
            value: Some(w.to_string()),
        });
    }
    push_opt_bool(
        &mut fields,
        "is_closed",
        FieldGroup::Status,
        period.is_closed,
    );
    fields.push(FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    });
    EntityIngest {
        entity_key: period.id.to_string(),
        fields,
    }
}

struct PeriodSplit {
    course_id: i64,
    column_fields: Vec<FieldWrite>,
    extra: Map<String, Value>,
}

fn upsert_grading_period(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_period_id(&entity.entity_key)?;
    let split = split_period_fields(dataset_course_id, &entity.fields)?;
    validate_column_fields(&split.column_fields)?;
    tx.execute(
        "INSERT INTO grading_periods (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, split.course_id],
    )?;
    let applied = apply_field_writes(
        tx,
        "grading_period",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    if applied.fields.contains(&"course_id") {
        tx.execute(
            "UPDATE grading_periods SET course_id = ?1 WHERE id = ?2",
            params![split.course_id, id],
        )?;
    }
    apply_period_columns(tx, id, &split.column_fields, &applied.fields)?;
    let extra = split
        .extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_period_extra(tx, id, extra)?;
    touch_period_observed(
        tx,
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

fn parse_period_id(entity_key: &str) -> Result<i64, IngestError> {
    let id: i64 = entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad grading period id: {e}"),
            ),
        )))
    })?;
    if entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(id)
}

fn split_period_fields(
    dataset_course_id: i64,
    fields: &[FieldWrite],
) -> Result<PeriodSplit, IngestError> {
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    let mut course_id = dataset_course_id;
    for field in fields {
        match field.name {
            "title" | "start_date" | "end_date" => column_fields.push(field.clone()),
            "course_id" => {
                if let Some(ref v) = field.value {
                    course_id = v.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?;
                }
                column_fields.push(field.clone());
            }
            "close_date" | "weight" | "is_closed" => {
                extra.insert(
                    field.name.to_owned(),
                    match &field.value {
                        Some(v) => Value::String(v.clone()),
                        None => Value::Null,
                    },
                );
            }
            other => {
                return Err(
                    DbError::Message(format!("unsupported grading_period field: {other}")).into(),
                );
            }
        }
    }
    Ok(PeriodSplit {
        course_id,
        column_fields,
        extra,
    })
}

fn apply_period_columns(
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
            "title" | "start_date" | "end_date" => {
                tx.execute(
                    &format!(
                        "UPDATE grading_periods SET {} = ?1 WHERE id = ?2",
                        field.name
                    ),
                    params![field.value, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_period_extra(
    tx: &Transaction<'_>,
    id: i64,
    extra: Map<String, Value>,
) -> Result<(), IngestError> {
    if extra.is_empty() {
        return Ok(());
    }
    let raw: String = tx.query_row(
        "SELECT data_json FROM grading_periods WHERE id = ?1",
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
        .ok_or_else(|| DbError::Message("grading_periods data_json must be an object".into()))?;
    for (k, v) in extra {
        obj.insert(k, v);
    }
    tx.execute(
        "UPDATE grading_periods SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(())
}

fn touch_period_observed(
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
            "UPDATE grading_periods SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            "UPDATE grading_periods SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            "UPDATE grading_periods SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

fn validate_column_fields(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let allowed = ["title", "start_date", "end_date", "course_id"];
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(
                DbError::Message("unsupported or duplicate grading_period field".into()).into(),
            );
        }
    }
    Ok(())
}

//! `planner` dataset (`window:<start>..<end>`).

#![allow(clippy::map_unwrap_or, clippy::collapsible_if, clippy::too_many_lines)]

use canvas_api::models::PlannerItem;
use jiff::{Span, Timestamp, civil::Date};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_api_to_string, push_opt_bool, push_opt_i64, push_opt_str, push_opt_ts};

/// Default TTL: `ttl_planner` = 10 minutes.
#[must_use]
pub fn default_ttl_planner() -> Span {
    Span::new().minutes(10)
}

/// Planner window coverage (UTC civil days, inclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerWindow {
    pub start: Date,
    pub end: Date,
}

impl PlannerWindow {
    /// Build a window of `days` starting at `start` (inclusive).
    #[must_use]
    pub fn from_days(start: Date, days: u32) -> Self {
        let end = start
            .checked_add(jiff::Span::new().days(i64::from(days.saturating_sub(1))))
            .unwrap_or(start);
        Self { start, end }
    }

    /// Default todo window: `today − 1` for `days` (default 14).
    #[must_use]
    pub fn todo_default(today: Date, days: u32) -> Self {
        let start = today
            .checked_sub(jiff::Span::new().days(1))
            .unwrap_or(today);
        Self::from_days(start, days)
    }

    #[must_use]
    pub fn scope_key(&self) -> String {
        format!("window:{}..{}", self.start, self.end)
    }

    #[must_use]
    pub fn start_timestamp(&self) -> Timestamp {
        self.start
            .in_tz("UTC")
            .map(|z| z.timestamp())
            .unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[must_use]
    pub fn end_timestamp(&self) -> Timestamp {
        // Exclusive end: start of the day after `end`.
        match self.end.checked_add(jiff::Span::new().days(1)) {
            Ok(d) => d
                .in_tz("UTC")
                .map(|z| z.timestamp())
                .unwrap_or(Timestamp::UNIX_EPOCH),
            Err(_) => Timestamp::UNIX_EPOCH,
        }
    }
}

/// Planner items for a UTC-day window.
#[derive(Debug, Clone)]
pub struct PlannerDataset {
    pub window: PlannerWindow,
    pub ttl: Span,
    scope_key: String,
}

impl PlannerDataset {
    #[must_use]
    pub fn new(window: PlannerWindow, ttl: Span) -> Self {
        Self {
            scope_key: window.scope_key(),
            window,
            ttl,
        }
    }

    #[must_use]
    pub fn with_default_ttl(window: PlannerWindow) -> Self {
        Self::new(window, default_ttl_planner())
    }
}

impl Dataset for PlannerDataset {
    fn name(&self) -> &'static str {
        "planner"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "planner_item"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_planner_item(tx, entity, fetched_at)
    }
}

/// Build the planner items path for a window.
#[must_use]
pub fn planner_path(window: &PlannerWindow) -> String {
    format!(
        "/api/v1/planner/items?start_date={}&end_date={}&per_page=100",
        window.start, window.end
    )
}

/// Convert planner items into an ingest page.
#[must_use]
pub fn planner_to_ingest_page(items: &[PlannerItem], fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items.iter().filter_map(planner_item_to_entity).collect(),
    }
}

/// Convert one planner item; skips items without a stable key.
#[must_use]
pub fn planner_item_to_entity(item: &PlannerItem) -> Option<EntityIngest> {
    let plannable_type = item
        .plannable_type
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let plannable_id = item
        .plannable_id
        .or_else(|| item.plannable.as_ref().and_then(|p| p.id))?;
    let entity_key = format!("{plannable_type}:{plannable_id}");
    let mut fields = Vec::new();
    fields.push(FieldWrite {
        name: "plannable_id",
        group: FieldGroup::Core,
        value: Some(plannable_id.to_string()),
    });
    fields.push(FieldWrite {
        name: "plannable_type",
        group: FieldGroup::Core,
        value: Some(plannable_type),
    });
    push_opt_i64(
        &mut fields,
        "course_id",
        FieldGroup::Core,
        item.course_id
            .or_else(|| item.plannable.as_ref().and_then(|p| p.course_id)),
    );
    let title = item
        .plannable
        .as_ref()
        .and_then(|p| p.title.clone())
        .or_else(|| item.context_name.clone());
    push_opt_str(&mut fields, "title", FieldGroup::Core, title.as_deref());
    push_opt_ts(
        &mut fields,
        "plannable_date",
        FieldGroup::Core,
        item.plannable_date,
    );
    if let Some(p) = item.plannable.as_ref() {
        push_api_to_string(&mut fields, "due_at", FieldGroup::Core, &p.due_at);
        push_opt_i64(
            &mut fields,
            "assignment_id",
            FieldGroup::Core,
            p.assignment_id,
        );
        push_opt_i64(
            &mut fields,
            "parent_assignment_id",
            FieldGroup::Core,
            p.parent_assignment_id,
        );
        push_opt_ts(&mut fields, "todo_date", FieldGroup::Core, p.todo_date);
        push_opt_ts(&mut fields, "start_at", FieldGroup::Core, p.start_at);
    }
    push_api_to_string(&mut fields, "html_url", FieldGroup::Detail, &item.html_url);
    if let Some(o) = item.planner_override.as_ref() {
        push_opt_bool(
            &mut fields,
            "marked_complete",
            FieldGroup::Status,
            o.marked_complete,
        );
        push_opt_bool(&mut fields, "dismissed", FieldGroup::Status, o.dismissed);
    }
    // Preserve submissions blob and override for merge.
    if let Some(ref subs) = item.submissions {
        fields.push(FieldWrite {
            name: "submissions_json",
            group: FieldGroup::Status,
            value: Some(subs.to_string()),
        });
    }
    if let Some(ref o) = item.planner_override {
        if let Ok(json) = serde_json::to_string(o) {
            fields.push(FieldWrite {
                name: "planner_override_json",
                group: FieldGroup::Status,
                value: Some(json),
            });
        }
    }
    Some(EntityIngest { entity_key, fields })
}

const COLUMN_FIELDS: &[&str] = &["plannable_id", "plannable_type", "course_id", "title"];

const EXTRA_FIELDS: &[&str] = &[
    "plannable_date",
    "due_at",
    "assignment_id",
    "parent_assignment_id",
    "todo_date",
    "start_at",
    "html_url",
    "marked_complete",
    "dismissed",
    "submissions_json",
    "planner_override_json",
];

fn upsert_planner_item(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    validate_fields(&entity.fields)?;
    tx.execute(
        "INSERT INTO planner_items (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![entity.entity_key],
    )?;
    let applied = apply_field_writes(
        tx,
        "planner_item",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    apply_planner_columns(tx, &entity.entity_key, &entity.fields, &applied.fields)?;
    merge_planner_extra(tx, &entity.entity_key, &entity.fields, &applied.fields)?;
    touch_planner_observed(
        tx,
        &entity.entity_key,
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

fn apply_planner_columns(
    tx: &Transaction<'_>,
    id: &str,
    fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in fields {
        if !applied.contains(&field.name) || !COLUMN_FIELDS.contains(&field.name) {
            continue;
        }
        match field.name {
            "plannable_type" | "title" => {
                tx.execute(
                    &format!("UPDATE planner_items SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            "plannable_id" | "course_id" => {
                let num: Option<i64> = match &field.value {
                    None => None,
                    Some(s) => Some(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                };
                tx.execute(
                    &format!("UPDATE planner_items SET {} = ?1 WHERE id = ?2", field.name),
                    params![num, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_planner_extra(
    tx: &Transaction<'_>,
    id: &str,
    fields: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    let mut data = load_planner_data_json(tx, id)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("planner_items data_json must be an object".into()))?;
    for field in fields {
        if !applied.contains(&field.name) || !EXTRA_FIELDS.contains(&field.name) {
            continue;
        }
        match &field.value {
            None => {
                obj.insert(field.name.into(), Value::Null);
            }
            Some(v) if matches!(field.name, "marked_complete" | "dismissed") => {
                obj.insert(field.name.into(), Value::Bool(v == "true" || v == "1"));
            }
            Some(v) if field.name.ends_with("_json") => {
                let parsed: Value = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
                obj.insert(field.name.into(), parsed);
            }
            Some(v) => {
                obj.insert(field.name.into(), Value::String(v.clone()));
            }
        }
    }
    tx.execute(
        "UPDATE planner_items SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(())
}

fn load_planner_data_json(tx: &Transaction<'_>, id: &str) -> Result<Value, IngestError> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT data_json FROM planner_items WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        None => Ok(Value::Object(Map::new())),
        Some(s) if s.is_empty() => Ok(Value::Object(Map::new())),
        Some(s) => serde_json::from_str(&s)
            .map_err(|_| DbError::Message("invalid planner_items data_json".into()).into()),
    }
}

fn touch_planner_observed(
    tx: &Transaction<'_>,
    id: &str,
    fetched_at: Timestamp,
    core: bool,
    detail: bool,
    status: bool,
) -> Result<(), IngestError> {
    let ts = fetched_at.to_string();
    if core {
        tx.execute(
            "UPDATE planner_items SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            "UPDATE planner_items SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            "UPDATE planner_items SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

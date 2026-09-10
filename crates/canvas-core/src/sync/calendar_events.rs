//! `calendar_events` dataset (`window:<start>..<end>:ctx:<sha256>`, §12.5).

use canvas_api::models::CalendarEvent;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, IngestError, IngestPage, apply_field_writes,
};

use super::context_window::{ContextWindow, context_codes_query};
use super::fields::{push_api_str, push_api_to_string, push_opt_bool, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_calendar` = 1 hour.
#[must_use]
pub fn default_ttl_calendar() -> Span {
    Span::new().hours(1)
}

/// Calendar events for a window over a set of contexts.
#[derive(Debug, Clone)]
pub struct CalendarEventsDataset {
    pub window: ContextWindow,
    pub ttl: Span,
}

impl CalendarEventsDataset {
    #[must_use]
    pub fn new(window: ContextWindow, ttl: Span) -> Self {
        Self { window, ttl }
    }

    #[must_use]
    pub fn with_default_ttl(window: ContextWindow) -> Self {
        Self::new(window, default_ttl_calendar())
    }
}

impl Dataset for CalendarEventsDataset {
    fn name(&self) -> &'static str {
        "calendar_events"
    }

    fn scope_key(&self) -> &str {
        self.window.scope_key()
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "calendar_event"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_calendar_event(tx, entity, fetched_at)
    }
}

/// Listing path for one batch of at most ten contexts.
///
/// `type=event` only: assignment due dates reach `calendar` through the planner
/// window (§12.5), so asking for them here would duplicate every deadline.
#[must_use]
pub fn calendar_events_path(batch: &[String], window: &ContextWindow) -> String {
    format!(
        "/api/v1/calendar_events?type=event&start_date={}&end_date={}&per_page=100{}",
        window.window().start_timestamp(),
        window.window().end_timestamp(),
        context_codes_query(batch)
    )
}

/// Convert calendar events into an ingest page.
#[must_use]
pub fn calendar_events_to_ingest_page(
    items: &[CalendarEvent],
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items.iter().map(calendar_event_to_entity).collect(),
    }
}

/// Convert one calendar event into an entity ingest row.
#[must_use]
pub fn calendar_event_to_entity(item: &CalendarEvent) -> EntityIngest {
    let mut fields = Vec::new();
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        item.title.as_deref(),
    );
    push_opt_ts(&mut fields, "start_at", FieldGroup::Core, item.start_at);
    push_opt_ts(&mut fields, "end_at", FieldGroup::Core, item.end_at);
    push_opt_str(
        &mut fields,
        "context_code",
        FieldGroup::Core,
        item.context_code.as_deref(),
    );
    push_opt_bool(&mut fields, "all_day", FieldGroup::Core, item.all_day);
    push_opt_str(
        &mut fields,
        "all_day_date",
        FieldGroup::Core,
        item.all_day_date.map(|d| d.to_string()).as_deref(),
    );
    push_api_str(
        &mut fields,
        "description",
        FieldGroup::Detail,
        &item.description,
    );
    push_opt_str(
        &mut fields,
        "location_name",
        FieldGroup::Detail,
        item.location_name.as_deref(),
    );
    push_api_str(
        &mut fields,
        "workflow_state",
        FieldGroup::Status,
        &item.workflow_state,
    );
    push_opt_bool(&mut fields, "hidden", FieldGroup::Status, item.hidden);
    push_api_to_string(&mut fields, "html_url", FieldGroup::Detail, &item.html_url);
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

const COLUMN_FIELDS: &[&str] = &["title", "start_at", "end_at", "context_code"];

const EXTRA_FIELDS: &[&str] = &[
    "all_day",
    "all_day_date",
    "description",
    "location_name",
    "workflow_state",
    "hidden",
    "html_url",
];

fn upsert_calendar_event(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("calendar_event", &entity.entity_key)?;
    let mut columns = Vec::new();
    let mut extra = Map::new();
    for field in &entity.fields {
        if COLUMN_FIELDS.contains(&field.name) {
            columns.push(field.clone());
        } else if EXTRA_FIELDS.contains(&field.name) {
            extra.insert(
                field.name.to_owned(),
                match (&field.value, field.name) {
                    (None, _) => Value::Null,
                    (Some(v), "all_day" | "hidden") => Value::Bool(v == "true" || v == "1"),
                    (Some(v), _) => Value::String(v.clone()),
                },
            );
        } else {
            return Err(DbError::Message(format!(
                "unsupported calendar event field: {}",
                field.name
            ))
            .into());
        }
    }
    validate_fields(&columns, COLUMN_FIELDS)?;
    tx.execute(
        "INSERT INTO calendar_events (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(
        tx,
        "calendar_event",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    for field in &columns {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        tx.execute(
            &format!(
                "UPDATE calendar_events SET {} = ?1 WHERE id = ?2",
                field.name
            ),
            params![field.value, id],
        )?;
    }
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "calendar_events", id, extra)?;
    touch_observed(
        tx,
        "calendar_events",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

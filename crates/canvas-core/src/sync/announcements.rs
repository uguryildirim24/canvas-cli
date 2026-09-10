//! `announcements` dataset (`window:<start>..<end>:ctx:<sha256>`, §12.6).

use canvas_api::models::Announcement;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::context_window::{ContextWindow, context_codes_query};
use super::fields::{push_api_to_string, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_announcements` = 15 minutes.
#[must_use]
pub fn default_ttl_announcements() -> Span {
    Span::new().minutes(15)
}

/// Announcements for a window over a set of courses.
#[derive(Debug, Clone)]
pub struct AnnouncementsDataset {
    pub window: ContextWindow,
    pub ttl: Span,
}

impl AnnouncementsDataset {
    #[must_use]
    pub fn new(window: ContextWindow, ttl: Span) -> Self {
        Self { window, ttl }
    }

    #[must_use]
    pub fn with_default_ttl(window: ContextWindow) -> Self {
        Self::new(window, default_ttl_announcements())
    }
}

impl Dataset for AnnouncementsDataset {
    fn name(&self) -> &'static str {
        "announcements"
    }

    fn scope_key(&self) -> &str {
        self.window.scope_key()
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "announcement"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_announcement(tx, entity, fetched_at)
    }
}

/// Listing path for one batch of at most ten contexts.
#[must_use]
pub fn announcements_path(batch: &[String], window: &ContextWindow) -> String {
    format!(
        "/api/v1/announcements?start_date={}&end_date={}&per_page=100{}",
        window.window().start_timestamp(),
        window.window().end_timestamp(),
        context_codes_query(batch)
    )
}

/// Detail path for one announcement (§12.6).
#[must_use]
pub fn announcement_path(course_id: i64, id: i64) -> String {
    format!("/api/v1/courses/{course_id}/discussion_topics/{id}")
}

/// Convert announcements into an ingest page.
#[must_use]
pub fn announcements_to_ingest_page(items: &[Announcement], fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items.iter().map(announcement_to_entity).collect(),
    }
}

/// Course id carried by a `course_<id>` context code.
#[must_use]
pub fn course_id_from_context(code: &str) -> Option<i64> {
    code.strip_prefix("course_").and_then(|id| id.parse().ok())
}

/// Convert one announcement into an entity ingest row.
///
/// The listing has no `course_id`; it comes from `context_code`.
#[must_use]
pub fn announcement_to_entity(item: &Announcement) -> EntityIngest {
    let mut fields = Vec::new();
    if let Some(course_id) = item
        .context_code
        .as_deref()
        .and_then(course_id_from_context)
    {
        fields.push(FieldWrite {
            name: "course_id",
            group: FieldGroup::Core,
            value: Some(course_id.to_string()),
        });
    }
    push_opt_str(
        &mut fields,
        "context_code",
        FieldGroup::Core,
        item.context_code.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        item.title.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "message",
        FieldGroup::Detail,
        item.message.as_deref(),
    );
    push_opt_ts(&mut fields, "posted_at", FieldGroup::Core, item.posted_at);
    push_opt_ts(
        &mut fields,
        "delayed_post_at",
        FieldGroup::Core,
        item.delayed_post_at,
    );
    push_opt_str(&mut fields, "author", FieldGroup::Core, author_name(item));
    // `read_state` is per-user status, never merged with the listing body.
    push_opt_str(
        &mut fields,
        "read_state",
        FieldGroup::Status,
        item.read_state.as_deref(),
    );
    push_api_to_string(&mut fields, "html_url", FieldGroup::Detail, &item.html_url);
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

/// Display name of the author, from `user_name` or the `author` object.
fn author_name(item: &Announcement) -> Option<&str> {
    if let Some(name) = item.user_name.as_deref().filter(|s| !s.is_empty()) {
        return Some(name);
    }
    item.author
        .as_ref()
        .and_then(|a| a.get("display_name").or_else(|| a.get("name")))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

const COLUMN_FIELDS: &[&str] = &["course_id", "title", "message", "posted_at"];

const EXTRA_FIELDS: &[&str] = &[
    "context_code",
    "delayed_post_at",
    "author",
    "read_state",
    "html_url",
];

fn upsert_announcement(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("announcement", &entity.entity_key)?;
    let mut columns = Vec::new();
    let mut extra = Map::new();
    for field in &entity.fields {
        if COLUMN_FIELDS.contains(&field.name) {
            columns.push(field.clone());
        } else if EXTRA_FIELDS.contains(&field.name) {
            extra.insert(
                field.name.to_owned(),
                match &field.value {
                    Some(v) => Value::String(v.clone()),
                    None => Value::Null,
                },
            );
        } else {
            return Err(DbError::Message(format!(
                "unsupported announcement field: {}",
                field.name
            ))
            .into());
        }
    }
    validate_fields(&columns, COLUMN_FIELDS)?;
    tx.execute(
        "INSERT INTO announcements (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(
        tx,
        "announcement",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    for field in &columns {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        if field.name == "course_id" {
            let value: Option<i64> = field
                .value
                .as_ref()
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: std::num::ParseIntError| {
                    DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                })?;
            tx.execute(
                "UPDATE announcements SET course_id = ?1 WHERE id = ?2",
                params![value, id],
            )?;
        } else {
            tx.execute(
                &format!("UPDATE announcements SET {} = ?1 WHERE id = ?2", field.name),
                params![field.value, id],
            )?;
        }
    }
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "announcements", id, extra)?;
    touch_observed(
        tx,
        "announcements",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

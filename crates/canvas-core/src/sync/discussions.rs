//! `discussions` and `discussion` datasets (§10, M8-a).
//!
//! Replies are covered by their own scope, `topic:<id>:replies`, so a topic
//! read that did not ask for replies can never make a reply-set look covered.
//! The materialized `/view` endpoint is never used: it marks entries read.

use canvas_api::models::{DiscussionEntry, DiscussionTopic};
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_opt_bool, push_opt_i64, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_discussions` = 15 minutes.
#[must_use]
pub fn default_ttl_discussions() -> Span {
    Span::new().minutes(15)
}

/// Why a reply set stayed empty or short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepliesBlock {
    /// The topic requires an initial post from this user before it shows any.
    InitialPostRequired,
    /// A reply page failed after earlier pages were stored.
    PageFailed,
}

impl RepliesBlock {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitialPostRequired => "initial_post_required",
            Self::PageFailed => "page_failed",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "initial_post_required" => Some(Self::InitialPostRequired),
            "page_failed" => Some(Self::PageFailed),
            _ => None,
        }
    }
}

/// Coverage of one reply set, recorded on the topic row so a cached read
/// reports the same truth as the read that fetched it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepliesCoverage {
    pub pages_fetched: u32,
    pub complete: bool,
    pub blocked: Option<RepliesBlock>,
}

/// Discussion topics listing for one course.
#[derive(Debug, Clone)]
pub struct DiscussionsDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl DiscussionsDataset {
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
        Self::new(course_id, default_ttl_discussions())
    }
}

impl Dataset for DiscussionsDataset {
    fn name(&self) -> &'static str {
        "discussions"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "discussion_topic"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_topic(tx, self.course_id, entity, fetched_at)
    }
}

/// One discussion topic, with or without its reply set.
#[derive(Debug, Clone)]
pub struct DiscussionDataset {
    pub course_id: i64,
    pub topic_id: i64,
    pub with_replies: bool,
    ttl: Span,
    scope: String,
}

impl DiscussionDataset {
    #[must_use]
    pub fn new(course_id: i64, topic_id: i64, with_replies: bool, ttl: Span) -> Self {
        let scope = if with_replies {
            format!("topic:{topic_id}:replies")
        } else {
            format!("topic:{topic_id}")
        };
        Self {
            course_id,
            topic_id,
            with_replies,
            ttl,
            scope,
        }
    }
}

impl Dataset for DiscussionDataset {
    fn name(&self) -> &'static str {
        "discussion"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "discussion_topic"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_topic(tx, self.course_id, entity, fetched_at)
    }
}

/// Listing path for course discussion topics.
#[must_use]
pub fn discussions_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/discussion_topics?only_announcements=false")
}

/// Detail path for one topic.
#[must_use]
pub fn discussion_path(course_id: i64, topic_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}")
}

/// Top-level entries of one topic.
#[must_use]
pub fn discussion_entries_path(course_id: i64, topic_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries")
}

/// Replies nested under one entry.
#[must_use]
pub fn discussion_replies_path(course_id: i64, topic_id: i64, entry_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/discussion_topics/{topic_id}/entries/{entry_id}/replies")
}

/// Convert topics into an ingest page.
#[must_use]
pub fn discussions_to_ingest_page(
    items: &[DiscussionTopic],
    course_id: i64,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items
            .iter()
            .map(|t| topic_to_entity(t, course_id))
            .collect(),
    }
}

/// Convert one topic into an entity ingest row.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn topic_to_entity(item: &DiscussionTopic, course_id: i64) -> EntityIngest {
    let mut fields = vec![FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    }];
    push_opt_str(&mut fields, "title", FieldGroup::Core, item.title.as_deref());
    push_opt_str(
        &mut fields,
        "message",
        FieldGroup::Detail,
        item.message.as_deref(),
    );
    push_opt_ts(&mut fields, "posted_at", FieldGroup::Core, item.posted_at);
    push_opt_ts(
        &mut fields,
        "last_reply_at",
        FieldGroup::Core,
        item.last_reply_at,
    );
    push_opt_ts(
        &mut fields,
        "delayed_post_at",
        FieldGroup::Core,
        item.delayed_post_at,
    );
    push_opt_str(
        &mut fields,
        "discussion_type",
        FieldGroup::Core,
        item.discussion_type.as_deref(),
    );
    push_opt_str(&mut fields, "author", FieldGroup::Core, author_name(item));
    push_opt_str(
        &mut fields,
        "context_code",
        FieldGroup::Core,
        item.context_code.as_deref(),
    );
    // Per-user state stays in its own group so a listing never overwrites it
    // with a value read for another user or at another time.
    push_opt_str(
        &mut fields,
        "read_state",
        FieldGroup::Status,
        item.read_state.as_deref(),
    );
    if let Some(n) = item.unread_count {
        fields.push(FieldWrite {
            name: "unread_count",
            group: FieldGroup::Status,
            value: Some(n.to_string()),
        });
    }
    if let Some(n) = item.discussion_subentry_count {
        fields.push(FieldWrite {
            name: "discussion_subentry_count",
            group: FieldGroup::Core,
            value: Some(n.to_string()),
        });
    }
    push_opt_bool(&mut fields, "published", FieldGroup::Status, item.published);
    push_opt_bool(&mut fields, "locked", FieldGroup::Status, item.locked);
    push_opt_bool(
        &mut fields,
        "locked_for_user",
        FieldGroup::Status,
        item.locked_for_user,
    );
    push_opt_bool(&mut fields, "pinned", FieldGroup::Core, item.pinned);
    push_opt_bool(
        &mut fields,
        "require_initial_post",
        FieldGroup::Core,
        item.require_initial_post,
    );
    push_opt_bool(
        &mut fields,
        "user_can_see_posts",
        FieldGroup::Status,
        item.user_can_see_posts,
    );
    push_opt_bool(
        &mut fields,
        "is_announcement",
        FieldGroup::Core,
        item.is_announcement,
    );
    push_opt_bool(
        &mut fields,
        "subscribed",
        FieldGroup::Status,
        item.subscribed,
    );
    push_opt_i64(
        &mut fields,
        "assignment_id",
        FieldGroup::Core,
        item.assignment_id,
    );
    if let Some(points) = item.points_possible {
        fields.push(FieldWrite {
            name: "points_possible",
            group: FieldGroup::Core,
            value: Some(points.to_string()),
        });
    }
    push_opt_i64(
        &mut fields,
        "group_category_id",
        FieldGroup::Core,
        item.group_category_id,
    );
    if !item.group_topic_children.is_empty() {
        let children: Vec<Value> = item
            .group_topic_children
            .iter()
            .map(|c| json!({ "id": c.id, "group_id": c.group_id }))
            .collect();
        fields.push(FieldWrite {
            name: "group_topic_children",
            group: FieldGroup::Core,
            value: Some(Value::Array(children).to_string()),
        });
    }
    if let Some(url) = item.html_url.as_ref() {
        fields.push(FieldWrite {
            name: "html_url",
            group: FieldGroup::Detail,
            value: Some(url.to_string()),
        });
    }
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

/// Attach a fetched reply set and its coverage to a topic entity.
///
/// The payload is stripped before the field-write rule runs, exactly as the
/// modules dataset does with its inline items.
#[must_use]
pub fn attach_replies(
    mut entity: EntityIngest,
    topic_id: i64,
    entries: &[DiscussionEntry],
    coverage: RepliesCoverage,
) -> EntityIngest {
    let payload: Vec<Value> = entries
        .iter()
        .map(|e| {
            let child = entry_to_entity(e, topic_id);
            json!({
                "entity_key": child.entity_key,
                "fields": child
                    .fields
                    .iter()
                    .map(|f| (f.name.to_owned(), f.value.clone().map_or(Value::Null, Value::String)))
                    .collect::<Map<String, Value>>(),
            })
        })
        .collect();
    entity.fields.push(FieldWrite {
        name: "entries_payload",
        group: FieldGroup::Detail,
        value: Some(Value::Array(payload).to_string()),
    });
    entity.fields.push(FieldWrite {
        name: "replies_pages_fetched",
        group: FieldGroup::Detail,
        value: Some(coverage.pages_fetched.to_string()),
    });
    entity.fields.push(FieldWrite {
        name: "replies_complete",
        group: FieldGroup::Detail,
        value: Some(if coverage.complete { "true" } else { "false" }.to_owned()),
    });
    entity.fields.push(FieldWrite {
        name: "replies_blocked",
        group: FieldGroup::Detail,
        value: coverage.blocked.map(|b| b.as_str().to_owned()),
    });
    entity
}

/// Convert one entry (or one nested reply) into an entity ingest row.
#[must_use]
pub fn entry_to_entity(item: &DiscussionEntry, topic_id: i64) -> EntityIngest {
    let mut fields = vec![FieldWrite {
        name: "topic_id",
        group: FieldGroup::Core,
        value: Some(topic_id.to_string()),
    }];
    push_opt_i64(&mut fields, "parent_id", FieldGroup::Core, item.parent_id);
    push_opt_i64(&mut fields, "user_id", FieldGroup::Core, item.user_id);
    push_opt_str(
        &mut fields,
        "user_name",
        FieldGroup::Core,
        item.user_name.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "message",
        FieldGroup::Detail,
        item.message.as_deref(),
    );
    push_opt_ts(&mut fields, "created_at", FieldGroup::Core, item.created_at);
    push_opt_ts(
        &mut fields,
        "updated_at",
        FieldGroup::Detail,
        item.updated_at,
    );
    push_opt_str(
        &mut fields,
        "read_state",
        FieldGroup::Status,
        item.read_state.as_deref(),
    );
    fields.push(FieldWrite {
        name: "replies_count",
        group: FieldGroup::Core,
        value: Some(item.replies_count().to_string()),
    });
    push_opt_bool(
        &mut fields,
        "has_more_replies",
        FieldGroup::Core,
        item.has_more_replies,
    );
    push_opt_bool(&mut fields, "deleted", FieldGroup::Status, item.deleted);
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

fn author_name(item: &DiscussionTopic) -> Option<&str> {
    if let Some(name) = item.user_name.as_deref().filter(|s| !s.is_empty()) {
        return Some(name);
    }
    item.author
        .as_ref()
        .and_then(|a| a.get("display_name").or_else(|| a.get("name")))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

const TOPIC_COLUMNS: &[&str] = &["course_id", "title", "message", "posted_at"];

const TOPIC_EXTRA: &[&str] = &[
    "last_reply_at",
    "delayed_post_at",
    "discussion_type",
    "author",
    "context_code",
    "read_state",
    "unread_count",
    "discussion_subentry_count",
    "published",
    "locked",
    "locked_for_user",
    "pinned",
    "require_initial_post",
    "user_can_see_posts",
    "is_announcement",
    "subscribed",
    "assignment_id",
    "points_possible",
    "group_category_id",
    "group_topic_children",
    "html_url",
    "replies_pages_fetched",
    "replies_complete",
    "replies_blocked",
];

const ENTRY_COLUMNS: &[&str] = &["topic_id", "parent_id", "user_id", "message", "created_at"];

const ENTRY_EXTRA: &[&str] = &[
    "user_name",
    "updated_at",
    "read_state",
    "replies_count",
    "has_more_replies",
    "deleted",
];

fn split_fields(
    kind: &str,
    fields: &[FieldWrite],
    columns: &[&str],
    extras: &[&str],
) -> Result<(Vec<FieldWrite>, Map<String, Value>), IngestError> {
    let mut column_fields = Vec::new();
    let mut extra = Map::new();
    for field in fields {
        if columns.contains(&field.name) {
            column_fields.push(field.clone());
        } else if extras.contains(&field.name) {
            extra.insert(
                field.name.to_owned(),
                match &field.value {
                    Some(v) => Value::String(v.clone()),
                    None => Value::Null,
                },
            );
        } else {
            return Err(
                DbError::Message(format!("unsupported {kind} field: {}", field.name)).into(),
            );
        }
    }
    Ok((column_fields, extra))
}

fn apply_columns(
    tx: &Transaction<'_>,
    table: &str,
    id: i64,
    columns: &[FieldWrite],
    applied: &[&str],
    numeric: &[&str],
) -> Result<(), IngestError> {
    for field in columns {
        if !applied.contains(&field.name) {
            continue;
        }
        if numeric.contains(&field.name) {
            let value: Option<i64> = field
                .value
                .as_ref()
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: std::num::ParseIntError| {
                    DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                })?;
            tx.execute(
                &format!("UPDATE {table} SET {} = ?1 WHERE id = ?2", field.name),
                params![value, id],
            )?;
        } else {
            tx.execute(
                &format!("UPDATE {table} SET {} = ?1 WHERE id = ?2", field.name),
                params![field.value, id],
            )?;
        }
    }
    Ok(())
}

fn upsert_topic(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("discussion_topic", &entity.entity_key)?;
    let payload = entity
        .fields
        .iter()
        .find(|f| f.name == "entries_payload")
        .and_then(|f| f.value.clone());
    let real_fields: Vec<FieldWrite> = entity
        .fields
        .iter()
        .filter(|f| f.name != "entries_payload")
        .cloned()
        .collect();
    let (columns, extra) = split_fields(
        "discussion topic",
        &real_fields,
        TOPIC_COLUMNS,
        TOPIC_EXTRA,
    )?;
    validate_fields(&columns, TOPIC_COLUMNS)?;
    tx.execute(
        "INSERT INTO discussion_topics (id, course_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, dataset_course_id],
    )?;
    let applied = apply_field_writes(
        tx,
        "discussion_topic",
        &entity.entity_key,
        fetched_at,
        &real_fields,
    )?;
    apply_columns(
        tx,
        "discussion_topics",
        id,
        &columns,
        &applied.fields,
        &["course_id"],
    )?;
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "discussion_topics", id, extra)?;
    touch_observed(
        tx,
        "discussion_topics",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    if let Some(payload) = payload {
        write_entries(tx, &payload, id, fetched_at)?;
    }
    Ok(())
}

/// Store a fetched reply set and replace its membership under `topic:<id>`.
fn write_entries(
    tx: &Transaction<'_>,
    payload: &str,
    topic_id: i64,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let entries: Vec<Value> = serde_json::from_str(payload).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    let scope = format!("topic:{topic_id}");
    tx.execute(
        "DELETE FROM membership WHERE dataset = 'discussion_entries' AND scope = ?1",
        params![scope],
    )?;
    for (position, item) in entries.into_iter().enumerate() {
        let key = item
            .get("entity_key")
            .and_then(Value::as_str)
            .ok_or_else(|| DbError::Message("entries payload missing entity_key".into()))?
            .to_owned();
        tx.execute(
            "INSERT OR IGNORE INTO membership (dataset, scope, entity_kind, entity_id, position)
             VALUES ('discussion_entries', ?1, 'discussion_entry', ?2, ?3)",
            params![scope, key, i64::try_from(position).unwrap_or(i64::MAX)],
        )?;
        let map = item
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| DbError::Message("entries payload missing fields".into()))?;
        let mut fields = Vec::new();
        for (name, value) in map {
            let Some(static_name) = entry_field_name(name) else {
                return Err(DbError::Message(format!("unsupported entry field: {name}")).into());
            };
            let group = entry_field_group(static_name);
            fields.push(FieldWrite {
                name: static_name,
                group,
                value: match value {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                },
            });
        }
        upsert_entry(
            tx,
            &EntityIngest {
                entity_key: key,
                fields,
            },
            topic_id,
            fetched_at,
        )?;
    }
    Ok(())
}

fn entry_field_name(name: &str) -> Option<&'static str> {
    ENTRY_COLUMNS
        .iter()
        .chain(ENTRY_EXTRA.iter())
        .find(|known| **known == name)
        .copied()
}

fn entry_field_group(name: &str) -> FieldGroup {
    match name {
        "message" | "updated_at" => FieldGroup::Detail,
        "read_state" | "deleted" => FieldGroup::Status,
        _ => FieldGroup::Core,
    }
}

fn upsert_entry(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    topic_id: i64,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("discussion_entry", &entity.entity_key)?;
    let (columns, extra) = split_fields(
        "discussion entry",
        &entity.fields,
        ENTRY_COLUMNS,
        ENTRY_EXTRA,
    )?;
    validate_fields(&columns, ENTRY_COLUMNS)?;
    tx.execute(
        "INSERT INTO discussion_entries (id, topic_id) VALUES (?1, ?2)
         ON CONFLICT(id) DO NOTHING",
        params![id, topic_id],
    )?;
    let applied = apply_field_writes(
        tx,
        "discussion_entry",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    apply_columns(
        tx,
        "discussion_entries",
        id,
        &columns,
        &applied.fields,
        &["topic_id", "parent_id", "user_id"],
    )?;
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "discussion_entries", id, extra)?;
    touch_observed(
        tx,
        "discussion_entries",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

/// Refresh the `discussions` listing for one course.
pub async fn refresh_discussions(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use futures_util::StreamExt;

    use super::refresh::{FetchBundle, refresh_listing_with_denial};

    let dataset = DiscussionsDataset::new(course_id, ttl);
    refresh_listing_with_denial(client, store, &dataset, now, fresh, offline, || async {
        let path = discussions_path(course_id);
        let mut items: Vec<DiscussionTopic> = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<DiscussionTopic>(&path));
        while let Some(page) = stream.next().await {
            items.extend(page?.items);
        }
        Ok(FetchBundle {
            pages: vec![discussions_to_ingest_page(&items, course_id, now)],
        })
    })
    .await
}

/// Refresh one topic, and its reply set when `with_replies` is set.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_discussion(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    topic_id: i64,
    with_replies: bool,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = DiscussionDataset::new(course_id, topic_id, with_replies, ttl);
    refresh_dataset(
        client,
        store,
        &dataset,
        now,
        fresh,
        offline,
        None,
        None,
        || async {
            let topic: DiscussionTopic = client.get(&discussion_path(course_id, topic_id)).await?;
            if topic.id != topic_id {
                return Err(canvas_api::Error::Decode.into());
            }
            let mut entity = topic_to_entity(&topic, course_id);
            if with_replies {
                let gated = topic.require_initial_post.unwrap_or(false);
                let (entries, coverage) =
                    fetch_replies(client, course_id, topic_id, gated).await?;
                entity = attach_replies(entity, topic_id, &entries, coverage);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![entity],
                }],
            })
        },
    )
    .await
}

/// Fetch every reply page, stopping at the first failure.
///
/// A gate (`require_initial_post`) and a mid-set page failure are both
/// recorded as coverage, never as a complete thread.
async fn fetch_replies(
    client: &canvas_api::Client,
    course_id: i64,
    topic_id: i64,
    gated: bool,
) -> Result<(Vec<DiscussionEntry>, RepliesCoverage), super::SyncError> {
    use futures_util::StreamExt;

    let mut out: Vec<DiscussionEntry> = Vec::new();
    let mut pages_fetched = 0u32;
    let path = discussion_entries_path(course_id, topic_id);
    let mut stream = std::pin::pin!(client.get_all::<DiscussionEntry>(&path));
    while let Some(page) = stream.next().await {
        match page {
            Ok(page) => {
                pages_fetched = pages_fetched.saturating_add(1);
                out.extend(page.items);
            }
            Err(err) => {
                return Ok((out, blocked_coverage(err, pages_fetched, gated)?));
            }
        }
    }

    // Nested replies arrive inline until Canvas truncates them; only then does
    // a second route become necessary.
    let deeper: Vec<i64> = out
        .iter()
        .filter(|e| e.has_more_replies.unwrap_or(false))
        .map(|e| e.id)
        .collect();
    let mut nested = Vec::new();
    for entry_id in deeper {
        let path = discussion_replies_path(course_id, topic_id, entry_id);
        let mut stream = std::pin::pin!(client.get_all::<DiscussionEntry>(&path));
        while let Some(page) = stream.next().await {
            match page {
                Ok(page) => {
                    pages_fetched = pages_fetched.saturating_add(1);
                    nested.extend(page.items);
                }
                Err(err) => {
                    out.extend(nested);
                    return Ok((out, blocked_coverage(err, pages_fetched, gated)?));
                }
            }
        }
    }
    for entry in &out {
        for reply in &entry.recent_replies {
            if !nested.iter().any(|e| e.id == reply.id) {
                nested.push(reply.clone());
            }
        }
    }
    out.extend(nested);
    Ok((
        out,
        RepliesCoverage {
            pages_fetched,
            complete: true,
            blocked: None,
        },
    ))
}

/// Classify a reply-page failure, keeping auth and throttle failures fatal.
///
/// Only a refusal on a topic that requires an initial post is reported as the
/// gate; any other refusal is an ordinary incomplete page set.
fn blocked_coverage(
    err: canvas_api::Error,
    pages_fetched: u32,
    gated: bool,
) -> Result<RepliesCoverage, super::SyncError> {
    use canvas_api::Error as Api;
    if matches!(
        err,
        Api::Unauthorized
            | Api::RateLimited
            | Api::Forbidden {
                rate_limited: true,
                ..
            }
    ) {
        return Err(err.into());
    }
    let refused = matches!(
        err,
        Api::Forbidden {
            rate_limited: false,
            ..
        } | Api::Denied { status: 403 }
    );
    let blocked = if refused && gated {
        RepliesBlock::InitialPostRequired
    } else {
        RepliesBlock::PageFailed
    };
    Ok(RepliesCoverage {
        pages_fetched,
        complete: false,
        blocked: Some(blocked),
    })
}

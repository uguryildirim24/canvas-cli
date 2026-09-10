//! `inbox`, `conversation`, and `inbox_unread` datasets (§10, M8-a).
//!
//! Every request carries `auto_mark_as_read=false`. Canvas marks a
//! conversation read on a plain detail fetch, and this package reads only.

use canvas_api::models::{Conversation, ConversationMessage, UnreadCount};
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_opt_bool, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_inbox` = 5 minutes.
#[must_use]
pub fn default_ttl_inbox() -> Span {
    Span::new().minutes(5)
}

/// The single cache row that holds the unread count for this identity.
pub const UNREAD_ROW_ID: i64 = 1;

/// Which conversation list Canvas should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxScope {
    Inbox,
    Unread,
    Sent,
    Archived,
}

impl InboxScope {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Unread => "unread",
            Self::Sent => "sent",
            Self::Archived => "archived",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "inbox" => Some(Self::Inbox),
            "unread" => Some(Self::Unread),
            "sent" => Some(Self::Sent),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Conversation listing for one scope.
#[derive(Debug, Clone)]
pub struct InboxDataset {
    pub scope: InboxScope,
    pub ttl: Span,
    scope_key: String,
}

impl InboxDataset {
    #[must_use]
    pub fn new(scope: InboxScope, ttl: Span) -> Self {
        Self {
            scope,
            ttl,
            scope_key: format!("scope:{}", scope.as_str()),
        }
    }
}

impl Dataset for InboxDataset {
    fn name(&self) -> &'static str {
        "inbox"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "conversation"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_conversation(tx, entity, fetched_at)
    }
}

/// One conversation with its messages.
#[derive(Debug, Clone)]
pub struct ConversationDataset {
    pub id: i64,
    ttl: Span,
    scope: String,
}

impl ConversationDataset {
    #[must_use]
    pub fn new(id: i64, ttl: Span) -> Self {
        Self {
            id,
            ttl,
            scope: format!("conversation:{id}"),
        }
    }
}

impl Dataset for ConversationDataset {
    fn name(&self) -> &'static str {
        "conversation"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "conversation"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_conversation(tx, entity, fetched_at)
    }
}

/// The unread-conversation count for the active identity.
#[derive(Debug, Clone)]
pub struct InboxUnreadDataset {
    ttl: Span,
}

impl InboxUnreadDataset {
    #[must_use]
    pub fn new(ttl: Span) -> Self {
        Self { ttl }
    }
}

impl Dataset for InboxUnreadDataset {
    fn name(&self) -> &'static str {
        "inbox_unread"
    }

    fn scope_key(&self) -> &'static str {
        "all"
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "inbox_unread"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_unread(tx, entity, fetched_at)
    }
}

/// Listing path for one inbox scope; never marks anything read.
#[must_use]
pub fn inbox_path(scope: InboxScope) -> String {
    format!(
        "/api/v1/conversations?scope={}&auto_mark_as_read=false",
        scope.as_str()
    )
}

/// Detail path for one conversation; never marks it read.
#[must_use]
pub fn conversation_path(id: i64) -> String {
    format!("/api/v1/conversations/{id}?auto_mark_as_read=false")
}

/// Unread-count path.
#[must_use]
pub fn unread_count_path() -> String {
    "/api/v1/conversations/unread_count".to_owned()
}

/// Convert conversations into an ingest page.
#[must_use]
pub fn conversations_to_ingest_page(items: &[Conversation], fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items.iter().map(conversation_to_entity).collect(),
    }
}

/// Convert one conversation into an entity ingest row.
///
/// `messages` arrive only from the detail route, so the listing leaves the
/// field absent and a cached message set survives a listing refresh.
#[must_use]
pub fn conversation_to_entity(item: &Conversation) -> EntityIngest {
    let mut fields = Vec::new();
    push_opt_str(
        &mut fields,
        "subject",
        FieldGroup::Core,
        item.subject.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "workflow_state",
        FieldGroup::Status,
        item.workflow_state.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "last_message",
        FieldGroup::Detail,
        item.last_message.as_deref(),
    );
    push_opt_ts(
        &mut fields,
        "last_message_at",
        FieldGroup::Core,
        item.last_message_at,
    );
    if let Some(n) = item.message_count {
        fields.push(FieldWrite {
            name: "message_count",
            group: FieldGroup::Core,
            value: Some(n.to_string()),
        });
    }
    push_opt_bool(
        &mut fields,
        "subscribed",
        FieldGroup::Status,
        item.subscribed,
    );
    push_opt_bool(&mut fields, "private", FieldGroup::Core, item.private);
    push_opt_bool(&mut fields, "starred", FieldGroup::Status, item.starred);
    push_opt_str(
        &mut fields,
        "context_name",
        FieldGroup::Core,
        item.context_name.as_deref(),
    );
    if !item.participants.is_empty() {
        let participants: Vec<Value> = item
            .participants
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name.clone().or_else(|| p.full_name.clone()),
                })
            })
            .collect();
        fields.push(FieldWrite {
            name: "participants",
            group: FieldGroup::Core,
            value: Some(Value::Array(participants).to_string()),
        });
    }
    if !item.messages.is_empty() {
        fields.push(FieldWrite {
            name: "messages",
            group: FieldGroup::Detail,
            value: Some(messages_json(&item.messages).to_string()),
        });
    }
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

fn messages_json(messages: &[ConversationMessage]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "author_id": m.author_id,
                    "created_at": m.created_at.map(|at| at.to_string()),
                    "body": m.body,
                    "generated": m.generated,
                    "attachments": m
                        .attachments
                        .iter()
                        .map(|a| json!({
                            "file_id": a.id,
                            "name": a.display_name.clone().or_else(|| a.filename.clone()),
                            "size": a.size,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

const CONVERSATION_COLUMNS: &[&str] = &["subject", "workflow_state", "last_message_at"];

const CONVERSATION_EXTRA: &[&str] = &[
    "last_message",
    "message_count",
    "subscribed",
    "private",
    "starred",
    "context_name",
    "participants",
    "messages",
];

fn upsert_conversation(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("conversation", &entity.entity_key)?;
    let mut columns = Vec::new();
    let mut extra = Map::new();
    for field in &entity.fields {
        if CONVERSATION_COLUMNS.contains(&field.name) {
            columns.push(field.clone());
        } else if CONVERSATION_EXTRA.contains(&field.name) {
            extra.insert(
                field.name.to_owned(),
                match &field.value {
                    Some(v) => Value::String(v.clone()),
                    None => Value::Null,
                },
            );
        } else {
            return Err(DbError::Message(format!(
                "unsupported conversation field: {}",
                field.name
            ))
            .into());
        }
    }
    validate_fields(&columns, CONVERSATION_COLUMNS)?;
    tx.execute(
        "INSERT INTO conversations (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(
        tx,
        "conversation",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    for field in &columns {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        tx.execute(
            &format!("UPDATE conversations SET {} = ?1 WHERE id = ?2", field.name),
            params![field.value, id],
        )?;
    }
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "conversations", id, extra)?;
    touch_observed(
        tx,
        "conversations",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

/// Convert the unread count into an entity ingest row.
///
/// Canvas reports the count as a string; a value it does not report stays
/// unknown rather than becoming zero.
#[must_use]
pub fn unread_to_entity(item: &UnreadCount) -> EntityIngest {
    let mut fields = Vec::new();
    let parsed = item
        .unread_count
        .as_deref()
        .and_then(|raw| raw.trim().parse::<i64>().ok());
    fields.push(FieldWrite {
        name: "unread_count",
        group: FieldGroup::Status,
        value: parsed.map(|n| n.to_string()),
    });
    EntityIngest {
        entity_key: UNREAD_ROW_ID.to_string(),
        fields,
    }
}

fn upsert_unread(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("inbox_unread", &entity.entity_key)?;
    validate_fields(&entity.fields, &["unread_count"])?;
    tx.execute(
        "INSERT INTO conversation_unread (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let applied = apply_field_writes(
        tx,
        "inbox_unread",
        &entity.entity_key,
        fetched_at,
        &entity.fields,
    )?;
    if applied.fields.contains(&"unread_count") {
        let value: Option<i64> = entity
            .fields
            .iter()
            .find(|f| f.name == "unread_count")
            .and_then(|f| f.value.as_ref())
            .map(|s| s.parse())
            .transpose()
            .map_err(|e: std::num::ParseIntError| {
                DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;
        tx.execute(
            "UPDATE conversation_unread SET unread_count = ?1 WHERE id = ?2",
            params![value, id],
        )?;
    }
    touch_observed(
        tx,
        "conversation_unread",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

/// Refresh the conversation listing for one scope.
pub async fn refresh_inbox(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    scope: InboxScope,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use futures_util::StreamExt;

    use super::refresh::{FetchBundle, refresh_listing_with_denial};

    let dataset = InboxDataset::new(scope, ttl);
    refresh_listing_with_denial(client, store, &dataset, now, fresh, offline, || async {
        let path = inbox_path(scope);
        let mut items: Vec<Conversation> = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<Conversation>(&path));
        while let Some(page) = stream.next().await {
            items.extend(page?.items);
        }
        Ok(FetchBundle {
            pages: vec![conversations_to_ingest_page(&items, now)],
        })
    })
    .await
}

/// Refresh one conversation by id.
pub async fn refresh_conversation(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = ConversationDataset::new(id, ttl);
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
            let item: Conversation = client.get(&conversation_path(id)).await?;
            if item.id != id {
                return Err(canvas_api::Error::Decode.into());
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![conversation_to_entity(&item)],
                }],
            })
        },
    )
    .await
}

/// Refresh the unread-conversation count.
pub async fn refresh_inbox_unread(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = InboxUnreadDataset::new(ttl);
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
            let item: UnreadCount = client.get(&unread_count_path()).await?;
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![unread_to_entity(&item)],
                }],
            })
        },
    )
    .await
}

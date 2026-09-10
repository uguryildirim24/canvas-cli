//! `canvas inbox`, `inbox show`, and `inbox unread-count` (class C, M8-a).
//!
//! Every request carries `auto_mark_as_read=false`; this package never marks
//! a conversation read.

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::markdown::BODY_LIMIT;
use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{
    ConversationDataset, InboxDataset, InboxScope, InboxUnreadDataset, RefreshOutcome,
    UNREAD_ROW_ID, listing_denial_status, refresh_conversation, refresh_inbox,
    refresh_inbox_unread,
};
use comfy_table::Row;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::Globals;
use super::course::refresh_fail;
use super::course_load::{
    RefreshFail, cached_outcome, cached_outcome_with_error, outcome_freshness,
};
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
use super::pages::{json_string, json_u64};
use crate::output::{
    ConversationAttachmentJson, ConversationDetailJson, ConversationMessageJson,
    ConversationResult, ConversationSummaryJson, FilesListingJson, InboxResult, InboxUnreadResult,
    Outcome, PartialScope, ParticipantJson, SCHEMA_CONVERSATION, SCHEMA_INBOX, SCHEMA_INBOX_UNREAD,
    apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_inbox};

/// Run `canvas inbox` for the CLI: one envelope, one exit code.
pub async fn run_list(globals: &Globals, scope: Option<String>) -> ExitCode {
    handle_list(globals, scope).await.emit(globals.json)
}

/// Run `canvas inbox [--scope inbox|unread|sent|archived]`.
pub async fn handle_list(globals: &Globals, scope: Option<String>) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let scope = match scope.as_deref() {
        None => InboxScope::Inbox,
        Some(raw) => match InboxScope::parse(raw) {
            Some(scope) => scope,
            None => {
                return emit_error(
                    "usage",
                    "--scope takes inbox, unread, sent, or archived",
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        },
    };

    let outcome = match ensure_inbox(globals, &session, scope).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    let freshness = vec![outcome_freshness(&outcome)];

    let key = format!("scope:{}", scope.as_str());
    let rows = match session
        .open
        .store
        .call(move |conns| load_conversations(conns, &key))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return local_error(&session, &e),
    };

    let denial = outcome.error.as_deref().and_then(listing_denial_status);
    // §10: a conversation this CLI started and did not resolve makes every
    // inbox read uncertain, whatever the cache says.
    let pending = pending_operations(&session, canvas_core::store::PendingTarget::Inbox).await;
    let result = InboxResult {
        scope: scope.as_str().to_owned(),
        listing: FilesListingJson {
            available: denial.is_none(),
            http_status: denial,
        },
        conversations: rows.iter().map(ConversationRow::summary).collect(),
        pending: !pending.is_empty(),
        pending_journals: pending,
    };
    let mut envelope = base_envelope(SCHEMA_INBOX, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale inbox cache".into());
    }
    if let Some(status) = denial {
        let message = format!("Inbox listing unavailable (HTTP {status})");
        envelope.partial.push(PartialScope {
            scope: format!("inbox:scope:{}", scope.as_str()),
            http_status: Some(status),
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    Handled::new(envelope, move |envelope| {
        print_inbox(&envelope.result.conversations)
    })
}

/// Run `canvas inbox show` for the CLI: one envelope, one exit code.
pub async fn run_show(globals: &Globals, id: String) -> ExitCode {
    handle_show(globals, id).await.emit(globals.json)
}

/// Run `canvas inbox show <id>`.
pub async fn handle_show(globals: &Globals, id: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let Some(id) = id.trim().parse::<i64>().ok().filter(|id| *id > 0) else {
        return emit_error(
            "resolution",
            "conversation id must be a positive number",
            6,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };

    let outcome = match ensure_conversation(globals, &session, id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    let freshness = vec![outcome_freshness(&outcome)];

    let row = match session
        .open
        .store
        .call(move |conns| load_conversation(conns, id))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                "resolution",
                &format!("conversation {id} not found"),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return local_error(&session, &e),
    };

    let messages = row.messages();
    let complete = row.data.get("messages").is_some();
    let pending = pending_operations(
        &session,
        canvas_core::store::PendingTarget::Conversation(id),
    )
    .await;
    let mut envelope = base_envelope(
        SCHEMA_CONVERSATION,
        &session,
        ConversationResult {
            conversation: ConversationDetailJson {
                id: row.id.to_string(),
                subject: row.subject.clone(),
                workflow_state: row.workflow_state.clone(),
                last_message_at: row.last_message_at.clone(),
                context_name: json_string(&row.data, "context_name"),
                participants: row.participants(),
                messages: messages.clone(),
                messages_complete: complete,
            },
            pending: !pending.is_empty(),
            pending_journals: pending,
        },
    );
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale conversation cache".into());
    }
    let cut: Vec<&ConversationMessageJson> = messages.iter().filter(|m| m.truncated).collect();
    if !cut.is_empty() {
        let message = format!(
            "{} message bodies truncated at {BODY_LIMIT} bytes",
            cut.len()
        );
        envelope.partial.push(PartialScope {
            scope: format!("conversation:{id}"),
            http_status: None,
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    Handled::new(envelope, move |envelope| {
        print_conversation(&envelope.result.conversation)
    })
}

/// Run `canvas inbox unread-count` for the CLI: one envelope, one exit code.
pub async fn run_unread_count(globals: &Globals) -> ExitCode {
    handle_unread_count(globals).await.emit(globals.json)
}

/// Run `canvas inbox unread-count`.
pub async fn handle_unread_count(globals: &Globals) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let outcome = match ensure_unread(globals, &session).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    let freshness = vec![outcome_freshness(&outcome)];

    let count = match session
        .open
        .store
        .call(move |conns| load_unread(conns))
        .await
    {
        Ok(count) => count,
        Err(e) => return local_error(&session, &e),
    };
    let pending = pending_operations(&session, canvas_core::store::PendingTarget::Inbox).await;

    let mut envelope = base_envelope(
        SCHEMA_INBOX_UNREAD,
        &session,
        InboxUnreadResult {
            unread_count: count,
            pending: !pending.is_empty(),
            pending_journals: pending,
        },
    );
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale inbox_unread cache".into());
    }

    Handled::new(envelope, move |envelope| {
        let shown = envelope
            .result
            .unread_count
            .map_or_else(|| "unknown".to_owned(), |n| n.to_string());
        writeln!(io::stdout(), "{shown}")
    })
}

async fn ensure_inbox(
    globals: &Globals,
    session: &Session,
    scope: InboxScope,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_inbox();
    let ds = InboxDataset::new(scope, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome_with_error(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_inbox(
        client,
        &session.open.store,
        scope,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

async fn ensure_conversation(
    globals: &Globals,
    session: &Session,
    id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_inbox();
    let ds = ConversationDataset::new(id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_conversation(
        client,
        &session.open.store,
        id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

async fn ensure_unread(
    globals: &Globals,
    session: &Session,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_inbox();
    let ds = InboxUnreadDataset::new(ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_inbox_unread(client, &session.open.store, ttl, now, globals.fresh, false)
        .await
        .map_err(RefreshFail::Sync)
}

/// One cached conversation row.
pub(crate) struct ConversationRow {
    pub id: i64,
    pub subject: Option<String>,
    pub workflow_state: Option<String>,
    pub last_message_at: Option<String>,
    pub data: Value,
}

impl ConversationRow {
    fn participants(&self) -> Vec<ParticipantJson> {
        parse_nested(&self.data, "participants")
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| ParticipantJson {
                id: id_string(p.get("id")),
                name: p.get("name").and_then(Value::as_str).map(str::to_owned),
            })
            .collect()
    }

    fn messages(&self) -> Vec<ConversationMessageJson> {
        parse_nested(&self.data, "messages")
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|m| {
                let (body, truncated) = bound_body(m.get("body").and_then(Value::as_str));
                ConversationMessageJson {
                    id: id_string(m.get("id")),
                    author_id: id_string(m.get("author_id")),
                    created_at: m
                        .get("created_at")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    body,
                    truncated,
                    attachments: m
                        .get("attachments")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|a| ConversationAttachmentJson {
                            file_id: id_string(a.get("file_id")),
                            name: a.get("name").and_then(Value::as_str).map(str::to_owned),
                            size: a.get("size").and_then(Value::as_u64),
                        })
                        .collect(),
                }
            })
            .collect()
    }

    fn summary(&self) -> ConversationSummaryJson {
        ConversationSummaryJson {
            id: self.id.to_string(),
            subject: self.subject.clone(),
            workflow_state: self.workflow_state.clone(),
            last_message_at: self.last_message_at.clone(),
            message_count: json_u64(&self.data, "message_count"),
            context_name: json_string(&self.data, "context_name"),
            starred: super::pages::json_bool(&self.data, "starred"),
            participants: self.participants(),
        }
    }
}

/// A message body is bounded like every other text this package returns.
fn bound_body(raw: Option<&str>) -> (Option<String>, bool) {
    let Some(raw) = raw else {
        return (None, false);
    };
    if raw.len() <= BODY_LIMIT {
        return (Some(raw.to_owned()), false);
    }
    let mut cut = BODY_LIMIT;
    while cut > 0 && !raw.is_char_boundary(cut) {
        cut -= 1;
    }
    (Some(raw[..cut].to_owned()), true)
}

/// `data_json` keeps nested arrays as JSON text; both shapes are accepted.
fn parse_nested(data: &Value, key: &str) -> Option<Value> {
    match data.get(key)? {
        Value::String(s) => serde_json::from_str(s).ok(),
        other => Some(other.clone()),
    }
}

fn id_string(raw: Option<&Value>) -> Option<String> {
    raw.and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}

const CONVERSATION_SELECT: &str =
    "SELECT id, subject, workflow_state, last_message_at, data_json FROM conversations";

fn read_conversation(r: &rusqlite::Row<'_>) -> Result<ConversationRow, rusqlite::Error> {
    let raw: String = r.get(4)?;
    Ok(ConversationRow {
        id: r.get(0)?,
        subject: r.get(1)?,
        workflow_state: r.get(2)?,
        last_message_at: r.get(3)?,
        data: serde_json::from_str(&raw).unwrap_or(Value::Null),
    })
}

fn load_conversations(conns: &StoreConns, scope: &str) -> Result<Vec<ConversationRow>, DbError> {
    let sql = format!(
        "{CONVERSATION_SELECT}
         INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = conversations.id
         WHERE m.dataset = 'inbox' AND m.scope = ?1 AND m.entity_kind = 'conversation'
         ORDER BY m.position, conversations.id"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    Ok(stmt
        .query_map(params![scope], read_conversation)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn load_conversation(conns: &StoreConns, id: i64) -> Result<Option<ConversationRow>, DbError> {
    let sql = format!("{CONVERSATION_SELECT} WHERE id = ?1");
    Ok(conns
        .cache
        .query_row(&sql, [id], read_conversation)
        .optional()?)
}

fn load_unread(conns: &StoreConns) -> Result<Option<u64>, DbError> {
    let raw: Option<Option<i64>> = conns
        .cache
        .query_row(
            "SELECT unread_count FROM conversation_unread WHERE id = ?1",
            [UNREAD_ROW_ID],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.flatten().and_then(|n| u64::try_from(n).ok()))
}

fn local_error(session: &Session, err: &DbError) -> Handled {
    emit_error(
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn print_inbox(items: &[ConversationSummaryJson]) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(io::stdout(), "no conversations");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec!["ID", "SUBJECT", "WITH", "LAST", "STATE"]));
    for item in items {
        let with = item
            .participants
            .iter()
            .filter_map(|p| p.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        table.add_row(Row::from(vec![
            item.id.clone(),
            item.subject.clone().unwrap_or_default(),
            with,
            item.last_message_at.clone().unwrap_or_default(),
            item.workflow_state.clone().unwrap_or_default(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn print_conversation(conversation: &ConversationDetailJson) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "{}", conversation.subject.clone().unwrap_or_default())?;
    let with = conversation
        .participants
        .iter()
        .filter_map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    if !with.is_empty() {
        writeln!(out, "with {with}")?;
    }
    for message in &conversation.messages {
        writeln!(out)?;
        writeln!(out, "— {}", message.created_at.clone().unwrap_or_default())?;
        if let Some(body) = message.body.as_deref() {
            writeln!(out, "{body}")?;
        }
        if message.truncated {
            writeln!(out, "[body truncated; not complete]")?;
        }
        for attachment in &message.attachments {
            writeln!(
                out,
                "attachment {} {}",
                attachment.file_id.clone().unwrap_or_default(),
                attachment.name.clone().unwrap_or_default()
            )?;
        }
    }
    if !conversation.messages_complete {
        writeln!(out, "\n[messages not fetched for this conversation]")?;
    }
    Ok(())
}

/// The unresolved operation journals for one target (§10 pending hook).
///
/// A read never changes a journal, and a store that cannot be read is not
/// evidence of anything: the hook then reports nothing pending rather than
/// blocking the read.
pub(super) async fn pending_operations(
    session: &Session,
    target: canvas_core::store::PendingTarget,
) -> Vec<String> {
    session
        .open
        .store
        .call(move |conns| canvas_core::store::pending_operations(&conns.state, target))
        .await
        .unwrap_or_default()
}

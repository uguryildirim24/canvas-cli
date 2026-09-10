//! The event log in `state.sqlite`: identity, insert, replay, retention.

use jiff::{Span, Timestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::compare::PendingEvent;
use super::kind::EventKind;
use crate::store::DbError;

/// How long event rows are kept (REPORT §3.6).
pub const RETENTION_DAYS: i64 = 30;

/// One `canvas-cli/event@1` document, before rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    /// Monotonic log position; never reused after retention.
    pub cursor: i64,
    /// The observation that produced it.
    pub observation_id: String,
    /// One of [`EventKind`].
    pub kind: String,
    /// When the observation was made.
    pub observed_at: String,
    /// Identity this event belongs to.
    pub identity_key: String,
    /// Identity generation (§10); a cursor from another one is not replayable.
    pub generation: String,
    /// `fetch_log.dataset`, or `submission_journal` for a journal transition.
    pub dataset: String,
    /// `fetch_log.scope`.
    pub scope: String,
    /// Entity key, when the event is about one entity.
    pub entity_key: Option<String>,
    /// Allowlisted fields before the change.
    pub before: Value,
    /// Allowlisted fields after it.
    pub after: Value,
}

/// The identity a state database is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventIdentity {
    pub key: String,
    pub generation: String,
}

/// Read the identity key and generation from `state.sqlite`.
pub fn identity(state: &Connection) -> Result<EventIdentity, DbError> {
    let value = |name: &str| -> Result<String, DbError> {
        Ok(state
            .query_row("SELECT value FROM identity WHERE key = ?1", [name], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_default())
    };
    Ok(EventIdentity {
        key: value("key")?,
        generation: value("generation")?,
    })
}

/// Append one event inside an open state transaction.
pub fn insert(
    tx: &Transaction<'_>,
    identity: &EventIdentity,
    observation_id: &str,
    observed_at: &str,
    dataset: &str,
    scope: &str,
    event: &PendingEvent,
) -> Result<i64, DbError> {
    tx.execute(
        "INSERT INTO events (
            observation_id, kind, observed_at, identity_key, generation,
            dataset, scope, entity_key, [before], [after]
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            observation_id,
            event.kind.as_str(),
            observed_at,
            identity.key,
            identity.generation,
            dataset,
            scope,
            event.entity_key,
            event.before.to_string(),
            event.after.to_string(),
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Every event after `since`, oldest first.
pub fn read_after(
    state: &Connection,
    since: i64,
    limit: usize,
) -> Result<Vec<EventRecord>, DbError> {
    let mut stmt = state.prepare(
        "SELECT cursor, observation_id, kind, observed_at, identity_key, generation,
                dataset, scope, entity_key, [before], [after]
         FROM events WHERE cursor > ?1 ORDER BY cursor ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![since, i64::try_from(limit).unwrap_or(i64::MAX)],
        |r| {
            Ok(EventRecord {
                cursor: r.get(0)?,
                observation_id: r.get(1)?,
                kind: r.get(2)?,
                observed_at: r.get(3)?,
                identity_key: r.get(4)?,
                generation: r.get(5)?,
                dataset: r.get(6)?,
                scope: r.get(7)?,
                entity_key: r.get(8)?,
                before: parse_json(&r.get::<_, String>(9)?),
                after: parse_json(&r.get::<_, String>(10)?),
            })
        },
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(Map::new()))
}

/// The highest cursor the log ever issued, including expired rows.
///
/// `events.cursor` is `AUTOINCREMENT`, so `sqlite_sequence` remembers the high
/// water mark after retention removes rows. That is what tells an expired
/// cursor apart from one that belongs to another database.
pub fn high_water(state: &Connection) -> Result<i64, DbError> {
    Ok(state
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'events'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// The lowest cursor still stored.
pub fn low_water(state: &Connection) -> Result<Option<i64>, DbError> {
    Ok(state
        .query_row("SELECT MIN(cursor) FROM events", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()?
        .flatten())
}

/// Whether a `--since` cursor can still be replayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorCheck {
    /// Replay from this cursor.
    Replay,
    /// Expired, or from another identity generation: emit `resync_required`.
    Resync,
}

/// Classify a `--since` cursor (REPORT §3.6, §3.2 exit row).
pub fn check_cursor(
    state: &Connection,
    since: i64,
    generation: &str,
) -> Result<CursorCheck, DbError> {
    // `--since 0` claims no position, so only the generation is checked.
    if since > 0 {
        if since > high_water(state)? {
            // Nothing this database ever issued: another identity or log.
            return Ok(CursorCheck::Resync);
        }
        match low_water(state)? {
            // Rows between `since` and the oldest retained row have expired.
            Some(low) if since + 1 < low => return Ok(CursorCheck::Resync),
            None if since < high_water(state)? => return Ok(CursorCheck::Resync),
            _ => {}
        }
    }
    // The generation that issued this cursor, and any generation after it,
    // must be the one this process is bound to.
    let issuing: Option<String> = state
        .query_row(
            "SELECT generation FROM events WHERE cursor <= ?1 ORDER BY cursor DESC LIMIT 1",
            params![since],
            |r| r.get(0),
        )
        .optional()?;
    let after: Option<String> = state
        .query_row(
            "SELECT generation FROM events WHERE cursor > ?1 AND generation <> ?2 LIMIT 1",
            params![since, generation],
            |r| r.get(0),
        )
        .optional()?;
    Ok(
        if after.is_some() || issuing.is_some_and(|g| g != generation) {
            CursorCheck::Resync
        } else {
            CursorCheck::Replay
        },
    )
}

/// Delete event rows older than the retention window.
///
/// The file is never removed, and neither is any other table: only rows go.
pub fn expire(state: &mut Connection, now: Timestamp) -> Result<usize, DbError> {
    // `Timestamp` arithmetic takes hours or smaller, so the window is exact.
    let cutoff = now
        .checked_sub(Span::new().hours(RETENTION_DAYS * 24))
        .map_err(|e| DbError::Message(e.to_string()))?;
    let tx = state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let removed = tx.execute(
        "DELETE FROM events WHERE observed_at < ?1",
        params![cutoff.to_string()],
    )?;
    tx.commit()?;
    Ok(removed)
}

/// How far a derived consumer has read the log.
///
/// `notify` deduplicates by cursor, so its position is durable: a second run
/// posts nothing the first one already posted (REPORT §3.6).
pub fn consumer_cursor(state: &Connection, consumer: &str) -> Result<i64, DbError> {
    Ok(state
        .query_row(
            "SELECT cursor FROM consumer_cursor WHERE consumer = ?1",
            [consumer],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// Move a consumer's position forward. It never moves back.
///
/// Every state write is one `BEGIN IMMEDIATE` transaction (SPEC §10).
pub fn set_consumer_cursor(
    state: &mut Connection,
    consumer: &str,
    cursor: i64,
) -> Result<(), DbError> {
    let tx = state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO consumer_cursor (consumer, cursor, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(consumer) DO UPDATE SET
            cursor = MAX(cursor, excluded.cursor), updated_at = excluded.updated_at",
        params![consumer, cursor, Timestamp::now().to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Replace a consumer's position after a resync.
///
/// [`set_consumer_cursor`] never moves a position back, because a consumer
/// that already reported an event must not report it twice. A cursor this log
/// cannot replay is not a position at all: it expired, or it belongs to
/// another identity generation. Once the consumer is told to resync, the
/// stored value is replaced, so the same gap is never reported again.
pub fn reset_consumer_cursor(
    state: &mut Connection,
    consumer: &str,
    cursor: i64,
) -> Result<(), DbError> {
    let tx = state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO consumer_cursor (consumer, cursor, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(consumer) DO UPDATE SET
            cursor = excluded.cursor, updated_at = excluded.updated_at",
        params![consumer, cursor, Timestamp::now().to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Record a journal state transition inside the journal's own transaction.
///
/// SPEC §12.2 keeps the journal and its event atomic: either both land or
/// neither does, so a reader never sees a state the log does not carry.
pub fn record_submission_state(
    tx: &Transaction<'_>,
    journal_id: &str,
    from: Option<&str>,
    to: &str,
) -> Result<(), DbError> {
    let assignment_id: i64 = tx.query_row(
        "SELECT assignment_id FROM submission_journal WHERE journal_id = ?1",
        [journal_id],
        |r| r.get(0),
    )?;
    let who = identity(tx)?;
    let field = |state: Option<&str>| {
        let mut map = Map::new();
        map.insert(
            "state".to_owned(),
            state.map_or(Value::Null, |s| Value::String(s.to_owned())),
        );
        Value::Object(map)
    };
    let event = PendingEvent {
        kind: EventKind::SubmissionState,
        entity_key: Some(journal_id.to_owned()),
        before: field(from),
        after: field(Some(to)),
    };
    let observed_at = Timestamp::now().to_string();
    insert(
        tx,
        &who,
        &format!("journal:{journal_id}:{to}"),
        &observed_at,
        "submission_journal",
        &format!("assignment:{assignment_id}"),
        &event,
    )?;
    Ok(())
}

/// Record an operation-journal transition inside the operation's transaction.
///
/// The same rule as [`record_submission_state`]: the row and its event land
/// together or not at all. The scope names the target the operation writes to,
/// so a reader can tell a reply to one topic from a reply to another without
/// reading a body.
pub fn record_operation_state(
    tx: &Transaction<'_>,
    journal_id: &str,
    scope: &str,
    from: Option<&str>,
    to: &str,
) -> Result<(), DbError> {
    let who = identity(tx)?;
    let field = |state: Option<&str>| {
        let mut map = Map::new();
        map.insert(
            "state".to_owned(),
            state.map_or(Value::Null, |s| Value::String(s.to_owned())),
        );
        Value::Object(map)
    };
    let event = PendingEvent {
        kind: EventKind::OperationState,
        entity_key: Some(journal_id.to_owned()),
        before: field(from),
        after: field(Some(to)),
    };
    let observed_at = Timestamp::now().to_string();
    insert(
        tx,
        &who,
        &format!("operation:{journal_id}:{to}"),
        &observed_at,
        "operation_journal",
        scope,
        &event,
    )?;
    Ok(())
}

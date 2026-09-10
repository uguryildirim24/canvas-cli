//! Hit predicate, epochs, pending hook, and cache stats/clear/path.

use std::path::{Path, PathBuf};

use jiff::{Span, Timestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

use super::dataset::{Dataset, LookupResult};
use super::db::{DbError, StoreConns};

/// One `fetch_log` row.
#[derive(Debug, Clone)]
pub struct FetchLogRow {
    pub dataset: String,
    pub scope: String,
    pub fetched_at: Timestamp,
    pub complete: bool,
    pub count: i64,
    pub stale: bool,
    pub error: Option<String>,
    pub epoch_seen: i64,
    pub contexts: Option<String>,
    pub window_start: Option<Timestamp>,
    pub window_end: Option<Timestamp>,
}

/// `fetch_log.error` marker for a window dataset whose batches were isolated
/// per context (§10, §12.6): `contexts_denied:<status>@<context>,...`.
///
/// Coverage is still complete — every batch was stored or isolated — so the
/// listed contexts are reported as `partial[]` rather than as a failure, and
/// they survive in the cache so a later cached read reports them too.
pub const CONTEXT_DENIED_PREFIX: &str = "contexts_denied:";

/// Encode isolated per-context denials for `fetch_log.error`.
///
/// Returns `None` when nothing was denied, so callers store no error at all.
#[must_use]
pub fn encode_context_denials(denials: &[(String, u16)]) -> Option<String> {
    if denials.is_empty() {
        return None;
    }
    let mut pairs: Vec<String> = denials
        .iter()
        .filter(|(context, _)| is_context_code(context))
        .map(|(context, status)| format!("{status}@{context}"))
        .collect();
    pairs.sort();
    pairs.dedup();
    if pairs.is_empty() {
        return None;
    }
    Some(format!("{CONTEXT_DENIED_PREFIX}{}", pairs.join(",")))
}

/// Decode [`encode_context_denials`]. Any other error string yields `[]`.
#[must_use]
pub fn parse_context_denials(error: &str) -> Vec<(String, u16)> {
    let Some(body) = error.strip_prefix(CONTEXT_DENIED_PREFIX) else {
        return Vec::new();
    };
    body.split(',')
        .filter_map(|pair| {
            let (status, context) = pair.split_once('@')?;
            let status: u16 = status.parse().ok()?;
            (is_context_code(context) && (400..600).contains(&status))
                .then(|| (context.to_owned(), status))
        })
        .collect()
}

/// True when `error` is a well-formed context-denial marker.
#[must_use]
pub fn is_context_denial(error: &str) -> bool {
    error.starts_with(CONTEXT_DENIED_PREFIX) && !parse_context_denials(error).is_empty()
}

/// Context codes are `<kind>_<id>`; reject anything that could confuse the
/// `,` and `@` separators or hide a newline in the stored row.
fn is_context_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Query parameters for [`lookup`].
#[derive(Debug, Clone)]
pub struct LookupQuery<'a> {
    pub dataset: &'a str,
    pub scope: &'a str,
    pub now: Timestamp,
    pub ttl: Span,
    /// When set, require matching context hash and window containment.
    pub window: Option<WindowQuery<'a>>,
}

/// Window containment inputs for planner / announcements / calendar.
#[derive(Debug, Clone)]
pub struct WindowQuery<'a> {
    pub contexts: &'a str,
    pub start: Timestamp,
    pub end: Timestamp,
}

/// Epoch advanced between refresh start and cache commit.
#[derive(Debug, Clone, Error)]
#[error("epoch abort for scope {scope}: seen {epoch_seen}, current {current}")]
pub struct EpochAbort {
    pub scope: String,
    pub epoch_seen: i64,
    pub current: i64,
}

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub tables: Vec<(String, i64)>,
    pub total_rows: i64,
}

/// Tables cleared by `cache clear` (every cache table, never unlink).
pub const CACHE_TABLES: &[&str] = &[
    "courses",
    "terms",
    "assignment_groups",
    "assignments",
    "submissions",
    "modules",
    "module_items",
    "folders",
    "files",
    "announcements",
    "calendar_events",
    "pages",
    "discussion_topics",
    "discussion_entries",
    "conversations",
    "conversation_unread",
    "planner_items",
    "enrollment_grades",
    "course_totals",
    "grading_periods",
    "fake_entities",
    "field_obs",
    "membership",
    "fetch_log",
];

/// Hit predicate (§10): complete, not stale, epoch ok, age within TTL, window ok.
pub fn lookup(conns: &StoreConns, query: &LookupQuery<'_>) -> Result<LookupResult, DbError> {
    let mut candidates = if query.window.is_some() {
        let mut stmt = conns
            .cache
            .prepare("SELECT scope FROM fetch_log WHERE dataset = ?1")?;
        let scopes = stmt
            .query_map([query.dataset], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        scopes
            .iter()
            .map(|scope| load_fetch_log(&conns.cache, query.dataset, scope))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    } else {
        load_fetch_log(&conns.cache, query.dataset, query.scope)?
            .into_iter()
            .collect()
    };
    candidates.sort_by_key(|row| std::cmp::Reverse(row.fetched_at));
    let requested_epoch =
        read_scope_epoch(&conns.state, &format!("{}:{}", query.dataset, query.scope))?;
    let mut stale = None;
    for row in candidates {
        if !row.complete
            || query
                .window
                .as_ref()
                .is_some_and(|w| !window_contains(&row, w))
        {
            continue;
        }
        let row_epoch = read_scope_epoch(&conns.state, &format!("{}:{}", row.dataset, row.scope))?;
        if !row.stale
            && row.epoch_seen >= requested_epoch
            && row.epoch_seen >= row_epoch
            && age_within_ttl(row.fetched_at, query.now, query.ttl)?
        {
            return Ok(LookupResult::Hit(row));
        }
        if stale.is_none() {
            stale = Some(row);
        }
    }
    Ok(stale.map_or(LookupResult::Miss, LookupResult::Stale))
}

/// Convenience: lookup using a [`Dataset`]'s name, scope, and TTL.
pub fn lookup_dataset(
    conns: &StoreConns,
    dataset: &impl Dataset,
    now: Timestamp,
    window: Option<WindowQuery<'_>>,
) -> Result<LookupResult, DbError> {
    let result = lookup(
        conns,
        &LookupQuery {
            dataset: dataset.name(),
            scope: dataset.scope_key(),
            now,
            ttl: dataset.ttl(),
            window,
        },
    )?;
    match result {
        LookupResult::Hit(row) if row.epoch_seen < dataset.current_epoch(&conns.state)? => {
            Ok(LookupResult::Stale(row))
        }
        other => Ok(other),
    }
}

fn window_contains(row: &FetchLogRow, w: &WindowQuery<'_>) -> bool {
    let Some(ref ctx) = row.contexts else {
        return false;
    };
    if ctx != w.contexts {
        return false;
    }
    let (Some(ws), Some(we)) = (row.window_start, row.window_end) else {
        return false;
    };
    w.start <= w.end && ws <= w.start && we >= w.end
}

fn age_within_ttl(fetched_at: Timestamp, now: Timestamp, ttl: Span) -> Result<bool, DbError> {
    let expiry = fetched_at
        .checked_add(ttl)
        .map_err(|e| DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    Ok(fetched_at <= now && now <= expiry)
}

/// Load a `fetch_log` row.
pub fn load_fetch_log(
    cache: &Connection,
    dataset: &str,
    scope: &str,
) -> Result<Option<FetchLogRow>, DbError> {
    let row = cache
        .query_row(
            "SELECT dataset, scope, fetched_at, complete, count, stale, error,
                    epoch_seen, contexts, window_start, window_end
             FROM fetch_log WHERE dataset = ?1 AND scope = ?2",
            params![dataset, scope],
            |r| {
                Ok(FetchLogRow {
                    dataset: r.get(0)?,
                    scope: r.get(1)?,
                    fetched_at: parse_ts(&r.get::<_, String>(2)?)?,
                    complete: r.get::<_, i64>(3)? != 0,
                    count: r.get(4)?,
                    stale: r.get::<_, i64>(5)? != 0,
                    error: r.get(6)?,
                    epoch_seen: r.get(7)?,
                    contexts: r.get(8)?,
                    window_start: r
                        .get::<_, Option<String>>(9)?
                        .map(|s| parse_ts(&s))
                        .transpose()?,
                    window_end: r
                        .get::<_, Option<String>>(10)?
                        .map(|s| parse_ts(&s))
                        .transpose()?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

fn parse_ts(s: &str) -> rusqlite::Result<Timestamp> {
    s.parse::<Timestamp>().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Effective epoch for a scope, including prefix scopes (`planner:*`).
/// Sum independent counters so incrementing any matching scope invalidates a hit.
pub fn read_scope_epoch(state: &Connection, scope: &str) -> Result<i64, DbError> {
    let mut effective_epoch: i64 = 0;
    let mut stmt = state.prepare("SELECT scope, epoch FROM scope_epoch")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (stored, epoch) = row?;
        if scope_matches(&stored, scope) {
            effective_epoch = effective_epoch
                .checked_add(epoch)
                .ok_or_else(|| DbError::Message("scope epoch overflow".into()))?;
        }
    }
    Ok(effective_epoch)
}

/// `stored` may be an exact scope or a prefix ending in `*`.
fn scope_matches(stored: &str, requested: &str) -> bool {
    if let Some(prefix) = stored.strip_suffix('*') {
        requested.starts_with(prefix)
    } else {
        stored == requested
    }
}

/// Increment epochs for the given scopes inside an open state transaction.
pub fn bump_epochs(scopes: &[&str], state_tx: &Transaction<'_>) -> Result<(), DbError> {
    for scope in scopes {
        state_tx.execute(
            "INSERT INTO scope_epoch (scope, epoch) VALUES (?1, 1)
             ON CONFLICT(scope) DO UPDATE SET epoch = epoch + 1",
            params![scope],
        )?;
    }
    Ok(())
}

/// One row of the pending-operation query: id, state, acknowledgement.
type PendingRow = (String, String, Option<String>);

/// Which target a read is asking about (§10 pending hook, M8-b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingTarget {
    /// One discussion topic.
    Topic(i64),
    /// One conversation.
    Conversation(i64),
    /// The inbox as a whole: any conversation write is pending for it.
    Inbox,
}

/// Every unresolved operation journal that touches this target (§10, M8-b).
///
/// `planned` and `posting` are always pending, and an `outcome_unknown`
/// operation is pending until it is acknowledged. A read that names one of
/// these targets says its answer may be behind.
///
/// Unlike a submission, a later write never supersedes an earlier one: a
/// second reply to a topic is a second post and a second conversation is a
/// second conversation, so a later success proves nothing about an earlier
/// unknown outcome. (An `inbox_send` has no target column at all until Canvas
/// answers, so a superseding rule keyed on the target columns would have let
/// any accepted send retire any other unresolved send.)
pub fn pending_operations(
    state: &Connection,
    target: PendingTarget,
) -> Result<Vec<String>, DbError> {
    // Each clause binds exactly the parameters it names, so the count always
    // matches what the statement was prepared with.
    let (clause, args): (&str, Vec<i64>) = match target {
        PendingTarget::Topic(id) => ("topic_id = ?1", vec![id]),
        // A send has no conversation id until Canvas answers, so an
        // unresolved one is pending for every conversation read — including
        // an `outcome_unknown` one, which is exactly the send that may have
        // landed in a conversation this journal cannot name.
        PendingTarget::Conversation(id) => {
            ("(conversation_id = ?1 OR kind = 'inbox_send')", vec![id])
        }
        PendingTarget::Inbox => ("kind IN ('inbox_send','inbox_reply')", Vec::new()),
    };
    let sql = format!(
        "SELECT journal_id, state, acknowledged_at
         FROM operation_journal
         WHERE {clause} AND state IN ('planned','posting','outcome_unknown')
         ORDER BY created_at ASC, journal_id ASC"
    );
    let mut stmt = state.prepare(&sql)?;
    let rows: Vec<PendingRow> = stmt
        .query_map(rusqlite::params_from_iter(args), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .filter(|(_, row_state, acknowledged_at)| {
            row_state != "outcome_unknown" || acknowledged_at.is_none()
        })
        .map(|(journal_id, _, _)| journal_id)
        .collect())
}

/// Pending journal read hook (§10).
///
/// Pending when state is `planned|uploading|uploaded|posting`, or
/// `outcome_unknown` and neither superseded nor acknowledged.
pub fn pending_for_assignment(state: &Connection, assignment_id: i64) -> Result<bool, DbError> {
    Ok(!pending_journals_for_assignment(state, assignment_id)?.is_empty())
}

/// Every pending journal id for an assignment, oldest first (`submission@1`).
///
/// Same rule as [`pending_for_assignment`]; this returns the identifiers so a
/// reader can name the journals that make the status unknown.
pub fn pending_journals_for_assignment(
    state: &Connection,
    assignment_id: i64,
) -> Result<Vec<String>, DbError> {
    let mut stmt = state.prepare(
        "SELECT journal_id, state, created_at, acknowledged_at FROM submission_journal \
         WHERE assignment_id = ?1 ORDER BY created_at ASC, journal_id ASC",
    )?;
    let rows: Vec<(String, String, Timestamp, Option<String>)> = stmt
        .query_map(params![assignment_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                parse_ts(&r.get::<_, String>(2)?)?,
                r.get(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut pending = Vec::new();
    for (journal_id, state_name, created_at, acknowledged_at) in &rows {
        match state_name.as_str() {
            "planned" | "uploading" | "uploaded" | "posting" => pending.push(journal_id.clone()),
            "outcome_unknown" if acknowledged_at.is_none() => {
                let superseded = rows.iter().any(|(_, later_state, later_at, _)| {
                    later_at > created_at && matches!(later_state.as_str(), "submitted" | "matched")
                });
                if !superseded {
                    pending.push(journal_id.clone());
                }
            }
            _ => {}
        }
    }
    Ok(pending)
}

/// `cache stats`: row counts per cache table.
pub fn cache_stats(cache: &Connection) -> Result<CacheStats, DbError> {
    let mut tables = Vec::new();
    let mut total = 0i64;
    for name in CACHE_TABLES {
        let count: i64 =
            cache.query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |r| r.get(0))?;
        total += count;
        tables.push(((*name).to_string(), count));
    }
    Ok(CacheStats {
        tables,
        total_rows: total,
    })
}

/// `cache clear`: DELETE every cache table in one transaction, then VACUUM.
pub fn cache_clear(cache: &mut Connection) -> Result<(), DbError> {
    {
        let tx = cache.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for name in CACHE_TABLES {
            tx.execute(&format!("DELETE FROM {name}"), [])?;
        }
        tx.commit()?;
    }
    cache.execute_batch("VACUUM")?;
    Ok(())
}

/// `cache path`: return the cache database path.
#[must_use]
pub fn cache_path(cache_db: &Path) -> PathBuf {
    cache_db.to_path_buf()
}

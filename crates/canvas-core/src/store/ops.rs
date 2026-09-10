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
    let row = load_fetch_log(&conns.cache, query.dataset, query.scope)?;
    let Some(row) = row else {
        return Ok(LookupResult::Miss);
    };

    let epoch = read_scope_epoch(&conns.state, query.scope)?;
    let age_ok = age_within_ttl(row.fetched_at, query.now, query.ttl)?;
    let window_ok = match &query.window {
        None => true,
        Some(w) => window_contains(&row, w),
    };
    let epoch_ok = row.epoch_seen >= epoch;

    if row.complete && !row.stale && epoch_ok && age_ok && window_ok {
        return Ok(LookupResult::Hit(row));
    }
    if row.complete {
        return Ok(LookupResult::Stale(row));
    }
    Ok(LookupResult::Miss)
}

/// Convenience: lookup using a [`Dataset`]'s name, scope, and TTL.
pub fn lookup_dataset(
    conns: &StoreConns,
    dataset: &impl Dataset,
    now: Timestamp,
    window: Option<WindowQuery<'_>>,
) -> Result<LookupResult, DbError> {
    lookup(
        conns,
        &LookupQuery {
            dataset: dataset.name(),
            scope: dataset.scope_key(),
            now,
            ttl: dataset.ttl(),
            window,
        },
    )
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
    ws <= w.start && we >= w.end
}

fn age_within_ttl(fetched_at: Timestamp, now: Timestamp, ttl: Span) -> Result<bool, DbError> {
    let expiry = fetched_at
        .checked_add(ttl)
        .map_err(|e| DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    Ok(now <= expiry)
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
pub fn read_scope_epoch(state: &Connection, scope: &str) -> Result<i64, DbError> {
    let mut max_epoch: i64 = 0;
    let mut stmt = state.prepare("SELECT scope, epoch FROM scope_epoch")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (stored, epoch) = row?;
        if scope_matches(&stored, scope) {
            max_epoch = max_epoch.max(epoch);
        }
    }
    Ok(max_epoch)
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

/// Pending journal read hook (§10).
///
/// Pending when state is `planned|uploading|uploaded|posting`, or
/// `outcome_unknown` and neither superseded nor acknowledged.
pub fn pending_for_assignment(state: &Connection, assignment_id: i64) -> Result<bool, DbError> {
    let mut stmt = state.prepare(
        "SELECT journal_id, state, created_at, acknowledged_at
         FROM submission_journal WHERE assignment_id = ?1",
    )?;
    let rows: Vec<(String, String, String, Option<String>)> = stmt
        .query_map(params![assignment_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (journal_id, state_name, created_at, acknowledged_at) in &rows {
        match state_name.as_str() {
            "planned" | "uploading" | "uploaded" | "posting" => return Ok(true),
            "outcome_unknown" => {
                if acknowledged_at.is_some() {
                    continue;
                }
                if is_superseded(state, assignment_id, created_at)? {
                    continue;
                }
                let _ = journal_id;
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

fn is_superseded(
    state: &Connection,
    assignment_id: i64,
    created_at: &str,
) -> Result<bool, DbError> {
    let found: Option<i64> = state
        .query_row(
            "SELECT 1 FROM submission_journal
             WHERE assignment_id = ?1
               AND created_at > ?2
               AND state IN ('submitted', 'matched')
             LIMIT 1",
            params![assignment_id, created_at],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
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

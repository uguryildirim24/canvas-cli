//! The observation outbox and its replay (agent-UX design).
//!
//! The cache commit and the state transaction are separate on purpose, so the
//! sequence has to survive a kill between them:
//!
//! 1. A refresh commits its pages to `cache.sqlite`.
//! 2. [`record`] writes a `pending` `observations` row naming the exact cache
//!    row it saw: `<dataset>:<scope>:<fetch_log rowid>:<fetched_at>`.
//! 3. [`apply`] compares that row against the baseline and commits the cursor,
//!    the baseline, and the events together, keyed by the observation id, so a
//!    re-run after a crash is idempotent.
//!
//! A kill between 1 and 2 leaves the baseline untouched: the next refresh
//! reports the same difference, one refresh later. A kill between 2 and 3
//! leaves a `pending` row that [`apply_pending`] picks up. If the cache row it
//! named is gone or has been overwritten in the meantime, the gap is real and
//! is reported once as `resync_required`, never passed over in silence.

use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Map, Value};

use super::compare::{Member, Members, PendingEvent, Shape, shape_for};
use super::kind::EventKind;
use super::log::{self};
use crate::store::{DbError, Store, StoreConns};

/// What one observation produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Observed {
    /// Events appended to the log.
    pub events: usize,
    /// Whether the baseline was established by this observation (silent).
    pub baseline: bool,
    /// Whether the gap had to be reported.
    pub resync: bool,
}

/// Record and apply the observation of a completed refresh.
pub async fn observe_refresh(
    store: &Store,
    dataset: &str,
    scope: &str,
) -> Result<Observed, DbError> {
    let dataset = dataset.to_owned();
    let scope = scope.to_owned();
    let Some(observation) = store
        .call({
            let (dataset, scope) = (dataset.clone(), scope.clone());
            move |conns| record(conns, &dataset, &scope)
        })
        .await?
    else {
        return Ok(Observed::default());
    };
    store.call(move |conns| apply(conns, &observation)).await
}

/// Record the observation of a completed refresh without applying it.
///
/// The two halves are separate so a test, and a crash, can land between them.
pub async fn record_observation(
    store: &Store,
    dataset: &str,
    scope: &str,
) -> Result<Option<String>, DbError> {
    let dataset = dataset.to_owned();
    let scope = scope.to_owned();
    store
        .call(move |conns| record(conns, &dataset, &scope))
        .await
}

/// Apply every observation left `pending` by an earlier run.
pub async fn apply_pending(store: &Store) -> Result<Observed, DbError> {
    let pending: Vec<String> = store
        .call(|conns| {
            let mut stmt = conns.state.prepare(
                "SELECT observation_id FROM observations
                 WHERE state = 'pending' ORDER BY recorded_at ASC, observation_id ASC",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    let mut total = Observed::default();
    for observation in pending {
        let one = store.call(move |conns| apply(conns, &observation)).await?;
        total.events += one.events;
        total.baseline |= one.baseline;
        total.resync |= one.resync;
    }
    Ok(total)
}

/// The identifier of the cache row a refresh just wrote.
fn observation_id(dataset: &str, scope: &str, row_id: i64, fetched_at: &str) -> String {
    format!("{dataset}:{scope}:{row_id}:{fetched_at}")
}

/// The `fetch_log` row for a scope, when it is complete coverage.
///
/// A partial or failed page has `complete = 0` or `stale = 1`; it is not
/// observed at all, so it can never imply a removal.
fn complete_row(
    conns: &StoreConns,
    dataset: &str,
    scope: &str,
) -> Result<Option<(i64, String)>, DbError> {
    Ok(conns
        .cache
        .query_row(
            "SELECT rowid, fetched_at, complete, stale FROM fetch_log
             WHERE dataset = ?1 AND scope = ?2",
            params![dataset, scope],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, i64>(3)? != 0,
                ))
            },
        )
        .optional()?
        .filter(|(_, _, complete, stale)| *complete && !*stale)
        .map(|(id, at, _, _)| (id, at)))
}

/// Write the `pending` outbox row. `None` when nothing is observable.
fn record(conns: &mut StoreConns, dataset: &str, scope: &str) -> Result<Option<String>, DbError> {
    if shape_for(dataset).is_none() {
        return Ok(None);
    }
    let Some((row_id, fetched_at)) = complete_row(conns, dataset, scope)? else {
        return Ok(None);
    };
    let id = observation_id(dataset, scope, row_id, &fetched_at);
    let tx = conns
        .state
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO observations
            (observation_id, dataset, scope, fetch_row_id, fetched_at, state, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)
         ON CONFLICT(observation_id) DO NOTHING",
        params![
            id,
            dataset,
            scope,
            row_id,
            fetched_at,
            Timestamp::now().to_string()
        ],
    )?;
    tx.commit()?;
    Ok(Some(id))
}

/// Compare one recorded observation and commit its events.
fn apply(conns: &mut StoreConns, observation_id: &str) -> Result<Observed, DbError> {
    let row: Option<(String, String, i64, String, String)> = conns
        .state
        .query_row(
            "SELECT dataset, scope, fetch_row_id, fetched_at, state
             FROM observations WHERE observation_id = ?1",
            [observation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((dataset, scope, row_id, fetched_at, state)) = row else {
        return Ok(Observed::default());
    };
    if state == "applied" {
        // A re-run after a crash changes nothing.
        return Ok(Observed::default());
    }
    let Some(shape) = shape_for(&dataset) else {
        return Ok(Observed::default());
    };

    // The cache row must still be the one that was observed.
    let current = complete_row(conns, &dataset, &scope)?
        .map(|(id, at)| observation_id_matches(&dataset, &scope, id, &at, row_id, &fetched_at));
    if current != Some(true) {
        return report_gap(conns, observation_id, &dataset, &scope, &fetched_at);
    }

    let observed = read_members(conns, shape, &scope)?;
    let who = log::identity(&conns.state)?;
    let tx = conns
        .state
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another process may have applied it while this one read the cache.
    let still_pending: Option<String> = tx
        .query_row(
            "SELECT state FROM observations WHERE observation_id = ?1",
            [observation_id],
            |r| r.get(0),
        )
        .optional()?;
    if still_pending.as_deref() != Some("pending") {
        return Ok(Observed::default());
    }
    let baseline: Option<String> = tx
        .query_row(
            "SELECT members_json FROM baselines WHERE dataset = ?1 AND scope = ?2",
            params![dataset, scope],
            |r| r.get(0),
        )
        .optional()?;
    let mut result = Observed::default();
    let events = match &baseline {
        // The first complete observation of a scope sets the baseline and
        // emits nothing.
        None => {
            result.baseline = true;
            Vec::new()
        }
        Some(raw) => super::compare::diff(shape, &parse_members(raw), &observed),
    };
    for event in &events {
        log::insert(
            &tx,
            &who,
            observation_id,
            &fetched_at,
            &dataset,
            &scope,
            event,
        )?;
    }
    write_baseline(
        &tx,
        &dataset,
        &scope,
        observation_id,
        &fetched_at,
        &observed,
    )?;
    mark_applied(&tx, observation_id)?;
    tx.commit()?;
    result.events = events.len();
    Ok(result)
}

fn observation_id_matches(
    dataset: &str,
    scope: &str,
    row_id: i64,
    fetched_at: &str,
    want_row: i64,
    want_at: &str,
) -> bool {
    observation_id(dataset, scope, row_id, fetched_at)
        == observation_id(dataset, scope, want_row, want_at)
}

/// The observed cache row is gone or was overwritten: say so once.
fn report_gap(
    conns: &mut StoreConns,
    observation_id: &str,
    dataset: &str,
    scope: &str,
    fetched_at: &str,
) -> Result<Observed, DbError> {
    let who = log::identity(&conns.state)?;
    let tx = conns
        .state
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let still_pending: Option<String> = tx
        .query_row(
            "SELECT state FROM observations WHERE observation_id = ?1",
            [observation_id],
            |r| r.get(0),
        )
        .optional()?;
    if still_pending.as_deref() != Some("pending") {
        return Ok(Observed::default());
    }
    let mut after = Map::new();
    after.insert(
        "reason".to_owned(),
        Value::String("observation_unavailable".to_owned()),
    );
    let event = PendingEvent {
        kind: EventKind::ResyncRequired,
        entity_key: None,
        before: Value::Object(Map::new()),
        after: Value::Object(after),
    };
    log::insert(
        &tx,
        &who,
        observation_id,
        fetched_at,
        dataset,
        scope,
        &event,
    )?;
    // The next complete observation of this scope establishes a fresh
    // baseline, silently, rather than comparing across the gap.
    tx.execute(
        "DELETE FROM baselines WHERE dataset = ?1 AND scope = ?2",
        params![dataset, scope],
    )?;
    mark_applied(&tx, observation_id)?;
    tx.commit()?;
    Ok(Observed {
        events: 1,
        baseline: false,
        resync: true,
    })
}

fn mark_applied(tx: &rusqlite::Transaction<'_>, observation_id: &str) -> Result<(), DbError> {
    tx.execute(
        "UPDATE observations SET state = 'applied' WHERE observation_id = ?1",
        [observation_id],
    )?;
    Ok(())
}

fn write_baseline(
    tx: &rusqlite::Transaction<'_>,
    dataset: &str,
    scope: &str,
    observation_id: &str,
    observed_at: &str,
    members: &Members,
) -> Result<(), DbError> {
    let json = serde_json::to_string(members).map_err(|e| DbError::Message(e.to_string()))?;
    tx.execute(
        "INSERT INTO baselines (dataset, scope, observation_id, observed_at, members_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(dataset, scope) DO UPDATE SET
            observation_id = excluded.observation_id,
            observed_at = excluded.observed_at,
            members_json = excluded.members_json",
        params![dataset, scope, observation_id, observed_at, json],
    )?;
    Ok(())
}

fn parse_members(raw: &str) -> Members {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Read the allowlisted record of every member of a complete scope.
fn read_members(conns: &StoreConns, shape: &Shape, scope: &str) -> Result<Members, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT entity_id FROM membership
         WHERE dataset = ?1 AND scope = ?2 ORDER BY position ASC",
    )?;
    let keys: Vec<String> = stmt
        .query_map(params![shape.dataset, scope], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut columns: Vec<&str> = shape.columns.to_vec();
    if !shape.json_keys.is_empty() {
        columns.push("data_json");
    }
    let sql = format!(
        "SELECT {} FROM {} WHERE id = ?1",
        columns.join(", "),
        shape.table
    );
    let mut row_stmt = conns.cache.prepare(&sql)?;
    let mut members = Members::new();
    for key in keys {
        let member = row_stmt
            .query_row(params![key], |r| {
                let mut map = Member::new();
                for (index, name) in shape.columns.iter().enumerate() {
                    map.insert((*name).to_owned(), sql_value(r, index)?);
                }
                if !shape.json_keys.is_empty() {
                    let raw: String = r.get(shape.columns.len())?;
                    let data: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
                    for name in shape.json_keys {
                        map.insert(
                            (*name).to_owned(),
                            data.get(*name).cloned().unwrap_or(Value::Null),
                        );
                    }
                }
                Ok(map)
            })
            .optional()?;
        if let Some(member) = member {
            members.insert(key, member);
        }
    }
    Ok(members)
}

/// Convert one column into JSON without inventing a type.
fn sql_value(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(index)? {
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        // No cache column an event allowlists holds a blob.
        ValueRef::Null | ValueRef::Blob(_) => Value::Null,
    })
}

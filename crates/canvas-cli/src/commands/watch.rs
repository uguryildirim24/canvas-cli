//! `canvas watch` (class C): the local event stream (agent-UX REPORT §3.6).
//!
//! Watch is a resident consumer. It holds the shared identity lock for its
//! whole life, so `identity remove` reports busy (§3.4). Each tick expires the
//! retention window, applies any observation an earlier run left pending, and
//! then refreshes the §10 datasets whose TTL has run out. The TTLs do the
//! staggering: watch promises no universal freshness and no 60 s guarantee.
//!
//! Events are never invented here. A tick emits exactly the rows the event log
//! gained, in cursor order, one complete `event@1` document per line. Replay
//! is at least once, so a consumer deduplicates by cursor.
//!
//! Priority (§3.6): while a foreground `submit` or `plan execute` holds
//! interest, watch admits no new polling request and holds no slot. A journal
//! that is still in flight has the same effect. A terminal `outcome_unknown`
//! journal is **not** in flight: it must never freeze polling.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use canvas_core::events::{CursorCheck, EventRecord, check_cursor, expire, read_after};
use canvas_core::sync::{
    ContextWindow, CoursesScope, PeriodKey, PlannerWindow, RefreshOutcome, SyncError,
    refresh_announcements, refresh_assignments, refresh_courses, refresh_enrollment_grades,
    refresh_missing, refresh_planner,
};
use jiff::tz::TimeZone;

use super::Globals;
use super::course_load::{load_courses_for_scope, map_source, u64_count};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    EventJson, SCHEMA_EVENT, SCHEMA_WATCH, SyncDatasetJson, WatchResult, now_timestamp,
};
use crate::session::{
    Session, ttl_announcements, ttl_assignments, ttl_courses, ttl_grades, ttl_missing, ttl_planner,
};

/// How long a tick waits before the next one, unless `--once` ends the run.
const TICK: Duration = Duration::from_secs(30);
/// First wait after a scope fails, doubled on each further failure.
const BACKOFF_MIN: Duration = Duration::from_secs(30);
/// Ceiling for that doubling, so a broken scope is still retried.
const BACKOFF_MAX: Duration = Duration::from_secs(900);
/// How many replayed events one read takes.
const REPLAY_BATCH: usize = 500;

/// Run `canvas watch`.
pub async fn run(globals: &Globals, jsonl: bool, since: Option<String>, once: bool) -> ExitCode {
    if globals.offline {
        return emit_error(
            globals.json,
            "usage",
            "watch cannot be used with --offline",
            2,
            globals.profile.clone(),
            None,
        );
    }
    let since = match since.as_deref().map(parse_cursor) {
        None => None,
        Some(Ok(cursor)) => Some(cursor),
        Some(Err(message)) => {
            return emit_error(
                globals.json,
                "usage",
                &message,
                2,
                globals.profile.clone(),
                None,
            );
        }
    };

    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let client = match super::emit::require_client(globals, &session) {
        Ok(client) => client.clone(),
        Err(code) => return code,
    };
    if let Err(e) = session.validate_network_token().await {
        return super::emit::sync_error(globals, &session, &e);
    }

    let mut stream = Stream::new(jsonl, &session);
    match watch(globals, &session, &client, &mut stream, since, once).await {
        Ok(result) => finish(globals, &session, &mut stream, result),
        Err(code) => code,
    }
}

/// A `--since` value: a decimal cursor, as §7 writes every id.
fn parse_cursor(raw: &str) -> Result<i64, String> {
    raw.parse::<i64>()
        .ok()
        .filter(|cursor| *cursor >= 0)
        .ok_or_else(|| format!("--since expects a decimal cursor, not {raw:?}"))
}

/// The stream half of the command: one line per event, in either form.
struct Stream {
    jsonl: bool,
    zone: TimeZone,
    identity: crate::output::IdentityRef,
    /// Last cursor written, so the summary can report the position.
    cursor: Option<i64>,
    /// Events written, replayed and new.
    events: u64,
}

impl Stream {
    fn new(jsonl: bool, session: &Session) -> Self {
        Self {
            jsonl,
            zone: session.time_zone(),
            identity: session.identity_ref(),
            cursor: None,
            events: 0,
        }
    }

    /// Write one event document and flush it, so a reader sees it at once.
    fn write(&mut self, record: &EventRecord) -> io::Result<()> {
        let mut out = io::stdout().lock();
        if self.jsonl {
            let document = EventJson {
                schema: SCHEMA_EVENT.to_owned(),
                cursor: record.cursor.to_string(),
                kind: record.kind.clone(),
                observed_at: record.observed_at.clone(),
                observed_at_local: local(&record.observed_at, &self.zone),
                identity: self.identity.clone(),
                generation: record.generation.clone(),
                dataset: record.dataset.clone(),
                scope: record.scope.clone(),
                entity_key: record.entity_key.clone(),
                before: record.before.clone(),
                after: record.after.clone(),
            };
            serde_json::to_writer(&mut out, &document)
                .map_err(|e| io::Error::other(e.to_string()))?;
            writeln!(out)?;
        } else {
            writeln!(
                out,
                "{}  {}  {}:{}  {}",
                record.cursor,
                record.kind,
                record.dataset,
                record.scope,
                record.entity_key.as_deref().unwrap_or("-")
            )?;
        }
        out.flush()?;
        self.cursor = Some(record.cursor);
        self.events += 1;
        Ok(())
    }
}

/// Render a stored UTC timestamp in the identity's zone (§7 `ts+local`).
fn local(raw: &str, zone: &TimeZone) -> Option<String> {
    let at = raw.parse::<jiff::Timestamp>().ok()?;
    let zoned = at.to_zoned(zone.clone());
    Some(format!("{}{}", zoned.datetime(), zoned.strftime("%:z")))
}

/// What the run accumulated, for the `watch@1` summary.
#[derive(Default)]
struct Summary {
    since: Option<i64>,
    ticks: u64,
    resync: bool,
    skipped: Option<String>,
    datasets: BTreeMap<(String, String), SyncDatasetJson>,
}

/// Replay, then tick until `--once` or SIGINT ends the run.
async fn watch(
    globals: &Globals,
    session: &Session,
    client: &canvas_api::Client,
    stream: &mut Stream,
    since: Option<i64>,
    once: bool,
) -> Result<Summary, ExitCode> {
    let mut summary = Summary {
        since,
        ..Summary::default()
    };
    let generation = session.identity.generation.to_string();

    // A cursor this log cannot replay is reported once, and the run closes
    // normally: the consumer establishes a fresh baseline (§3.2 exit row).
    if let Some(since) = since {
        let check = {
            let generation = generation.clone();
            store_call(globals, session, move |conns| {
                check_cursor(&conns.state, since, &generation)
            })?
        };
        if check == CursorCheck::Resync {
            summary.resync = true;
            let record = resync_record(session, since, &generation);
            write(globals, session, stream, &record)?;
            return Ok(summary);
        }
    }

    // Replay is at least once, in cursor order; consumers deduplicate.
    let mut cursor = since.unwrap_or(0);
    loop {
        let batch = store_call(globals, session, move |conns| {
            read_after(&conns.state, cursor, REPLAY_BATCH)
        })?;
        if batch.is_empty() {
            break;
        }
        for record in &batch {
            write(globals, session, stream, record)?;
            cursor = record.cursor;
        }
    }

    let mut backoff: BTreeMap<(String, String), (Instant, Duration)> = BTreeMap::new();
    loop {
        tick(globals, session, client, &mut summary, &mut backoff).await?;
        summary.ticks += 1;
        // Whatever the tick observed is already in the log; drain it in order.
        loop {
            let batch = store_call(globals, session, move |conns| {
                read_after(&conns.state, cursor, REPLAY_BATCH)
            })?;
            if batch.is_empty() {
                break;
            }
            for record in &batch {
                write(globals, session, stream, record)?;
                cursor = record.cursor;
            }
        }
        if once {
            break;
        }
        // SIGINT closes the stream cleanly. Every cursor is already durable in
        // `state.sqlite`, so nothing is lost by stopping between ticks.
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => break,
            () = tokio::time::sleep(tick_interval()) => {}
        }
    }
    Ok(summary)
}

/// One tick: retention, pending observations, then the due refreshes.
async fn tick(
    globals: &Globals,
    session: &Session,
    client: &canvas_api::Client,
    summary: &mut Summary,
    backoff: &mut BTreeMap<(String, String), (Instant, Duration)>,
) -> Result<(), ExitCode> {
    let now = now_timestamp();
    store_call(globals, session, move |conns| {
        expire(&mut conns.state, now).map(|_| ())
    })?;
    canvas_core::events::apply_pending(&session.open.store)
        .await
        .map_err(|e| db_error(globals, session, &e))?;

    summary.skipped = skip_reason(globals, session)?;
    if summary.skipped.is_some() {
        return Ok(());
    }
    refresh_due(globals, session, client, summary, backoff, now).await
}

/// Why this tick admits no polling request, or `None` when it may poll.
///
/// Foreground interest comes first: it is the §3.6 priority rule, and it is
/// what bounds a submission's wait at `api_concurrency = 1`. An in-flight
/// journal is the §10 pending hook. A terminal `outcome_unknown` journal is
/// neither: polling continues, so its readback can still happen.
fn skip_reason(globals: &Globals, session: &Session) -> Result<Option<String>, ExitCode> {
    match session.open.store.coordinator().foreground_interest() {
        Ok(true) => return Ok(Some("foreground_interest".to_owned())),
        Ok(false) => {}
        Err(error) => tracing::debug!(%error, "cannot read foreground interest"),
    }
    let in_flight: i64 = store_call(globals, session, |conns| {
        Ok(conns.state.query_row(
            "SELECT COUNT(*) FROM submission_journal
             WHERE state IN ('planned','uploading','uploaded','posting')",
            [],
            |r| r.get(0),
        )?)
    })?;
    Ok((in_flight > 0).then(|| "journal_in_flight".to_owned()))
}

/// Refresh every §10 dataset whose TTL has run out and whose backoff allows it.
async fn refresh_due(
    globals: &Globals,
    session: &Session,
    client: &canvas_api::Client,
    summary: &mut Summary,
    backoff: &mut BTreeMap<(String, String), (Instant, Duration)>,
    now: jiff::Timestamp,
) -> Result<(), ExitCode> {
    let store = &session.open.store;
    let record = |summary: &mut Summary, outcome: &RefreshOutcome| {
        let row = to_dataset(outcome);
        summary
            .datasets
            .insert((row.dataset.clone(), row.scope.clone()), row);
    };

    if let Some(outcome) = attempt(
        backoff,
        ("courses", "active"),
        refresh_courses(
            client,
            store,
            CoursesScope::Active,
            ttl_courses(),
            now,
            false,
            false,
        ),
    )
    .await?
    {
        record(summary, &outcome);
    }
    if let Some(outcome) = attempt(
        backoff,
        ("enrollment_grades", "none"),
        refresh_enrollment_grades(
            client,
            store,
            PeriodKey::None,
            ttl_grades(),
            now,
            false,
            false,
        ),
    )
    .await?
    {
        record(summary, &outcome);
    }

    let course_ids: Vec<i64> = store_call(globals, session, |conns| {
        Ok(load_courses_for_scope(conns, "active")?
            .into_iter()
            .map(|c| c.id)
            .collect())
    })?;
    for course_id in course_ids.iter().copied() {
        if let Some(outcome) = attempt(
            backoff,
            ("assignments", &format!("course:{course_id}")),
            refresh_assignments(
                client,
                store,
                course_id,
                ttl_assignments(),
                now,
                false,
                false,
            ),
        )
        .await?
        {
            record(summary, &outcome);
        }
    }

    if let Some(outcome) = attempt(
        backoff,
        ("missing", "self"),
        refresh_missing(client, store, ttl_missing(), now, false, false),
    )
    .await?
    {
        record(summary, &outcome);
    }
    let window = PlannerWindow::todo_default(now.to_zoned(session.time_zone()).date(), 14);
    if let Some(outcome) = attempt(
        backoff,
        ("planner", "default"),
        refresh_planner(
            client,
            store,
            window.clone(),
            ttl_planner(),
            now,
            false,
            false,
        ),
    )
    .await?
    {
        record(summary, &outcome);
    }
    let announcements = attempt(backoff, ("announcements", "courses"), async {
        refresh_announcements(
            client,
            store,
            ContextWindow::courses(window, &course_ids),
            ttl_announcements(),
            now,
            false,
            false,
        )
        .await
        .map(|batch| batch.outcome)
    })
    .await?;
    if let Some(outcome) = announcements {
        record(summary, &outcome);
    }
    Ok(())
}

/// Run one refresh unless its scope is backing off; fold the result either way.
///
/// A failed scope never stops the stream: it waits, doubling up to
/// [`BACKOFF_MAX`], and every other dataset keeps its own schedule. An
/// authentication failure is different — no wait fixes it — so it ends the run
/// with the §14 exit its classification names.
async fn attempt<F>(
    backoff: &mut BTreeMap<(String, String), (Instant, Duration)>,
    key: (&str, &str),
    future: F,
) -> Result<Option<RefreshOutcome>, ExitCode>
where
    F: std::future::Future<Output = Result<RefreshOutcome, SyncError>>,
{
    let key = (key.0.to_owned(), key.1.to_owned());
    if let Some((until, _)) = backoff.get(&key)
        && Instant::now() < *until
    {
        return Ok(None);
    }
    match future.await {
        Ok(outcome) => {
            backoff.remove(&key);
            Ok(Some(outcome))
        }
        Err(error) => {
            if error.classification().1 == 3 {
                return Err(ExitCode::from(3));
            }
            let wait = backoff
                .get(&key)
                .map_or(BACKOFF_MIN, |(_, wait)| (*wait * 2).min(BACKOFF_MAX));
            tracing::debug!(dataset = %key.0, scope = %key.1, %error, "refresh failed; backing off");
            backoff.insert(key, (Instant::now() + wait, wait));
            Ok(None)
        }
    }
}

/// Write the closing `watch@1` document and exit 0.
fn finish(globals: &Globals, session: &Session, stream: &mut Stream, summary: Summary) -> ExitCode {
    let datasets: Vec<SyncDatasetJson> = summary.datasets.into_values().collect();
    let result = WatchResult {
        since: summary.since.map(|c| c.to_string()),
        cursor: stream.cursor.map(|c| c.to_string()),
        events: stream.events,
        ticks: summary.ticks,
        resync_required: summary.resync,
        skipped: summary.skipped,
        datasets,
    };
    let mut envelope = base_envelope(SCHEMA_WATCH, session, result);
    envelope.freshness = envelope
        .result
        .datasets
        .iter()
        .map(SyncDatasetJson::to_freshness)
        .collect();
    envelope.requests = session.requests();
    let _ = globals;
    emit(stream.jsonl, &envelope, || {
        let mut out = io::stdout().lock();
        writeln!(
            out,
            "{} event(s), {} tick(s), cursor {}",
            envelope.result.events,
            envelope.result.ticks,
            envelope.result.cursor.as_deref().unwrap_or("-")
        )
    })
}

/// The `resync_required` document for a cursor this log cannot replay.
fn resync_record(session: &Session, since: i64, generation: &str) -> EventRecord {
    EventRecord {
        cursor: since,
        observation_id: format!("resync:{since}"),
        kind: canvas_core::events::EventKind::ResyncRequired
            .as_str()
            .to_owned(),
        observed_at: now_timestamp().to_string(),
        identity_key: session.identity.key.to_string(),
        generation: generation.to_owned(),
        dataset: "events".to_owned(),
        scope: "cursor".to_owned(),
        entity_key: Some(since.to_string()),
        before: serde_json::json!({ "cursor": since.to_string() }),
        after: serde_json::json!({ "cursor": null }),
    }
}

fn write(
    globals: &Globals,
    session: &Session,
    stream: &mut Stream,
    record: &EventRecord,
) -> Result<(), ExitCode> {
    stream.write(record).map_err(|e| {
        emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        )
    })
}

/// Run one store job, mapping a database failure to the §14 exit 13.
fn store_call<T, F>(globals: &Globals, session: &Session, f: F) -> Result<T, ExitCode>
where
    F: FnOnce(&mut canvas_core::store::StoreConns) -> Result<T, canvas_core::store::DbError>
        + Send
        + 'static,
    T: Send + 'static,
{
    session
        .open
        .store
        .call_blocking(f)
        .map_err(|e| db_error(globals, session, &e))
}

fn db_error(globals: &Globals, session: &Session, error: &canvas_core::store::DbError) -> ExitCode {
    emit_error(
        globals.json,
        "local",
        &error.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// How long a tick waits, shortened by tests through the environment.
fn tick_interval() -> Duration {
    if cfg!(debug_assertions)
        && let Ok(raw) = std::env::var("CANVAS_TEST_WATCH_TICK_MS")
        && let Ok(millis) = raw.parse::<u64>()
    {
        return Duration::from_millis(millis);
    }
    TICK
}

fn to_dataset(outcome: &RefreshOutcome) -> SyncDatasetJson {
    SyncDatasetJson {
        dataset: outcome.freshness.dataset.clone(),
        scope: outcome.freshness.scope.clone(),
        source: map_source(outcome.freshness.source),
        fetched_at: Some(outcome.freshness.fetched_at.to_string()),
        complete: outcome.freshness.complete,
        count: u64_count(outcome.freshness.count),
        stale: outcome.freshness.stale,
        requests: u64::from(outcome.requests),
        error: outcome.error.clone(),
    }
}

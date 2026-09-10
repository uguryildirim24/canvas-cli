//! `canvas calendar` (class C, §12.5).

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::ics::{CalendarItem, write_ics};
use canvas_core::store::{DbError, StoreConns, WindowQuery, lookup_dataset};
use canvas_core::sync::{
    BatchOutcome, CalendarEventsDataset, ContextWindow, CoursesScope, PlannerWindow,
    refresh_calendar_events,
};
use canvas_core::todo::{self, TodoFilters, TodoItem, TodoWindow};
use comfy_table::Row;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde_json::Value;

use super::announcements::{denial_scopes, json_string};
use super::course_load::{RefreshFail, cached_outcome_with_error, outcome_freshness};
use super::duration::rfc_duration;
use super::emit::{base_envelope, emit_error, session_error, sync_error};
use super::handled::Handled;
use super::{Globals, assignment_read as read};
use crate::output::{
    CalendarItemJson, CalendarResult, Freshness, Outcome, SCHEMA_CALENDAR, WindowJson,
    apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_assignments, ttl_calendar, ttl_courses};

/// Default window (§12.5, shared with `todo`).
const DEFAULT_DAYS: u32 = 14;

/// Operands of `canvas calendar`.
#[derive(Debug, Clone, Default)]
pub struct CalendarArgs {
    pub days: Option<u32>,
    pub course: Option<String>,
    pub ics: Option<String>,
    pub alarm: Option<String>,
}

/// Where `--ics` writes.
enum IcsTarget {
    None,
    Stdout,
    Path(std::path::PathBuf),
}

/// Run `canvas calendar [--days N] [--course <course>] [--ics PATH|-] [--alarm DURATION]`.
#[allow(clippy::too_many_lines)]
/// Run `canvas calendar` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, args: CalendarArgs) -> ExitCode {
    handle(globals, args).await.emit(globals.json)
}

pub async fn handle(globals: &Globals, args: CalendarArgs) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let days = args.days.unwrap_or(DEFAULT_DAYS);
    if days == 0
        || now_timestamp()
            .to_zoned(TimeZone::UTC)
            .date()
            .checked_add(jiff::Span::new().days(i64::from(days)))
            .is_err()
    {
        return usage(
            &session,
            "days must be positive and fit the supported date range",
        );
    }
    // The alarm is a usage operand, so its grammar is checked here rather
    // than left to the writer: `canvas calendar --alarm` must exit 2 (§14)
    // whether or not `--ics` asks for a file.
    let alarm = match args.alarm.as_deref() {
        None => None,
        Some(raw) => match rfc_duration(raw).filter(|d| canvas_core::ics::valid_alarm(d)) {
            Some(alarm) => Some(alarm),
            None => {
                return usage(&session, "--alarm takes a duration such as 30m, 24h, or 2d");
            }
        },
    };
    let target = match args.ics.as_deref() {
        None => IcsTarget::None,
        Some("-") => IcsTarget::Stdout,
        Some(path) => IcsTarget::Path(std::path::PathBuf::from(path)),
    };

    let now = now_timestamp();
    let zone = read::zone(&session);
    let today = now.to_zoned(zone.clone()).date();
    let window = PlannerWindow::todo_default(today, days);

    let mut freshness: Vec<Freshness> = Vec::new();
    let course_id = if let Some(course) = args.course.as_deref() {
        match super::course::resolve_with_refresh(globals, &session, course).await {
            Ok((resolved, rows, _)) => {
                freshness.extend(rows);
                Some(resolved.id)
            }
            Err(code) => return code,
        }
    } else {
        None
    };

    // The planner window is the same one `todo` caches, so both share it.
    let planner_scope = match read::planner(&session, globals, window.clone()).await {
        Ok(outcome) => {
            let scope = outcome.freshness.scope.clone();
            freshness.push(outcome_freshness(&outcome));
            scope
        }
        Err(e) => return sync_error(&session, &e),
    };

    let contexts = match contexts_for(globals, &session, &mut freshness).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    let context_window = ContextWindow::contexts(window.clone(), &contexts);
    let events = match ensure_calendar_events(globals, &session, &context_window).await {
        Ok(o) => o,
        Err(e) => return super::course::refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&events.outcome));

    let todo_window = TodoWindow {
        start: window.start,
        end: window.end,
        days,
    };
    // A wider cached window can serve a narrower request (§10), so the rows
    // come from the scope of the row that answered.
    let event_scope = events.outcome.freshness.scope.clone();
    let loaded = session
        .open
        .store
        .call({
            let window = window.clone();
            let zone = zone.clone();
            move |conns| {
                // `PlannerSourceRow` is not a nameable type outside `todo`,
                // so the window filter and the merge stay in one closure.
                let mut planner = todo::load_planner_rows(conns, &planner_scope)?;
                planner.retain(|row| in_window(&row.data_json, &window));
                let mut codes = BTreeMap::new();
                let mut pending = BTreeMap::new();
                for row in &planner {
                    if let Some(cid) = row.course_id
                        && let Some(code) = todo::course_code(conns, cid)?
                    {
                        codes.insert(cid, code);
                    }
                    if let Some(id) = row.plannable_id {
                        pending.insert(id, todo::assignment_pending(conns, id)?);
                    }
                }
                let (items, _) = todo::build_todo_in_zone(
                    &planner,
                    &[],
                    &codes,
                    &pending,
                    now,
                    today,
                    &todo_window,
                    &TodoFilters {
                        // The calendar shows everything the window covers.
                        all: true,
                        course_id,
                        ..TodoFilters::default()
                    },
                    ttl_assignments(),
                    &todo::assignment_observations(conns)?,
                    &zone,
                );
                let events = load_calendar_events(conns, &event_scope)?;
                Ok((items, events))
            }
        })
        .await;
    let (deadlines, event_rows) = match loaded {
        Ok(v) => v,
        Err(e) => return sync_error(&session, &e.into()),
    };

    let key = session.identity.key.to_string();
    let mut merged: BTreeMap<(String, i64), Merged> = BTreeMap::new();
    for item in deadlines {
        merged.insert(
            (item.kind.as_str().to_owned(), item.id),
            Merged::from_todo(&item, &zone),
        );
    }
    let mut warnings = Vec::new();
    for row in event_rows {
        if course_id.is_some_and(|id| row.course_id != Some(id)) || !row.in_window(&window) {
            continue;
        }
        // The calendar-events representation wins for event fields (§12.5).
        if let Some(message) = row.longer_span_warning() {
            warnings.push(message);
        }
        merged.insert(("event".to_owned(), row.id), row.to_merged(&zone));
    }

    let mut undated = 0u64;
    let mut rows: Vec<Merged> = merged
        .into_values()
        .filter(|item| {
            let dated =
                item.all_day_date.is_some() || item.start_at.is_some() || item.due_at.is_some();
            if !dated {
                undated += 1;
            }
            dated
        })
        .collect();
    rows.sort_by_key(|row| row.sort_key(&key));
    // Two events can share a title, and the one-day rule warns about each of
    // them with the same words.
    warnings.dedup();
    if undated > 0 {
        warnings.push(format!("{undated} item(s) without a date are not shown"));
    }
    let items: Vec<CalendarItemJson> = rows.iter().map(|row| row.to_json(&key, &zone)).collect();

    // Only `--ics` needs the calendar text. Building it for a plain listing
    // would let an unwritable item (§12.5) fail a table that never asked for
    // a file; the one-day warnings above are the command's own.
    let text = if matches!(target, IcsTarget::None) {
        String::new()
    } else {
        match write_calendar(&rows, &key, &warnings, alarm.as_deref(), now) {
            Ok(document) => {
                warnings = document.warnings;
                document.text
            }
            // Not a usage error: the operands were accepted and the items came
            // from Canvas, so this is exit 1 (§14).
            Err(e) => {
                return emit_error(
                    "generic",
                    &format!("cannot write iCalendar: {e}"),
                    1,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
        }
    };

    if let IcsTarget::Path(path) = &target
        && let Err(e) = std::fs::write(path, text.as_bytes())
    {
        return emit_error(
            "local",
            &format!("cannot write {}: {e}", path.display()),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }

    let result = CalendarResult {
        window: WindowJson {
            start: window.start.to_string(),
            end: window.end.to_string(),
        },
        items,
    };
    let mut envelope = base_envelope(SCHEMA_CALENDAR, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    envelope.warnings = warnings;
    if events.outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale calendar_events cache".into());
    }
    let codes = super::announcements::course_labels(&session, &events.denials).await;
    for scope in denial_scopes("calendar_events", "Calendar", &events.denials, &codes) {
        envelope.warnings.push(scope.message.clone());
        envelope.partial.push(scope);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    // `--ics -` streams raw text with no envelope (§7). The envelope still
    // carries the outcome, so the exit and the warnings stay the command's.
    if matches!(target, IcsTarget::Stdout) {
        return Handled::raw(envelope, text.into_bytes());
    }

    let written = match &target {
        IcsTarget::Path(path) => Some(path.display().to_string()),
        IcsTarget::None | IcsTarget::Stdout => None,
    };
    Handled::new(envelope, move |envelope| {
        if let Some(path) = &written {
            writeln!(
                io::stdout(),
                "wrote {} event(s) to {path}",
                envelope.result.items.len()
            )
        } else {
            print_table(&envelope.result.items, &zone)
        }
    })
}

/// Render the ICS text and collect the one-day conversion warnings.
fn write_calendar(
    rows: &[Merged],
    identity_key: &str,
    warnings: &[String],
    alarm: Option<&str>,
    now: Timestamp,
) -> Result<canvas_core::ics::IcsDocument, canvas_core::ics::IcsError> {
    let ics_items: Vec<CalendarItem> = rows
        .iter()
        .map(|row| row.to_ics(identity_key, alarm))
        .collect();
    let mut document = write_ics(&ics_items, now)?;
    // The one-day warning belongs to the command, not only to the file, so it
    // is computed before the write and merged here without repeats.
    let mut merged = warnings.to_vec();
    for warning in document.warnings {
        if !merged.contains(&warning) {
            merged.push(warning);
        }
    }
    document.warnings = merged;
    Ok(document)
}

/// One merged calendar row before it becomes JSON.
struct Merged {
    kind: String,
    id: i64,
    course_id: Option<i64>,
    course_code: Option<String>,
    title: String,
    is_deadline: bool,
    due_at: Option<Timestamp>,
    start_at: Option<Timestamp>,
    end_at: Option<Timestamp>,
    all_day: bool,
    all_day_date: Option<Date>,
    html_url: Option<String>,
    /// `DESCRIPTION` body: points and status for a deadline (§12.5).
    description: Option<String>,
}

impl Merged {
    fn from_todo(item: &TodoItem, _zone: &TimeZone) -> Self {
        let is_deadline = item.due_at.is_some();
        let mut parts = Vec::new();
        if let Some(points) = item.points_possible {
            parts.push(format!("{points} points"));
        }
        parts.push(read::label(item));
        Self {
            kind: item.kind.as_str().to_owned(),
            id: item.id,
            course_id: item.course_id,
            course_code: item.course_code.clone(),
            title: item.title.clone(),
            is_deadline,
            due_at: item.due_at,
            start_at: if is_deadline { None } else { item.scheduled_at },
            end_at: None,
            all_day: false,
            all_day_date: None,
            html_url: item.html_url.clone(),
            description: Some(parts.join(" · ")).filter(|d| !d.is_empty()),
        }
    }

    /// Appendix D: `(all_day_date ?? start_at ?? due_at)` ascending, then
    /// `uid`, so the JSON and the ICS carry one order.
    fn sort_key(&self, identity_key: &str) -> (String, String) {
        let when = self
            .all_day_date
            .map(|d| d.to_string())
            .or_else(|| self.start_at.map(|at| at.to_string()))
            .or_else(|| self.due_at.map(|at| at.to_string()))
            .unwrap_or_default();
        (when, self.uid(identity_key))
    }

    fn uid(&self, identity_key: &str) -> String {
        format!("canvas-{}-{}@{identity_key}", self.kind, self.id)
    }

    fn to_ics(&self, identity_key: &str, alarm: Option<&str>) -> CalendarItem {
        CalendarItem {
            kind: self.kind.clone(),
            id: self.id,
            identity_key: identity_key.to_owned(),
            course_code: self.course_code.clone(),
            title: self.title.clone(),
            is_deadline: self.is_deadline,
            due_at: self.due_at,
            start_at: self.start_at,
            end_at: self.end_at,
            all_day: self.all_day,
            all_day_date: self.all_day_date,
            url: self.html_url.clone(),
            description: self.description.clone(),
            alarm: alarm.map(str::to_owned),
        }
    }

    fn to_json(&self, identity_key: &str, zone: &TimeZone) -> CalendarItemJson {
        CalendarItemJson {
            uid: self.uid(identity_key),
            kind: self.kind.clone(),
            id: self.id.to_string(),
            course_id: self.course_id.map(|id| id.to_string()),
            course_code: self.course_code.clone(),
            title: self.title.clone(),
            is_deadline: self.is_deadline,
            due_at: self.due_at.map(|at| at.to_string()),
            due_at_local: read::local(self.due_at, zone),
            start_at: self.start_at.map(|at| at.to_string()),
            start_at_local: read::local(self.start_at, zone),
            end_at: self.end_at.map(|at| at.to_string()),
            end_at_local: read::local(self.end_at, zone),
            all_day: self.all_day,
            all_day_date: self.all_day_date.map(|d| d.to_string()),
            html_url: self.html_url.clone(),
        }
    }
}

/// Contexts the events fetch covers: the user plus every active course
/// (§12.5). `--course` filters what is shown, so one cached window serves
/// every invocation.
async fn contexts_for(
    globals: &Globals,
    session: &Session,
    freshness: &mut Vec<Freshness>,
) -> Result<Vec<String>, Handled> {
    let courses = match super::course_load::ensure_courses(
        session,
        CoursesScope::Active,
        ttl_courses(),
        now_timestamp(),
        globals.fresh,
        globals.offline,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return Err(super::course::refresh_fail(session, e)),
    };
    freshness.push(outcome_freshness(&courses));
    let ids = session
        .open
        .store
        .call(|conns| {
            Ok(super::course_load::load_courses_for_scope(conns, "active")?
                .into_iter()
                .map(|row| row.id)
                .collect::<Vec<_>>())
        })
        .await
        .map_err(|e| {
            emit_error(
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            )
        })?;
    let mut contexts = vec![format!("user_{}", session.identity.user_id)];
    contexts.extend(ids.into_iter().map(|id| format!("course_{id}")));
    Ok(contexts)
}

/// Serve the events window from cache, or refresh it over the network.
pub(crate) async fn ensure_calendar_events(
    globals: &Globals,
    session: &Session,
    window: &ContextWindow,
) -> Result<BatchOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_calendar();
    let dataset = CalendarEventsDataset::new(window.clone(), ttl);
    let lookup = session
        .open
        .store
        .call({
            let query = window.clone();
            move |conns| {
                lookup_dataset(
                    conns,
                    &dataset,
                    now,
                    Some(WindowQuery {
                        start: query.window().start_timestamp(),
                        end: query.window().end_timestamp(),
                        contexts: query.context_hash(),
                    }),
                )
            }
        })
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome_with_error(lookup, globals.fresh, globals.offline)? {
        return Ok(BatchOutcome {
            denials: super::announcements::decode_denials(outcome.error.as_deref()),
            outcome,
        });
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_calendar_events(
        client,
        &session.open.store,
        window.clone(),
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

/// True when a planner row's date falls in the window (undated rows stay).
fn in_window(data_json: &str, window: &PlannerWindow) -> bool {
    let data: Value = serde_json::from_str(data_json).unwrap_or(Value::Null);
    let at = data
        .get("plannable_date")
        .or_else(|| data.get("due_at"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Timestamp>().ok());
    at.is_none_or(|t| t >= window.start_timestamp() && t < window.end_timestamp())
}

/// One cached calendar event.
struct EventRow {
    id: i64,
    course_id: Option<i64>,
    course_code: Option<String>,
    title: Option<String>,
    start_at: Option<Timestamp>,
    end_at: Option<Timestamp>,
    all_day: bool,
    all_day_date: Option<Date>,
    html_url: Option<String>,
}

impl EventRow {
    /// Keep the answer inside the window the command asked for: an all-day
    /// event by its civil date, a timed event by its start.
    fn in_window(&self, window: &PlannerWindow) -> bool {
        if let Some(date) = self.all_day_date.filter(|_| self.all_day) {
            return date >= window.start && date <= window.end;
        }
        self.start_at
            .is_none_or(|at| at >= window.start_timestamp() && at < window.end_timestamp())
    }

    /// §12.5: an all-day event is one civil day; a longer span is warned about.
    fn longer_span_warning(&self) -> Option<String> {
        let (start, end) = self.start_at.zip(self.end_at)?;
        (self.all_day && start != end).then(|| {
            format!(
                "{}: all-day event with a longer span is shown as one day in v1",
                self.title.clone().unwrap_or_default()
            )
        })
    }

    fn to_merged(&self, zone: &TimeZone) -> Merged {
        let all_day_date = if self.all_day {
            self.all_day_date
                .or_else(|| self.start_at.map(|at| at.to_zoned(zone.clone()).date()))
        } else {
            None
        };
        Merged {
            kind: "event".to_owned(),
            id: self.id,
            course_id: self.course_id,
            course_code: self.course_code.clone(),
            title: self.title.clone().unwrap_or_default(),
            is_deadline: false,
            due_at: None,
            start_at: self.start_at,
            end_at: self.end_at,
            all_day: self.all_day,
            all_day_date,
            html_url: self.html_url.clone(),
            // §12.5 puts points and status in DESCRIPTION; an event has
            // neither.
            description: None,
        }
    }
}

fn load_calendar_events(conns: &StoreConns, scope: &str) -> Result<Vec<EventRow>, DbError> {
    let mut stmt = conns.cache.prepare(
        "SELECT e.id, e.title, e.start_at, e.end_at, e.context_code, e.data_json, c.course_code
         FROM membership m
         INNER JOIN calendar_events e ON e.id = CAST(m.entity_id AS INTEGER)
         LEFT JOIN courses c
                ON 'course_' || c.id = e.context_code
         WHERE m.dataset = 'calendar_events' AND m.scope = ?1
           AND m.entity_kind = 'calendar_event'
         ORDER BY m.position, e.id",
    )?;
    let rows = stmt
        .query_map([scope], |r| {
            let raw: String = r.get(5)?;
            let data: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            let context: Option<String> = r.get(4)?;
            Ok(EventRow {
                id: r.get(0)?,
                course_id: context
                    .as_deref()
                    .and_then(canvas_core::sync::course_id_from_context),
                course_code: r.get(6)?,
                title: r.get(1)?,
                start_at: r
                    .get::<_, Option<String>>(2)?
                    .and_then(|at| at.parse().ok()),
                end_at: r
                    .get::<_, Option<String>>(3)?
                    .and_then(|at| at.parse().ok()),
                all_day: data
                    .get("all_day")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                all_day_date: json_string(&data, "all_day_date").and_then(|d| d.parse().ok()),
                html_url: json_string(&data, "html_url"),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn usage(session: &Session, message: &str) -> Handled {
    emit_error(
        "usage",
        message,
        2,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn print_table(items: &[CalendarItemJson], zone: &TimeZone) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(io::stdout(), "no calendar items in this window");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec!["WHEN", "COURSE", "KIND", "TITLE"]));
    for item in items {
        table.add_row(Row::from(vec![
            when_label(item, zone),
            item.course_code
                .clone()
                .or_else(|| item.course_id.clone())
                .unwrap_or_default(),
            if item.is_deadline {
                format!("{} due", item.kind)
            } else {
                item.kind.clone()
            },
            item.title.clone(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn when_label(item: &CalendarItemJson, zone: &TimeZone) -> String {
    if let Some(date) = item.all_day_date.as_deref() {
        return format!("{date} all day");
    }
    let at = item
        .start_at
        .as_deref()
        .or(item.due_at.as_deref())
        .and_then(|s| s.parse::<Timestamp>().ok());
    at.map(|at| {
        let zoned = at.to_zoned(zone.clone());
        format!("{} {}", zoned.date(), zoned.strftime("%H:%M"))
    })
    .unwrap_or_default()
}

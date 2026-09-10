//! `canvas announcements` (class C, §12.6).

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{
    AnnouncementsDataset, BatchOutcome, ContextDenial, ContextWindow, CoursesScope, PlannerWindow,
    refresh_announcements,
};
use comfy_table::Row;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};
use serde_json::Value;

use super::course_load::{RefreshFail, cached_outcome_with_error, outcome_freshness};
use super::duration::{duration_in_days, parse_duration};
use super::emit::{base_envelope, emit, emit_error, session_error};
use super::{Globals, assignment_read as read};
use crate::output::{
    AnnouncementJson, AnnouncementsResult, Outcome, PartialScope, SCHEMA_ANNOUNCEMENTS, WindowJson,
    apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_announcements, ttl_courses};

/// Default `--since` window (§12.6).
const DEFAULT_SINCE_DAYS: u32 = 14;

/// Run `canvas announcements [<course>] [--since DURATION] [--unread]`.
pub async fn run(
    globals: &Globals,
    course: Option<String>,
    since: Option<String>,
    unread: bool,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let Some((days, cutoff)) = window_span(since.as_deref()) else {
        return emit_error(
            globals.json,
            "usage",
            "--since takes a duration such as 24h, 7d, or 2w",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };

    let now = now_timestamp();
    let zone = read::zone(&session);
    let today = now.to_zoned(zone.clone()).date();
    let window = since_window(today, days);
    let cutoff = cutoff.and_then(|span| now.checked_sub(span).ok());

    let mut freshness = Vec::new();
    // The fetch always covers the active courses, so every invocation shares
    // one cached window (§12.6); `<course>` filters what is shown.
    let course_ids = match active_course_ids(globals, &session, &mut freshness).await {
        Ok(ids) => ids,
        Err(code) => return code,
    };
    let only_course = if let Some(course) = course {
        match super::course::resolve_with_refresh(globals, &session, &course).await {
            Ok((resolved, rows, _)) => {
                for row in rows {
                    if !freshness.iter().any(|f: &crate::output::Freshness| {
                        f.dataset == row.dataset && f.scope == row.scope
                    }) {
                        freshness.push(row);
                    }
                }
                Some(resolved.id)
            }
            Err(code) => return code,
        }
    } else {
        None
    };

    let context_window = ContextWindow::courses(window.clone(), &course_ids);
    let batch = match ensure_announcements(globals, &session, &context_window).await {
        Ok(o) => o,
        Err(e) => return super::course::refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&batch.outcome));

    // A wider cached window can serve a narrower request (§10), so the rows
    // come from the scope of the row that answered, not from the request.
    let scope = batch.outcome.freshness.scope.clone();
    let rows = match session
        .open
        .store
        .call(move |conns| load_announcements(conns, &scope))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            return emit_error(
                globals.json,
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    let start = window.start_timestamp();
    let end = window.end_timestamp();
    let mut items: Vec<AnnouncementJson> = rows
        .iter()
        .filter(|row| only_course.is_none_or(|id| row.course_id == Some(id)))
        // Keep the answer inside the window the command asked for.
        .filter(|row| row.posted_at.is_none_or(|at| at >= start && at < end))
        .filter(|row| cutoff.is_none_or(|at| row.posted_at.is_none_or(|posted| posted >= at)))
        .map(|row| row.to_json(&zone))
        .collect();
    if unread {
        // `--unread` filters locally on `read_state`; nothing is marked read.
        items.retain(|item| !item.read);
    }
    sort_announcements(&mut items);

    let result = AnnouncementsResult {
        window: WindowJson {
            start: window.start.to_string(),
            end: window.end.to_string(),
        },
        announcements: items,
    };
    let mut envelope = base_envelope(SCHEMA_ANNOUNCEMENTS, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if batch.outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale announcements cache".into());
    }
    let codes = course_labels(&session, &batch.denials).await;
    for scope in denial_scopes("announcements", "Announcements", &batch.denials, &codes) {
        envelope.warnings.push(scope.message.clone());
        envelope.partial.push(scope);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    emit(globals.json, &envelope, || {
        print_table(&envelope.result.announcements, &zone)
    })
}

/// `--since` looks back: the window ends today and starts `days` before it.
fn since_window(today: Date, days: u32) -> PlannerWindow {
    let start = today
        .checked_sub(Span::new().days(i64::from(days)))
        .unwrap_or(today);
    PlannerWindow { start, end: today }
}

/// Days of coverage and the exact cutoff `--since` asks for.
///
/// The fetch window is whole civil days, so a duration shorter than a day is
/// still fetched as one day and filtered to the exact instant.
fn window_span(since: Option<&str>) -> Option<(u32, Option<Span>)> {
    match since {
        None => Some((DEFAULT_SINCE_DAYS, None)),
        Some(raw) => {
            let span = parse_duration(raw)?;
            Some((duration_in_days(span)?, Some(span)))
        }
    }
}

/// Every active course: the fetch set for the window (§12.6).
async fn active_course_ids(
    globals: &Globals,
    session: &Session,
    freshness: &mut Vec<crate::output::Freshness>,
) -> Result<Vec<i64>, ExitCode> {
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
        Err(e) => return Err(super::course::refresh_fail(globals, session, e)),
    };
    freshness.push(outcome_freshness(&courses));
    session
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
                globals.json,
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            )
        })
}

/// Serve the window from cache, or refresh it over the network.
pub(crate) async fn ensure_announcements(
    globals: &Globals,
    session: &Session,
    window: &ContextWindow,
) -> Result<BatchOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_announcements();
    let dataset = AnnouncementsDataset::new(window.clone(), ttl);
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
                    Some(canvas_core::store::WindowQuery {
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
            denials: decode_denials(outcome.error.as_deref()),
            outcome,
        });
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_announcements(
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

pub(crate) fn decode_denials(error: Option<&str>) -> Vec<ContextDenial> {
    error
        .map(canvas_core::store::parse_context_denials)
        .unwrap_or_default()
        .into_iter()
        .map(|(context, http_status)| ContextDenial {
            context,
            http_status,
        })
        .collect()
}

/// `partial[]` rows naming the contexts that stayed unreadable (§12.6).
///
/// `dataset` is the §10 dataset name and `subject` the word the message uses,
/// so `calendar` reports its own denials instead of relabelling these. A
/// course scope is `<dataset>:course:<id>`, as in the §7 envelope; any other
/// context keeps its own code, which is the shape `sync` records too.
pub(crate) fn denial_scopes(
    dataset: &str,
    subject: &str,
    denials: &[ContextDenial],
    codes: &HashMap<String, String>,
) -> Vec<PartialScope> {
    denials
        .iter()
        .map(|denial| {
            let (scope, label) = match denial.course_id() {
                Some(id) => {
                    let id = id.to_string();
                    let label = codes.get(&id).map_or_else(|| id.clone(), Clone::clone);
                    (format!("{dataset}:course:{id}"), label)
                }
                None => (
                    format!("{dataset}:{}", denial.context),
                    denial.context.clone(),
                ),
            };
            PartialScope {
                scope,
                http_status: Some(denial.http_status),
                message: format!(
                    "{subject} for {label} unavailable (HTTP {})",
                    denial.http_status
                ),
            }
        })
        .collect()
}

/// Course codes for the denied courses, so `partial[]` names them the way
/// the student sees them.
pub(crate) async fn course_labels(
    session: &Session,
    denials: &[ContextDenial],
) -> HashMap<String, String> {
    let ids: Vec<i64> = denials
        .iter()
        .filter_map(ContextDenial::course_id)
        .collect();
    if ids.is_empty() {
        return HashMap::new();
    }
    session
        .open
        .store
        .call(move |conns| {
            use rusqlite::OptionalExtension;
            let mut out = HashMap::new();
            for id in ids {
                let code: Option<String> = conns
                    .cache
                    .query_row("SELECT course_code FROM courses WHERE id = ?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()?
                    .flatten();
                if let Some(code) = code {
                    out.insert(id.to_string(), code);
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default()
}

/// Appendix D: `posted_at` descending, then `id`.
pub(crate) fn sort_announcements(items: &mut [AnnouncementJson]) {
    items.sort_by(|a, b| {
        b.posted_at
            .cmp(&a.posted_at)
            .then_with(|| numeric_id(&a.id).cmp(&numeric_id(&b.id)))
    });
}

fn numeric_id(id: &str) -> (i64, String) {
    (id.parse().unwrap_or(i64::MAX), id.to_owned())
}

/// One cached announcement row with its course code.
#[derive(Debug, Clone)]
pub(crate) struct AnnouncementRow {
    pub id: i64,
    pub course_id: Option<i64>,
    pub course_code: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub posted_at: Option<Timestamp>,
    pub data: Value,
}

impl AnnouncementRow {
    pub fn to_json(&self, zone: &TimeZone) -> AnnouncementJson {
        AnnouncementJson {
            id: self.id.to_string(),
            course_id: self.course_id.map(|id| id.to_string()),
            course_code: self.course_code.clone(),
            title: self.title.clone().unwrap_or_default(),
            posted_at: self.posted_at.map(|at| at.to_string()),
            posted_at_local: read::local(self.posted_at, zone),
            author: json_string(&self.data, "author"),
            read: json_string(&self.data, "read_state").as_deref() == Some("read"),
            html_url: json_string(&self.data, "html_url"),
        }
    }
}

const ROW_SELECT: &str = "SELECT a.id, a.course_id, c.course_code, a.title, a.message,
            a.posted_at, a.data_json
     FROM announcements a
     LEFT JOIN courses c ON c.id = a.course_id";

fn read_row(r: &rusqlite::Row<'_>) -> Result<AnnouncementRow, rusqlite::Error> {
    let raw: String = r.get(6)?;
    Ok(AnnouncementRow {
        id: r.get(0)?,
        course_id: r.get(1)?,
        course_code: r.get(2)?,
        title: r.get(3)?,
        message: r.get(4)?,
        posted_at: r
            .get::<_, Option<String>>(5)?
            .and_then(|at| at.parse().ok()),
        data: serde_json::from_str(&raw).unwrap_or(Value::Null),
    })
}

/// Rows this window covers, in membership order.
fn load_announcements(conns: &StoreConns, scope: &str) -> Result<Vec<AnnouncementRow>, DbError> {
    let sql = format!(
        "{ROW_SELECT}
         INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = a.id
         WHERE m.dataset = 'announcements' AND m.scope = ?1
           AND m.entity_kind = 'announcement'
         ORDER BY m.position, a.id"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    Ok(stmt
        .query_map([scope], read_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

/// One announcement by id, whatever window last covered it.
pub(crate) fn load_announcement(
    conns: &StoreConns,
    id: i64,
) -> Result<Option<AnnouncementRow>, DbError> {
    use rusqlite::OptionalExtension;
    let sql = format!("{ROW_SELECT} WHERE a.id = ?1");
    Ok(conns.cache.query_row(&sql, [id], read_row).optional()?)
}

pub(crate) fn json_string(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn print_table(items: &[AnnouncementJson], zone: &TimeZone) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(io::stdout(), "no announcements in this window");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec!["POSTED", "COURSE", "ID", "TITLE", "READ"]));
    for item in items {
        table.add_row(Row::from(vec![
            item.posted_at
                .as_deref()
                .and_then(|at| at.parse::<Timestamp>().ok())
                .map(|at| local_day(at, zone))
                .unwrap_or_default(),
            item.course_code
                .clone()
                .or_else(|| item.course_id.clone())
                .unwrap_or_default(),
            item.id.clone(),
            item.title.clone(),
            if item.read { "read" } else { "unread" }.to_owned(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn local_day(at: Timestamp, zone: &TimeZone) -> String {
    let zoned = at.to_zoned(zone.clone());
    let date: Date = zoned.date();
    format!("{date} {}", zoned.strftime("%H:%M"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7: a denied course is `<dataset>:course:<id>`; any other context keeps
    /// its own code, so `calendar`'s `user_<id>` context is never dressed up
    /// as a course.
    #[test]
    fn denial_scopes_name_the_dataset_and_the_context() {
        let denials = vec![
            ContextDenial {
                context: "course_45679".into(),
                http_status: 403,
            },
            ContextDenial {
                context: "user_12345".into(),
                http_status: 403,
            },
        ];
        let mut codes = HashMap::new();
        codes.insert("45679".to_owned(), "CS-101".to_owned());

        let rows = denial_scopes("announcements", "Announcements", &denials, &codes);
        assert_eq!(rows[0].scope, "announcements:course:45679");
        assert_eq!(
            rows[0].message,
            "Announcements for CS-101 unavailable (HTTP 403)"
        );
        assert_eq!(rows[1].scope, "announcements:user_12345");

        let rows = denial_scopes("calendar_events", "Calendar", &denials, &codes);
        assert_eq!(rows[0].scope, "calendar_events:course:45679");
        assert_eq!(
            rows[0].message,
            "Calendar for CS-101 unavailable (HTTP 403)"
        );
        // The same shape `sync` records for a context that is not a course.
        assert_eq!(rows[1].scope, "calendar_events:user_12345");
        assert_eq!(rows[1].http_status, Some(403));
    }
}

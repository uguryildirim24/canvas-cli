//! Shared course row loading and freshness conversion.

use canvas_core::store::{
    DbError, FetchLogRow, LookupResult, StoreConns, load_fetch_log, lookup_dataset,
};
use canvas_core::sync::{
    CoursesDataset, CoursesScope, FreshnessInfo, FreshnessSource as SyncFreshnessSource,
    RefreshOutcome, SyncError, refresh_courses,
};
use jiff::{Span, Timestamp};
use rusqlite::OptionalExtension;
use serde_json::{Map, Value};

use crate::output::{
    CourseJson, Freshness, FreshnessSource, GradeJson, PeriodJson, TeacherJson, TermJson,
};
use crate::session::Session;

/// Errors from a cache-or-network courses refresh.
#[derive(Debug)]
pub enum RefreshFail {
    OfflineMiss,
    NeedAuth,
    Sync(SyncError),
    Db(DbError),
}

/// Refresh (or serve) the courses dataset for `scope`.
pub async fn ensure_courses(
    session: &Session,
    scope: CoursesScope,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, RefreshFail> {
    let dataset = CoursesDataset::new(scope, ttl);
    let lookup = session
        .open
        .store
        .call({
            let dataset = dataset.clone();
            move |conns| lookup_dataset(conns, &dataset, now, None)
        })
        .await
        .map_err(RefreshFail::Db)?;

    let grades_stale = session
        .open
        .store
        .call(move |conns| {
            let rows = load_courses_for_scope(conns, scope.as_str())?;
            Ok(grade_freshness(conns, &rows, now, false, None)?
                .iter()
                .any(|f| f.stale))
        })
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, fresh || grades_stale, offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_courses(
        client,
        &session.open.store,
        scope,
        ttl,
        now,
        fresh || grades_stale,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

fn complete_row(lookup: LookupResult) -> Option<FetchLogRow> {
    match lookup {
        LookupResult::Hit(row) | LookupResult::Stale(row) if row.complete => Some(row),
        LookupResult::Hit(_) | LookupResult::Stale(_) | LookupResult::Miss => None,
    }
}

fn cache_outcome(row: &FetchLogRow, stale: bool) -> RefreshOutcome {
    RefreshOutcome {
        freshness: FreshnessInfo {
            dataset: row.dataset.clone(),
            scope: row.scope.clone(),
            source: SyncFreshnessSource::Cache,
            fetched_at: row.fetched_at,
            complete: row.complete,
            count: row.count,
            stale,
        },
        requests: 0,
        error: None,
    }
}

fn cache_outcome_with_error(row: &FetchLogRow, stale: bool) -> RefreshOutcome {
    RefreshOutcome {
        freshness: FreshnessInfo {
            dataset: row.dataset.clone(),
            scope: row.scope.clone(),
            source: SyncFreshnessSource::Cache,
            fetched_at: row.fetched_at,
            complete: row.complete,
            count: row.count,
            stale,
        },
        requests: 0,
        error: row.error.clone(),
    }
}

/// Like [`cached_outcome`], but keeps `fetch_log.error` (listing denials).
pub fn cached_outcome_with_error(
    lookup: LookupResult,
    fresh: bool,
    offline: bool,
) -> Result<Option<RefreshOutcome>, RefreshFail> {
    if let LookupResult::Hit(row) = &lookup
        && !fresh
        && !offline
    {
        return Ok(Some(cache_outcome_with_error(row, false)));
    }
    if offline {
        return complete_row(lookup)
            .map(|row| Some(cache_outcome_with_error(&row, true)))
            .ok_or(RefreshFail::OfflineMiss);
    }
    Ok(None)
}

/// Convert a sync freshness source to an envelope source.
#[must_use]
pub fn map_source(source: SyncFreshnessSource) -> FreshnessSource {
    match source {
        SyncFreshnessSource::Cache => FreshnessSource::Cache,
        SyncFreshnessSource::Network => FreshnessSource::Network,
    }
}

/// Convert a refresh outcome into an envelope freshness row.
#[must_use]
pub fn outcome_freshness(outcome: &RefreshOutcome) -> Freshness {
    Freshness {
        dataset: outcome.freshness.dataset.clone(),
        scope: outcome.freshness.scope.clone(),
        source: map_source(outcome.freshness.source),
        fetched_at: Some(outcome.freshness.fetched_at.to_string()),
        complete: outcome.freshness.complete,
        count: u64_count(outcome.freshness.count),
        stale: outcome.freshness.stale,
    }
}

/// Convert a non-negative count.
#[must_use]
pub fn u64_count(n: i64) -> Option<u64> {
    u64::try_from(n).ok()
}

/// One course row plus joined term / totals used by list and detail.
#[derive(Debug, Clone)]
pub struct CourseRow {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub html_url: String,
    pub term_id: Option<i64>,
    pub term_name: Option<String>,
    pub term_start: Option<String>,
    pub term_end: Option<String>,
    pub enrollment_state: String,
    pub is_favorite: bool,
    pub restricted: bool,
    pub data_json: Value,
    pub current_score: Option<f64>,
    pub current_grade: Option<String>,
    pub final_score: Option<f64>,
    pub final_grade: Option<String>,
    pub period_mode: String,
    pub period_id: Option<String>,
    pub period_title: Option<String>,
}

type CourseSqlRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn map_sql_row(row: CourseSqlRow) -> CourseRow {
    let (
        id,
        code,
        name,
        html_url,
        term_id,
        data_raw,
        term_name,
        term_start,
        term_end,
        current_score,
        current_grade,
        final_score,
        final_grade,
        totals_raw,
        period_mode,
    ) = row;
    let data_json: Value =
        serde_json::from_str(&data_raw).unwrap_or_else(|_| Value::Object(Map::default()));
    let (period_id, period_title) = period_meta(totals_raw.as_deref());
    CourseRow {
        id,
        code: code.unwrap_or_default(),
        name: name.unwrap_or_default(),
        html_url: html_url.unwrap_or_default(),
        term_id,
        term_name,
        term_start,
        term_end,
        enrollment_state: data_json
            .get("enrollment_state")
            .and_then(Value::as_str)
            .unwrap_or("active")
            .to_owned(),
        is_favorite: data_json
            .get("is_favorite")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        restricted: data_json
            .get("restricted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        data_json,
        current_score,
        current_grade,
        final_score,
        final_grade,
        period_mode: period_mode.unwrap_or_else(|| "all".into()),
        period_id,
        period_title,
    }
}

fn read_course_columns(r: &rusqlite::Row<'_>) -> Result<CourseSqlRow, rusqlite::Error> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
    ))
}

const COURSE_SELECT: &str =
    "SELECT c.id, c.course_code, c.name, c.html_url, c.term_id, c.data_json,
                t.name, t.start_at, t.end_at,
                ct.current_score, ct.current_grade, ct.final_score, ct.final_grade, ct.data_json, ct.mode";

/// Load membership courses for a scope, with term and totals for the default period mode.
pub fn load_courses_for_scope(conns: &StoreConns, scope: &str) -> Result<Vec<CourseRow>, DbError> {
    let sql = format!(
        "{COURSE_SELECT}
         FROM membership m
         INNER JOIN courses c ON c.id = CAST(m.entity_id AS INTEGER)
         LEFT JOIN terms t ON t.id = c.term_id
         LEFT JOIN course_totals ct ON ct.course_id = c.id AND ct.mode = CASE WHEN json_extract(c.data_json, '$.has_grading_periods') = 1 OR EXISTS (SELECT 1 FROM course_totals current WHERE current.course_id=c.id AND current.mode='current' AND json_extract(current.data_json,'$.period_id') IS NOT NULL) THEN 'current' ELSE 'all' END
         WHERE m.dataset = 'courses' AND m.scope = ?1 AND m.entity_kind = 'course'"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    let rows = stmt
        .query_map([scope], read_course_columns)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| bind_period_scores(conns, map_sql_row(row)))
        .collect()
}

/// Load one course by id (no membership requirement).
pub fn load_course_by_id(conns: &StoreConns, id: i64) -> Result<Option<CourseRow>, DbError> {
    let sql = format!(
        "{COURSE_SELECT}
         FROM courses c
         LEFT JOIN terms t ON t.id = c.term_id
         LEFT JOIN course_totals ct ON ct.course_id = c.id AND ct.mode = CASE WHEN json_extract(c.data_json, '$.has_grading_periods') = 1 OR EXISTS (SELECT 1 FROM course_totals current WHERE current.course_id=c.id AND current.mode='current' AND json_extract(current.data_json,'$.period_id') IS NOT NULL) THEN 'current' ELSE 'all' END
         WHERE c.id = ?1"
    );
    let row = conns
        .cache
        .query_row(&sql, [id], read_course_columns)
        .optional()?;
    row.map(|row| bind_period_scores(conns, map_sql_row(row)))
        .transpose()
}

fn period_meta(totals_raw: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = totals_raw else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return (None, None);
    };
    let id = v.get("period_id").and_then(|x| {
        x.as_str()
            .map(str::to_owned)
            .or_else(|| x.as_i64().map(|n| n.to_string()))
    });
    let title = v
        .get("period_title")
        .and_then(Value::as_str)
        .map(str::to_owned);
    (id, title)
}

impl CourseRow {
    /// Convert to list JSON with the course's default grade period.
    #[must_use]
    pub fn to_course_json(&self) -> CourseJson {
        CourseJson {
            id: self.id.to_string(),
            code: self.code.clone(),
            name: self.name.clone(),
            term: TermJson {
                id: self.term_id.map(|id| id.to_string()),
                name: self.term_name.clone(),
                start_at: self.term_start.clone(),
                end_at: self.term_end.clone(),
            },
            enrollment_state: self.enrollment_state.clone(),
            is_favorite: self.is_favorite,
            restricted: self.restricted,
            html_url: self.html_url.clone(),
            grades: self.grades(),
        }
    }

    /// Grades from the selected `course_totals` mode.
    #[must_use]
    pub fn grades(&self) -> GradeJson {
        GradeJson {
            current_score: self.current_score,
            current_grade: self.current_grade.clone(),
            final_score: self.final_score,
            final_grade: self.final_grade.clone(),
            period: PeriodJson {
                mode: self.period_mode.clone(),
                id: self.period_id.clone(),
                title: self.period_title.clone(),
            },
        }
    }

    /// Teachers from `data_json` when present.
    #[must_use]
    pub fn teachers(&self) -> Vec<TeacherJson> {
        let Some(arr) = self.data_json.get("teachers").and_then(Value::as_array) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|t| {
                let id = t.get("id").and_then(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })?;
                let name = t
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                Some(TeacherJson { id, name })
            })
            .collect()
    }

    /// Syllabus markdown from `data_json` when present.
    #[must_use]
    pub fn syllabus_markdown(&self) -> Option<String> {
        self.data_json
            .get("syllabus_markdown")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                self.data_json
                    .get("syllabus_body")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
    }

    /// Time zone from `data_json` when present.
    #[must_use]
    pub fn time_zone(&self) -> Option<String> {
        self.data_json
            .get("time_zone")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    /// Modules count from `data_json` when present.
    #[must_use]
    pub fn modules_count(&self) -> Option<u64> {
        self.data_json.get("modules_count").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        })
    }
}

/// Parse courses scope from the `IncompleteDataset` error scope string.
#[must_use]
pub fn scope_from_str(scope: &str) -> CoursesScope {
    if scope == "all" {
        CoursesScope::All
    } else {
        CoursesScope::Active
    }
}

/// Read a `fetch_log` row for envelope freshness (optional).
#[allow(dead_code)]
pub fn fetch_log_freshness(
    conns: &StoreConns,
    dataset: &str,
    scope: &str,
) -> Result<Option<Freshness>, DbError> {
    Ok(
        load_fetch_log(&conns.cache, dataset, scope)?.map(|row| Freshness {
            dataset: row.dataset,
            scope: row.scope,
            source: FreshnessSource::Cache,
            fetched_at: Some(row.fetched_at.to_string()),
            complete: row.complete,
            count: u64_count(row.count),
            stale: row.stale,
        }),
    )
}

pub fn cached_outcome(
    lookup: LookupResult,
    fresh: bool,
    offline: bool,
) -> Result<Option<RefreshOutcome>, RefreshFail> {
    if let LookupResult::Hit(row) = &lookup
        && !fresh
        && !offline
    {
        return Ok(Some(cache_outcome(row, false)));
    }
    if offline {
        return complete_row(lookup)
            .map(|row| Some(cache_outcome(&row, true)))
            .ok_or(RefreshFail::OfflineMiss);
    }
    Ok(None)
}

/// Totals have a shorter TTL and independent observation clocks from the course list.
///
/// `mode` names the `course_totals` mode the caller actually read; `None`
/// means the course's own default mode.
pub fn grade_freshness(
    conns: &StoreConns,
    rows: &[CourseRow],
    now: Timestamp,
    offline: bool,
    mode: Option<&str>,
) -> Result<Vec<Freshness>, DbError> {
    use canvas_core::sync::CourseTotalsDataset;
    let ttl = crate::session::ttl_grades();
    rows.iter().map(|row| {
        let dataset = CourseTotalsDataset::new(row.id, ttl);
        let lookup = lookup_dataset(conns, &dataset, now, None)?;
        let (log, hit) = match lookup { LookupResult::Hit(log) => (Some(log), true), LookupResult::Stale(log) => (Some(log), false), LookupResult::Miss => (None, false) };
        let key = format!("{}|{}", row.id, mode.unwrap_or(&row.period_mode));
        let oldest: Option<String> = conns.cache.query_row("SELECT MIN(observed_at) FROM field_obs WHERE entity_kind='course_totals' AND entity_key=?1", [key], |r| r.get(0))?;
        let has_values = row.current_score.is_some() || row.current_grade.is_some() || row.final_score.is_some() || row.final_grade.is_some();
        let fields_fresh = if let Some(raw) = oldest { let at: Timestamp = raw.parse().map_err(|_| DbError::Message("invalid grade observation timestamp".into()))?; at <= now && at.checked_add(ttl).is_ok_and(|expiry| now <= expiry) } else { !has_values };
        Ok(Freshness { dataset: "course_totals".into(), scope: format!("course:{}",row.id), source: FreshnessSource::Cache,
            fetched_at: log.as_ref().map(|l| l.fetched_at.to_string()), complete: log.as_ref().is_some_and(|l| l.complete), count: log.as_ref().and_then(|l| u64_count(l.count)), stale: offline || !hit || !fields_fresh })
    }).collect()
}

/// A current-period label cannot make scores observed for an earlier period applicable.
fn bind_period_scores(conns: &StoreConns, mut row: CourseRow) -> Result<CourseRow, DbError> {
    if row.period_mode != "current" {
        return Ok(row);
    }
    let changed: Option<String> = conns.cache.query_row("SELECT json_extract(data_json,'$.period_changed_at') FROM course_totals WHERE course_id=?1 AND mode='current'", [row.id], |r|r.get(0)).optional()?.flatten();
    let Some(changed) = changed else {
        return Ok(row);
    };
    let changed: Timestamp = changed
        .parse()
        .map_err(|_| DbError::Message("invalid period transition timestamp".into()))?;
    let key = format!("{}|current", row.id);
    for name in [
        "current_score",
        "final_score",
        "current_grade",
        "final_grade",
    ] {
        let observed: Option<String> = conns.cache.query_row("SELECT observed_at FROM field_obs WHERE entity_kind='course_totals' AND entity_key=?1 AND field=?2", rusqlite::params![key,name], |r|r.get(0)).optional()?;
        let belongs = observed
            .as_deref()
            .and_then(|v| v.parse::<Timestamp>().ok())
            .is_some_and(|at| at >= changed);
        if !belongs {
            match name {
                "current_score" => row.current_score = None,
                "final_score" => row.final_score = None,
                "current_grade" => row.current_grade = None,
                _ => row.final_grade = None,
            }
        }
    }
    Ok(row)
}

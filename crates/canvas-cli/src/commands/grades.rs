//! `canvas grades` (class C, SPEC §12.4).
//!
//! v1 shows Canvas-reported values only; no local estimation.

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::store::{StoreConns, lookup_dataset};
use canvas_core::sync::{
    AssignmentGroupsDataset, CoursesScope, EnrollmentGradesDataset, GradingPeriodsDataset,
    PeriodKey, RefreshOutcome, refresh_assignment_groups, refresh_enrollment_grades,
    refresh_grading_periods,
};
use comfy_table::Row;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{
    CourseRow, RefreshFail, cached_outcome, ensure_courses, load_course_by_id,
    load_courses_for_scope, outcome_freshness,
};
use super::emit::{base_envelope, emit, emit_error, session_error};
use super::grades_load::{
    PeriodSelection, PeriodTotals, course_totals_for_mode, enrollment_grades_by_course,
    load_assignment_groups, load_grading_periods, parse_period,
};
use crate::output::{
    CourseBaseJson, GradeJson, GradesCourseJson, GradesCourseViewJson, GradesResult, PeriodJson,
    SCHEMA_GRADES, TermJson, apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_courses, ttl_grades};

/// Run `canvas grades [<course>] [--period current|all|ID]`.
pub async fn run(globals: &Globals, course: Option<String>, period: Option<String>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let requested = match period.as_deref().map(parse_period).transpose() {
        Ok(v) => v,
        Err(bad) => {
            return emit_error(
                globals.json,
                "usage",
                &format!("--period must be current, all, or a grading period id, not {bad}"),
                2,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    // Baseline request 1: the course list (and the totals that ride with it).
    let courses_outcome = match ensure_courses(
        &session,
        CoursesScope::Active,
        ttl_courses(),
        now_timestamp(),
        globals.fresh,
        globals.offline,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };

    let selected = match course {
        Some(ref operand) => match resolve_with_refresh(globals, &session, operand).await {
            Ok((resolved, _, _)) => Some(resolved.id),
            Err(code) => return code,
        },
        None => None,
    };

    match build(globals, &session, selected, requested, &courses_outcome).await {
        Ok(envelope) => {
            let stale = courses_outcome.freshness.stale;
            let mut envelope = envelope;
            if stale {
                envelope.warnings.push("served stale courses cache".into());
            }
            envelope.requests = session.requests();
            emit(globals.json, &envelope, || print_grades(&envelope.result))
        }
        Err(code) => code,
    }
}

type Envelope = crate::output::Envelope<GradesResult>;

async fn build(
    globals: &Globals,
    session: &Session,
    selected: Option<i64>,
    requested: Option<PeriodSelection>,
    courses_outcome: &RefreshOutcome,
) -> Result<Envelope, ExitCode> {
    let now = now_timestamp();
    let rows = load_rows(globals, session, selected).await?;

    // With no flag the default follows the course: `current` when it has
    // grading periods, `all` when it does not (SPEC §12.4).
    let period = requested.unwrap_or_else(|| default_period(&rows));

    let mut envelope = base_envelope(
        SCHEMA_GRADES,
        session,
        GradesResult {
            period_mode: period.mode().to_owned(),
            courses: Vec::new(),
            course: None,
        },
    );
    envelope.freshness.push(outcome_freshness(courses_outcome));

    // The period list validates an explicit id and titles it. It is read before
    // that period's enrollments so an unknown id is a resolution failure (exit
    // 6), which SPEC §14 detects ahead of an offline miss (exit 7).
    let mut periods = Vec::new();
    if let Some(course_id) = selected {
        let outcome = ensure_grading_periods(globals, session, course_id).await?;
        envelope.freshness.push(outcome_freshness(&outcome));
        periods = session
            .open
            .store
            .call(move |conns| load_grading_periods(conns, course_id, now))
            .await
            .map_err(|e| local_error(globals, session, &e.to_string()))?;
    }

    // A period the selected course does not have has nothing to fetch: the
    // course reports `unavailable` (SPEC §12.4) rather than failing.
    let absent_here = matches!(period, PeriodSelection::Id(id) if selected.is_some()
        && !periods.iter().any(|p| p.id == id.to_string()));

    // Baseline request 2: enrollments. An explicit period adds one more.
    let by_course = if absent_here {
        std::collections::HashMap::new()
    } else {
        let enrollments = ensure_enrollment_grades(globals, session, period.period_key()).await?;
        envelope.freshness.push(outcome_freshness(&enrollments));
        let period_key = period.period_key();
        session
            .open
            .store
            .call(move |conns| enrollment_grades_by_course(conns, period_key))
            .await
            .map_err(|e| local_error(globals, session, &e.to_string()))?
    };

    // With no course operand there is no period list to check against, so an id
    // is unknown only when Canvas reported no grades for it anywhere.
    if let PeriodSelection::Id(id) = period
        && selected.is_none()
        && by_course.is_empty()
    {
        return Err(unknown_period(globals, session, id));
    }

    let mut courses = Vec::new();
    for row in &rows {
        let totals = totals_for(globals, session, row, period, &by_course).await?;
        courses.push(course_json(row, period, &periods, &totals));
    }
    courses.sort_by(|a, b| {
        a.course
            .code
            .cmp(&b.course.code)
            .then_with(|| a.course.id.cmp(&b.course.id))
    });
    envelope.result.courses = courses;

    if let Some(course_id) = selected {
        // A period the course does not have has no groups either.
        let groups = if absent_here {
            Vec::new()
        } else {
            let outcome =
                ensure_assignment_groups(globals, session, course_id, period.period_key()).await?;
            envelope.freshness.push(outcome_freshness(&outcome));
            let zone = session.time_zone();
            let key = period.period_key();
            session
                .open
                .store
                .call(move |conns| load_assignment_groups(conns, course_id, key, &zone))
                .await
                .map_err(|e| local_error(globals, session, &e.to_string()))?
        };
        envelope.result.course = Some(GradesCourseViewJson { groups, periods });
    }
    Ok(envelope)
}

fn unknown_period(globals: &Globals, session: &Session, id: i64) -> ExitCode {
    emit_error(
        globals.json,
        "resolution",
        &format!("no grading period {id} for this identity"),
        6,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

async fn load_rows(
    globals: &Globals,
    session: &Session,
    selected: Option<i64>,
) -> Result<Vec<CourseRow>, ExitCode> {
    let rows = session
        .open
        .store
        .call(move |conns: &mut StoreConns| match selected {
            Some(id) => Ok(load_course_by_id(conns, id)?.into_iter().collect()),
            None => load_courses_for_scope(conns, "active"),
        })
        .await
        .map_err(|e| local_error(globals, session, &e.to_string()))?;
    Ok(rows)
}

/// With no `--period`, a course that has grading periods defaults to `current`.
fn default_period(rows: &[CourseRow]) -> PeriodSelection {
    if rows.iter().any(|r| r.period_mode == "current") {
        PeriodSelection::Current
    } else {
        PeriodSelection::All
    }
}

/// Totals for one course under the selected period.
async fn totals_for(
    globals: &Globals,
    session: &Session,
    row: &CourseRow,
    period: PeriodSelection,
    by_course: &std::collections::HashMap<i64, PeriodTotals>,
) -> Result<PeriodTotals, ExitCode> {
    // An explicit period is answered by that period's enrollments only; the
    // whole-course and current-period totals belong to other modes.
    if matches!(period, PeriodSelection::Id(_)) {
        return Ok(by_course.get(&row.id).cloned().unwrap_or_default());
    }
    let Some(mode) = period.totals_mode() else {
        return Ok(PeriodTotals::default());
    };
    let course_id = row.id;
    let mode = mode.to_owned();
    let totals = session
        .open
        .store
        .call(move |conns| course_totals_for_mode(conns, course_id, &mode))
        .await
        .map_err(|e| local_error(globals, session, &e.to_string()))?;
    if totals.covered {
        return Ok(totals);
    }
    // Canvas answers the default modes on the course list; the unqualified
    // enrollments only fill a course the list did not cover. Those values are
    // whole-course ones, so they may stand in for `all` and never for
    // `current`: a total from one mode is never labelled with another
    // (SPEC §12.4).
    if matches!(period, PeriodSelection::All) {
        Ok(by_course.get(&row.id).cloned().unwrap_or_default())
    } else {
        Ok(PeriodTotals::default())
    }
}

fn course_json(
    row: &CourseRow,
    period: PeriodSelection,
    periods: &[crate::output::GradingPeriodJson],
    totals: &PeriodTotals,
) -> GradesCourseJson {
    let (period_id, period_title) = match period {
        PeriodSelection::Id(id) => {
            let title = periods
                .iter()
                .find(|p| p.id == id.to_string())
                .map(|p| p.title.clone());
            (Some(id.to_string()), title)
        }
        PeriodSelection::Current => (totals.period_id.clone(), totals.period_title.clone()),
        PeriodSelection::All => (None, None),
    };
    let unavailable_reason = if totals.covered {
        None
    } else {
        Some(match period {
            PeriodSelection::Id(id) => format!("course has no grading period {id}"),
            PeriodSelection::Current => "no current grading period total".to_owned(),
            PeriodSelection::All => "no course total".to_owned(),
        })
    };
    GradesCourseJson {
        course: CourseBaseJson {
            id: row.id.to_string(),
            code: row.code.clone(),
            name: row.name.clone(),
            term: TermJson {
                id: row.term_id.map(|id| id.to_string()),
                name: row.term_name.clone(),
                start_at: row.term_start.clone(),
                end_at: row.term_end.clone(),
            },
            enrollment_state: row.enrollment_state.clone(),
            is_favorite: row.is_favorite,
            restricted: row.restricted,
            html_url: row.html_url.clone(),
        },
        grades: GradeJson {
            current_score: totals.current_score,
            current_grade: totals.current_grade.clone(),
            final_score: totals.final_score,
            final_grade: totals.final_grade.clone(),
            period: PeriodJson {
                mode: period.mode().to_owned(),
                id: period_id,
                title: period_title,
            },
        },
        unavailable_reason,
    }
}

async fn ensure_enrollment_grades(
    globals: &Globals,
    session: &Session,
    period: PeriodKey,
) -> Result<RefreshOutcome, ExitCode> {
    let now = now_timestamp();
    let ttl = ttl_grades();
    let ds = EnrollmentGradesDataset::new(period, ttl);
    ensure(globals, session, ds, move |client, store, fresh| {
        Box::pin(async move {
            refresh_enrollment_grades(client, store, period, ttl, now, fresh, false).await
        })
    })
    .await
}

async fn ensure_grading_periods(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, ExitCode> {
    let now = now_timestamp();
    let ttl = ttl_grades();
    let ds = GradingPeriodsDataset::new(course_id, ttl);
    ensure(globals, session, ds, move |client, store, fresh| {
        Box::pin(async move {
            refresh_grading_periods(client, store, course_id, ttl, now, fresh, false).await
        })
    })
    .await
}

async fn ensure_assignment_groups(
    globals: &Globals,
    session: &Session,
    course_id: i64,
    period: PeriodKey,
) -> Result<RefreshOutcome, ExitCode> {
    let now = now_timestamp();
    let ttl = ttl_grades();
    let ds = AssignmentGroupsDataset::new(course_id, period, ttl);
    ensure(globals, session, ds, move |client, store, fresh| {
        Box::pin(async move {
            refresh_assignment_groups(client, store, course_id, period, ttl, now, fresh, false)
                .await
        })
    })
    .await
}

type RefreshFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<RefreshOutcome, canvas_core::sync::SyncError>> + 'a,
    >,
>;

/// Serve one dataset from cache, else refresh it once.
async fn ensure<D, F>(
    globals: &Globals,
    session: &Session,
    dataset: D,
    refresh: F,
) -> Result<RefreshOutcome, ExitCode>
where
    D: canvas_core::store::Dataset + Clone + Send + Sync + 'static,
    F: for<'a> FnOnce(
        &'a canvas_api::Client,
        &'a canvas_core::store::Store,
        bool,
    ) -> RefreshFuture<'a>,
{
    let now = now_timestamp();
    let ds = dataset.clone();
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(|e| local_error(globals, session, &e.to_string()))?;
    match cached_outcome(lookup, globals.fresh, globals.offline) {
        Ok(Some(outcome)) => return Ok(outcome),
        Ok(None) => {}
        // Name the dataset that is missing, not the courses cache.
        Err(RefreshFail::OfflineMiss) => {
            return Err(emit_error(
                globals.json,
                "offline",
                &format!(
                    "offline and no complete {} cache coverage for {}",
                    dataset.name(),
                    dataset.scope_key()
                ),
                7,
                session.profile.clone(),
                Some(session.identity_ref()),
            ));
        }
        Err(e) => return Err(refresh_fail(globals, session, e)),
    }
    session
        .validate_network_token()
        .await
        .map_err(|e| refresh_fail(globals, session, RefreshFail::Sync(e)))?;
    let client = session
        .client
        .as_ref()
        .ok_or_else(|| refresh_fail(globals, session, RefreshFail::NeedAuth))?;
    refresh(client, &session.open.store, globals.fresh)
        .await
        .map_err(|e| refresh_fail(globals, session, RefreshFail::Sync(e)))
}

fn local_error(globals: &Globals, session: &Session, message: &str) -> ExitCode {
    emit_error(
        globals.json,
        "local",
        message,
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// Absent or `null` totals print `unavailable` and stay `null` in JSON.
fn total_label(grades: &GradeJson) -> String {
    grades
        .current_grade
        .clone()
        .or_else(|| grades.current_score.map(|s| format!("{s}")))
        .unwrap_or_else(|| "unavailable".into())
}

fn print_grades(result: &GradesResult) -> io::Result<()> {
    let mut out = io::stdout();
    let mut table = new_table();
    table.set_header(Row::from(vec!["CODE", "NAME", "PERIOD", "GRADE"]));
    for c in &result.courses {
        let period = c
            .grades
            .period
            .title
            .clone()
            .unwrap_or_else(|| c.grades.period.mode.clone());
        table.add_row(Row::from(vec![
            c.course.code.clone(),
            c.course.name.clone(),
            period,
            total_label(&c.grades),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(out, "{table}")?;

    let Some(ref view) = result.course else {
        return Ok(());
    };
    for group in &view.groups {
        let weight = group
            .weight
            .map_or_else(String::new, |w| format!("  ({w}%)"));
        writeln!(out, "\n{}{weight}", group.name)?;
        let mut t = new_table();
        t.set_header(Row::from(vec!["ASSIGNMENT", "SCORE", "OUT OF", "STATUS"]));
        for a in &group.assignments {
            t.add_row(Row::from(vec![
                a.name.clone(),
                a.status
                    .score
                    .map_or_else(|| "—".to_owned(), |s| format!("{s}")),
                a.points_possible
                    .map_or_else(|| "—".to_owned(), |p| format!("{p}")),
                assignment_status(a),
            ]));
        }
        apply_two_space_padding(&mut t);
        writeln!(out, "{t}")?;
        if let Some(ref sub) = group.subtotal {
            writeln!(
                out,
                "  subtotal: {} / {}",
                sub.score.map_or_else(|| "—".to_owned(), |s| format!("{s}")),
                sub.possible
                    .map_or_else(|| "—".to_owned(), |p| format!("{p}")),
            )?;
        }
    }
    Ok(())
}

fn assignment_status(a: &crate::output::GroupAssignmentJson) -> String {
    let s = &a.status;
    if s.pending {
        return "pending · unknown".into();
    }
    let mut parts = Vec::new();
    if s.missing {
        parts.push("missing".to_owned());
    }
    if s.excused == Some(true) {
        parts.push("excused".to_owned());
    }
    if s.late == Some(true) {
        parts.push("late".to_owned());
    }
    if a.omit_from_final_grade {
        parts.push("not counted".to_owned());
    }
    if parts.is_empty() {
        s.workflow_state.clone().unwrap_or_else(|| "—".into())
    } else {
        parts.join(" · ")
    }
}

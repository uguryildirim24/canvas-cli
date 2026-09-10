//! `canvas course` (class C).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{CommandClass, ResolveError, ResolvedCourse, resolve_course};
use canvas_core::store::DbError;
use canvas_core::sync::SyncError;

use super::Globals;
use super::course_load::{
    CourseRow, RefreshFail, ensure_courses, load_course_by_id, outcome_freshness, scope_from_str,
};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    CourseDetailJson, CourseResult, Freshness, GradeJson, PeriodJson, SCHEMA_COURSE, TermJson,
    now_timestamp,
};
use crate::session::{Session, ttl_courses};

/// Run `canvas course <course>`.
pub async fn run(globals: &Globals, course: String) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let (resolved, mut freshness, mut api_requests) =
        match resolve_with_refresh(globals, &session, &course).await {
            Ok(v) => v,
            Err(code) => return code,
        };

    if let Err(code) =
        maybe_refresh_active(globals, &session, &mut freshness, &mut api_requests).await
    {
        return code;
    }

    let detail = match session
        .open
        .store
        .call(move |conns| load_course_by_id(conns, resolved.id))
        .await
    {
        Ok(Some(row)) => detail_from_row(row),
        Ok(None) => detail_from_resolved(&session, &resolved),
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

    let result = CourseResult { course: detail };
    let mut envelope = base_envelope(SCHEMA_COURSE, &session, result);
    envelope.freshness = freshness;
    envelope.requests.api = api_requests;

    emit(globals.json, &envelope, || {
        print_human(&envelope.result.course)
    })
}

async fn resolve_with_refresh(
    globals: &Globals,
    session: &Session,
    course: &str,
) -> Result<(ResolvedCourse, Vec<Freshness>, u64), ExitCode> {
    let mut freshness = Vec::new();
    let mut api_requests = 0u64;
    let mut tried_refresh = false;

    loop {
        match resolve_once(session, course).await {
            Ok(Ok(r)) => return Ok((r, freshness, api_requests)),
            Ok(Err(ResolveError::IncompleteDataset { scope, .. })) if !tried_refresh => {
                tried_refresh = true;
                let outcome = ensure_courses(
                    session,
                    scope_from_str(&scope),
                    ttl_courses(),
                    now_timestamp(),
                    globals.fresh,
                    globals.offline,
                )
                .await
                .map_err(|e| refresh_fail(globals, session, e))?;
                api_requests += u64::from(outcome.requests);
                freshness.push(outcome_freshness(&outcome));
            }
            Ok(Err(e)) => return Err(resolve_exit(globals, session, &e)),
            Err(e) => {
                return Err(emit_error(
                    globals.json,
                    "local",
                    &e.to_string(),
                    13,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                ));
            }
        }
    }
}

async fn maybe_refresh_active(
    globals: &Globals,
    session: &Session,
    freshness: &mut Vec<Freshness>,
    api_requests: &mut u64,
) -> Result<(), ExitCode> {
    if !freshness.is_empty() {
        return Ok(());
    }
    match ensure_courses(
        session,
        canvas_core::sync::CoursesScope::Active,
        ttl_courses(),
        now_timestamp(),
        globals.fresh,
        globals.offline,
    )
    .await
    {
        Ok(outcome) => {
            *api_requests += u64::from(outcome.requests);
            freshness.push(outcome_freshness(&outcome));
            Ok(())
        }
        Err(RefreshFail::NeedAuth) if globals.fresh => {
            Err(refresh_fail(globals, session, RefreshFail::NeedAuth))
        }
        Err(_) => Ok(()),
    }
}

async fn resolve_once(
    session: &Session,
    course: &str,
) -> Result<Result<ResolvedCourse, ResolveError>, DbError> {
    let course = course.to_owned();
    let origin = session.identity.origin.clone();
    session
        .open
        .store
        .call(
            move |conns| match resolve_course(conns, &course, &origin, CommandClass::C) {
                Ok(r) => Ok(Ok(r)),
                Err(ResolveError::Db(e)) => Err(e),
                Err(e) => Ok(Err(e)),
            },
        )
        .await
}

fn detail_from_row(row: CourseRow) -> CourseDetailJson {
    let grades = row.grades_all();
    let teachers = row.teachers();
    let syllabus_markdown = row.syllabus_markdown();
    let time_zone = row.time_zone();
    let modules_count = row.modules_count();
    CourseDetailJson {
        id: row.id.to_string(),
        code: row.code,
        name: row.name,
        term: TermJson {
            id: row.term_id.map(|id| id.to_string()),
            name: row.term_name,
            start_at: row.term_start,
            end_at: row.term_end,
        },
        enrollment_state: row.enrollment_state,
        is_favorite: row.is_favorite,
        restricted: row.restricted,
        html_url: row.html_url,
        grades,
        teachers,
        syllabus_markdown,
        time_zone,
        modules_count,
    }
}

fn detail_from_resolved(session: &Session, resolved: &ResolvedCourse) -> CourseDetailJson {
    CourseDetailJson {
        id: resolved.id.to_string(),
        code: resolved.code.clone().unwrap_or_default(),
        name: resolved.name.clone().unwrap_or_default(),
        term: TermJson {
            id: None,
            name: None,
            start_at: None,
            end_at: None,
        },
        enrollment_state: String::new(),
        is_favorite: false,
        restricted: false,
        html_url: format!("{}/courses/{}", session.identity.origin, resolved.id),
        grades: GradeJson {
            current_score: None,
            current_grade: None,
            final_score: None,
            final_grade: None,
            period: PeriodJson {
                mode: "all".into(),
                id: None,
                title: None,
            },
        },
        teachers: Vec::new(),
        syllabus_markdown: None,
        time_zone: None,
        modules_count: None,
    }
}

fn refresh_fail(globals: &Globals, session: &Session, err: RefreshFail) -> ExitCode {
    match err {
        RefreshFail::OfflineMiss | RefreshFail::Sync(SyncError::OfflineMiss) => emit_error(
            globals.json,
            "offline",
            "offline and no complete courses cache coverage",
            7,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        RefreshFail::NeedAuth => emit_error(
            globals.json,
            "auth",
            "no token; set CANVAS_TOKEN or run auth login",
            3,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        RefreshFail::Sync(e) => emit_error(
            globals.json,
            "sync",
            &e.to_string(),
            4,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        RefreshFail::Db(e) => emit_error(
            globals.json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

fn print_human(c: &CourseDetailJson) -> io::Result<()> {
    let grade = c
        .grades
        .current_grade
        .clone()
        .or_else(|| c.grades.current_score.map(|s| format!("{s}")))
        .unwrap_or_else(|| "—".into());
    writeln!(
        io::stdout(),
        "{code}  {name}\nterm: {term}\ngrade: {grade}\n{url}",
        code = c.code,
        name = c.name,
        term = c.term.name.as_deref().unwrap_or("—"),
        url = c.html_url,
    )
}

fn resolve_exit(globals: &Globals, session: &Session, err: &ResolveError) -> ExitCode {
    let message = match err {
        ResolveError::NotFound { .. } => "course not found".to_owned(),
        ResolveError::Ambiguous { .. } => "ambiguous course".to_owned(),
        ResolveError::NeedIdOrUrl => "use a numeric ID or a URL".to_owned(),
        ResolveError::OriginMismatch => "URL origin does not match identity".to_owned(),
        ResolveError::CourseIdMismatch { .. } => "course id mismatch".to_owned(),
        other => other.to_string(),
    };
    emit_error(
        globals.json,
        "resolution",
        &message,
        6,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

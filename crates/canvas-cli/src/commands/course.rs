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
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
use crate::output::{
    CourseDetailJson, CourseResult, Freshness, SCHEMA_COURSE, TermJson, now_timestamp,
};
use crate::session::{Session, ttl_courses};

/// Run `canvas course` for the CLI: one envelope, one exit code.
pub async fn run(globals: &Globals, course: String) -> ExitCode {
    handle(globals, course).await.emit(globals.json)
}

/// Run `canvas course <course>`.
pub async fn handle(globals: &Globals, course: String) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };

    let (resolved, mut freshness, _api_requests) =
        match resolve_with_refresh(globals, &session, &course).await {
            Ok(v) => v,
            Err(code) => return code,
        };

    let outcome = match ensure_detail(globals, &session, resolved.id).await {
        Ok(outcome) => outcome,
        Err(error) => return refresh_fail(&session, error),
    };
    freshness.push(outcome_freshness(&outcome));

    let (mut detail, mut grade_freshness) = match session
        .open
        .store
        .call({
            let offline = globals.offline;
            move |conns| {
                let row = load_course_by_id(conns, resolved.id)?;
                let freshness = super::course_load::grade_freshness(
                    conns,
                    row.as_slice(),
                    now_timestamp(),
                    offline,
                    None,
                )?;
                Ok((row, freshness))
            }
        })
        .await
    {
        Ok((Some(row), freshness)) => (detail_from_row(row), freshness),
        Ok((None, _)) => {
            return emit_error(
                "offline",
                "no cached course detail",
                7,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => {
            return emit_error(
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    if detail.html_url.is_empty() {
        detail.html_url = format!("{}/courses/{}", session.identity.origin, resolved.id);
    }
    let result = CourseResult { course: detail };
    let mut envelope = base_envelope(SCHEMA_COURSE, &session, result);
    envelope.freshness = freshness;
    if outcome.freshness.source == canvas_core::sync::FreshnessSource::Network {
        for row in &mut grade_freshness {
            row.source = crate::output::FreshnessSource::Network;
        }
    }
    envelope.freshness.extend(grade_freshness);
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale course detail".into());
    }

    Handled::new(envelope, move |envelope| {
        print_human(&envelope.result.course)
    })
}

/// Resolve a course for class-C commands, refreshing courses once on incomplete cache.
pub(crate) async fn resolve_with_refresh(
    globals: &Globals,
    session: &Session,
    course: &str,
) -> Result<(ResolvedCourse, Vec<Freshness>, u64), Handled> {
    let mut freshness = Vec::new();
    let mut api_requests = 0u64;
    let mut refreshed = std::collections::HashSet::new();

    loop {
        match resolve_once(session, course).await {
            Ok(Ok(r)) => {
                let used = resolver_freshness(session, course, r.id, globals.offline)
                    .await
                    .map_err(|e| refresh_fail(session, RefreshFail::Db(e)))?;
                for row in used {
                    if !freshness
                        .iter()
                        .any(|f: &Freshness| f.dataset == row.dataset && f.scope == row.scope)
                    {
                        freshness.push(row);
                    }
                }
                return Ok((r, freshness, api_requests));
            }
            Ok(Err(ResolveError::IncompleteDataset { scope, .. }))
                if refreshed.insert(scope.clone()) =>
            {
                let outcome = ensure_courses(
                    session,
                    scope_from_str(&scope),
                    ttl_courses(),
                    now_timestamp(),
                    globals.fresh,
                    globals.offline,
                )
                .await
                .map_err(|e| refresh_fail(session, e))?;
                api_requests += u64::from(outcome.requests);
                freshness.push(outcome_freshness(&outcome));
            }
            Ok(Err(e)) => return Err(resolve_exit(session, &e)),
            Err(e) => {
                return Err(emit_error(
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

async fn ensure_detail(
    globals: &Globals,
    session: &Session,
    id: i64,
) -> Result<canvas_core::sync::RefreshOutcome, RefreshFail> {
    use canvas_core::store::lookup_dataset;
    use canvas_core::sync::{CourseDetailDataset, refresh_course};
    let now = now_timestamp();
    let ds = CourseDetailDataset::new(id, ttl_courses());
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    let grades_stale = session
        .open
        .store
        .call(move |conns| {
            let row = load_course_by_id(conns, id)?;
            Ok(
                super::course_load::grade_freshness(conns, row.as_slice(), now, false, None)?
                    .iter()
                    .any(|f| f.stale),
            )
        })
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) =
        super::course_load::cached_outcome(lookup, globals.fresh || grades_stale, globals.offline)?
    {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_course(
        client,
        &session.open.store,
        id,
        ttl_courses(),
        now,
        globals.fresh || grades_stale,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
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
    let grades = row.grades();
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

pub(crate) fn refresh_fail(session: &Session, err: RefreshFail) -> Handled {
    match err {
        RefreshFail::OfflineMiss | RefreshFail::Sync(SyncError::OfflineMiss) => emit_error(
            "offline",
            "offline and no complete courses cache coverage",
            7,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        RefreshFail::NeedAuth => emit_error(
            "auth",
            "no token; set CANVAS_TOKEN or run auth login",
            3,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        RefreshFail::Sync(e) => super::emit::sync_error(session, &e),
        RefreshFail::Db(e) => emit_error(
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
    )?;
    for teacher in &c.teachers {
        writeln!(io::stdout(), "teacher: {}", teacher.name)?;
    }
    if let Some(syllabus) = &c.syllabus_markdown {
        writeln!(io::stdout(), "\n{syllabus}")?;
    }
    Ok(())
}

fn resolve_exit(session: &Session, err: &ResolveError) -> Handled {
    super::emit::resolve_error(session, err)
}

async fn resolver_freshness(
    session: &Session,
    input: &str,
    id: i64,
    offline: bool,
) -> Result<Vec<Freshness>, DbError> {
    if input.parse::<i64>().is_ok() || reqwest::Url::parse(input).is_ok() {
        return Ok(Vec::new());
    }
    let input = input.to_owned();
    session.open.store.call(move |conns| {
        use canvas_core::{store::{lookup_dataset, LookupResult}, sync::{CoursesDataset,CoursesScope}};
        let alias: bool = conns.state.query_row("SELECT EXISTS(SELECT 1 FROM alias WHERE name=?1 AND target_kind='course')", [input], |r| r.get(0))?;
        if alias { return Ok(Vec::new()) }
        let active: bool = conns.cache.query_row("SELECT EXISTS(SELECT 1 FROM membership WHERE dataset='courses' AND scope='active' AND entity_id=?1)", [id.to_string()], |r| r.get(0))?;
        let mut rows=Vec::new();
        for scope in [CoursesScope::Active, CoursesScope::All] {
            if active && scope == CoursesScope::All { break; }
            let ds=CoursesDataset::new(scope,ttl_courses());
            let lookup=lookup_dataset(conns,&ds,now_timestamp(),None)?;
            if let LookupResult::Hit(ref row) | LookupResult::Stale(ref row) = lookup {
                rows.push(Freshness { dataset: row.dataset.clone(), scope: row.scope.clone(), source: crate::output::FreshnessSource::Cache, fetched_at:Some(row.fetched_at.to_string()), complete:row.complete, count:super::course_load::u64_count(row.count), stale:offline || !matches!(lookup,LookupResult::Hit(_)) });
            }
        }
        Ok(rows)
    }).await
}

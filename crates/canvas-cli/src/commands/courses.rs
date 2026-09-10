//! `canvas courses` (class C).

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use canvas_core::sync::{CoursesScope, SyncError};
use comfy_table::Row;

use super::Globals;
use super::course_load::{
    CourseRow, RefreshFail, ensure_courses, load_courses_for_scope, outcome_freshness,
};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    CoursesResult, SCHEMA_COURSES, apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_courses};

/// Run `canvas courses`.
pub async fn run(globals: &Globals, all: bool, term: Option<String>, favorites: bool) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let scope = if all {
        CoursesScope::All
    } else {
        CoursesScope::Active
    };
    let outcome = match ensure_courses(
        &session,
        scope,
        ttl_courses(),
        now_timestamp(),
        globals.fresh,
        globals.offline,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return refresh_exit(globals, &session, e),
    };

    let rows = match load_filtered(&session, scope.as_str(), term, favorites).await {
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

    let rows_for_freshness = rows.clone();
    let offline = globals.offline;
    let mut grades_freshness = match session
        .open
        .store
        .call(move |conns| {
            super::course_load::grade_freshness(
                conns,
                &rows_for_freshness,
                now_timestamp(),
                offline,
                None,
            )
        })
        .await
    {
        Ok(rows) => rows,
        Err(e) => return refresh_exit(globals, &session, RefreshFail::Db(e)),
    };
    if outcome.freshness.source == canvas_core::sync::FreshnessSource::Network {
        for row in &mut grades_freshness {
            row.source = crate::output::FreshnessSource::Network;
        }
    }
    let result = CoursesResult {
        courses: rows
            .iter()
            .map(|row| {
                let mut c = row.to_course_json();
                if c.html_url.is_empty() {
                    c.html_url = format!("{}/courses/{}", session.identity.origin, row.id);
                }
                c
            })
            .collect(),
    };
    let mut envelope = base_envelope(SCHEMA_COURSES, &session, result);
    envelope.freshness.push(outcome_freshness(&outcome));
    envelope.freshness.extend(grades_freshness);
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale courses cache".into());
    }

    let _use_color =
        crate::output::resolve_color(globals.json, globals.color, io::stdout().is_terminal());

    emit(globals.json, &envelope, || print_table(&envelope.result))
}

async fn load_filtered(
    session: &Session,
    scope_key: &str,
    term: Option<String>,
    favorites: bool,
) -> Result<Vec<CourseRow>, canvas_core::store::DbError> {
    let scope_key = scope_key.to_owned();
    session
        .open
        .store
        .call(move |conns| {
            let mut rows = load_courses_for_scope(conns, &scope_key)?;
            if let Some(ref needle) = term {
                let needle = needle.to_lowercase();
                rows.retain(|r| {
                    r.term_name
                        .as_ref()
                        .is_some_and(|n| n.to_lowercase().contains(&needle))
                });
            }
            if favorites {
                rows.retain(|r| r.is_favorite);
            }
            rows.sort_by(|a, b| a.code.cmp(&b.code).then_with(|| a.id.cmp(&b.id)));
            Ok(rows)
        })
        .await
}

fn print_table(result: &CoursesResult) -> io::Result<()> {
    let mut table = new_table();
    table.set_header(Row::from(vec!["CODE", "NAME", "TERM", "GRADE"]));
    for c in &result.courses {
        let grade = c
            .grades
            .current_grade
            .clone()
            .or_else(|| c.grades.current_score.map(|s| format!("{s}")))
            .unwrap_or_else(|| "unavailable".into());
        table.add_row(Row::from(vec![
            c.code.clone(),
            c.name.clone(),
            c.term.name.clone().unwrap_or_default(),
            grade,
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn refresh_exit(globals: &Globals, session: &Session, err: RefreshFail) -> ExitCode {
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
        RefreshFail::Sync(e) => super::emit::sync_error(globals, session, &e),
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

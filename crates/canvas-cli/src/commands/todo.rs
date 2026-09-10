//! `canvas todo` (class C).

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{CommandClass, ResolveError, resolve_course};
use canvas_core::sync::{
    CoursesScope, PlannerWindow, SyncError, refresh_courses, refresh_missing, refresh_planner,
};
use canvas_core::todo::{
    TodoFilters, TodoItem, TodoWindow, assignment_pending, build_todo, course_code,
    load_missing_rows, load_planner_rows, today_utc,
};
use serde_json::{Value, json};

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit, emit_error, session_error};
use crate::output::{SCHEMA_TODO, now_timestamp};
use crate::session::{Session, ttl_courses, ttl_missing, ttl_planner};

/// Run `canvas todo`.
#[allow(clippy::too_many_lines)]
pub async fn run(
    globals: &Globals,
    days: Option<u32>,
    all: bool,
    missing: bool,
    course: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };

    let now = now_timestamp();
    let today = today_utc(now);
    let days = days.unwrap_or(14).max(1);
    let window = PlannerWindow::todo_default(today, days);
    let todo_window = TodoWindow {
        start: window.start,
        end: window.end,
        days,
    };

    let client = session.client.as_ref();
    if client.is_none() && !globals.offline {
        // Class C without token still tries cache; treat as offline.
    }

    let mut freshness = Vec::new();
    let warnings = Vec::new();

    if let Some(client) = client {
        match refresh_courses(
            client,
            &session.open.store,
            CoursesScope::Active,
            ttl_courses(),
            now,
            globals.fresh,
            globals.offline,
        )
        .await
        {
            Ok(o) => freshness.push(o),
            Err(e) => return sync_exit(globals, &session, e),
        }
        match refresh_planner(
            client,
            &session.open.store,
            window.clone(),
            ttl_planner(),
            now,
            globals.fresh,
            globals.offline,
        )
        .await
        {
            Ok(o) => freshness.push(o),
            Err(e) => return sync_exit(globals, &session, e),
        }
        match refresh_missing(
            client,
            &session.open.store,
            ttl_missing(),
            now,
            globals.fresh,
            globals.offline,
        )
        .await
        {
            Ok(o) => freshness.push(o),
            Err(e) => return sync_exit(globals, &session, e),
        }
    } else if globals.offline || client.is_none() {
        // Offline / no client: serve cache only via empty refresh paths.
        for (dataset, scope) in [
            ("courses", "active"),
            ("planner", window.scope_key().as_str()),
            ("missing", "all"),
        ] {
            let _ = (dataset, scope);
        }
    }

    let course_id = if let Some(ref course) = course {
        let origin = session.identity.origin.clone();
        let course = course.clone();
        match call_resolve(&session, move |conns| {
            resolve_course(conns, &course, &origin, CommandClass::C)
        })
        .await
        {
            Ok(Ok(c)) => Some(c.id),
            Ok(Err(ResolveError::IncompleteDataset { .. })) => {
                return emit_error(
                    globals.json,
                    "offline",
                    "course list is incomplete; refresh online or use a numeric id",
                    7,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
            Ok(Err(e)) => {
                return emit_error(
                    globals.json,
                    "usage",
                    &e.to_string(),
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            }
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
        }
    } else {
        None
    };

    if all && globals.offline {
        return emit_error(
            globals.json,
            "offline",
            "todo --all requires complete assignments per active course",
            7,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }

    let scope = window.scope_key();
    let built = match session
        .open
        .store
        .call({
            let filters = TodoFilters {
                all,
                missing_only: missing,
                course_id,
                bucket: None,
                search: None,
            };
            let todo_window = todo_window.clone();
            move |conns| {
                let planner = load_planner_rows(conns, &scope)?;
                let missing_rows = load_missing_rows(conns)?;
                let mut codes = BTreeMap::new();
                let mut pending = BTreeMap::new();
                for row in &planner {
                    if let Some(cid) = row.course_id {
                        if let Ok(Some(code)) = course_code(conns, cid) {
                            codes.insert(cid, code);
                        }
                    }
                    if let Some(aid) = row.plannable_id {
                        if let Ok(p) = assignment_pending(conns, aid) {
                            pending.insert(aid, p);
                        }
                    }
                }
                for row in &missing_rows {
                    if let Some(cid) = row.course_id {
                        if let Ok(Some(code)) = course_code(conns, cid) {
                            codes.insert(cid, code);
                        }
                    }
                    if let Ok(p) = assignment_pending(conns, row.id) {
                        pending.insert(row.id, p);
                    }
                }
                Ok::<_, canvas_core::store::DbError>(build_todo(
                    &planner,
                    &missing_rows,
                    &codes,
                    &pending,
                    now,
                    today,
                    &todo_window,
                    &filters,
                    canvas_core::sync::default_ttl_assignments(),
                    &BTreeMap::new(),
                ))
            }
        })
        .await
    {
        Ok(v) => v,
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

    let (items, counts) = built;
    let result = json!({
        "window": {
            "start": todo_window.start.to_string(),
            "end": todo_window.end.to_string(),
            "days": todo_window.days,
        },
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
        "counts": {
            "missing": counts.missing,
            "due_today": counts.due_today,
            "due_week": counts.due_week,
            "hidden": counts.hidden,
        }
    });

    let mut envelope = base_envelope(SCHEMA_TODO, &session, result);
    envelope.freshness = freshness
        .iter()
        .map(|o| crate::output::Freshness {
            dataset: o.freshness.dataset.clone(),
            scope: o.freshness.scope.clone(),
            source: match o.freshness.source {
                canvas_core::sync::FreshnessSource::Cache => crate::output::FreshnessSource::Cache,
                canvas_core::sync::FreshnessSource::Network => {
                    crate::output::FreshnessSource::Network
                }
            },
            fetched_at: Some(o.freshness.fetched_at.to_string()),
            complete: o.freshness.complete,
            count: Some(u64_count(o.freshness.count)),
            stale: o.freshness.stale,
        })
        .collect();
    envelope.requests = session.requests();
    envelope.warnings = warnings;

    emit(globals.json, &envelope, || print_human(&items, &counts))
}

fn item_json(item: &TodoItem) -> Value {
    json!({
        "key": item.key,
        "kind": item.kind.as_str(),
        "raw_type": item.raw_type,
        "id": item.id.to_string(),
        "assignment_id": item.assignment_id.map(|v| v.to_string()),
        "parent_assignment_id": item.parent_assignment_id.map(|v| v.to_string()),
        "course_id": item.course_id.map(|v| v.to_string()),
        "course_code": item.course_code,
        "title": item.title,
        "due_at": item.due_at.map(|t| t.to_string()),
        "due_at_local": null,
        "scheduled_at": item.scheduled_at.map(|t| t.to_string()),
        "scheduled_at_local": null,
        "points_possible": item.points_possible,
        "status": {
            "submitted": item.status.submitted,
            "graded": item.status.graded,
            "score": item.status.score,
            "late": item.status.late,
            "missing": item.status.missing,
            "excused": item.status.excused,
            "locked": item.status.locked,
            "pending": item.status.pending,
        },
        "availability": {
            "locked": item.status.locked,
            "lock_explanation": item.availability.lock_explanation,
            "submittable": item.availability.submittable,
            "external": item.availability.external,
            "unlock_at": item.availability.unlock_at.map(|t| t.to_string()),
            "unlock_at_local": null,
            "lock_at": item.availability.lock_at.map(|t| t.to_string()),
            "lock_at_local": null,
        },
        "marked_complete": item.marked_complete,
        "dismissed": item.dismissed,
        "html_url": item.html_url,
    })
}

fn print_human(items: &[TodoItem], counts: &canvas_core::todo::TodoCounts) -> io::Result<()> {
    writeln!(
        io::stdout(),
        "missing={} due_today={} due_week={} hidden={}",
        counts.missing,
        counts.due_today,
        counts.due_week,
        counts.hidden
    )?;
    for item in items {
        let status = if item.status.pending {
            "pending"
        } else if item.status.missing {
            "missing"
        } else if item.status.submitted.unwrap_or(false) {
            "submitted"
        } else {
            "open"
        };
        writeln!(
            io::stdout(),
            "[{status}] {} {}",
            item.course_code.as_deref().unwrap_or("-"),
            item.title
        )?;
    }
    Ok(())
}

fn u64_count(n: i64) -> u64 {
    u64::try_from(n.max(0)).unwrap_or(0)
}

fn sync_exit(globals: &Globals, session: &Session, err: SyncError) -> ExitCode {
    match err {
        SyncError::OfflineMiss => emit_error(
            globals.json,
            "offline",
            "required dataset is not cached",
            7,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        other => emit_error(
            globals.json,
            "network",
            &other.to_string(),
            4,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

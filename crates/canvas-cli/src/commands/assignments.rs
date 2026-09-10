//! `canvas assignments` (class C).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{CommandClass, ResolveError, resolve_course};
use canvas_core::sync::{SyncError, refresh_assignments};
use canvas_core::todo::{
    AssignmentBucket, TodoAvailability, TodoFilters, TodoItem, TodoKind, TodoStatus, in_bucket,
};
use jiff::Timestamp;
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit, emit_error, session_error};
use crate::cli::AssignmentBucket as CliBucket;
use crate::output::{SCHEMA_ASSIGNMENTS, now_timestamp};
use crate::session::ttl_assignments;

/// Run `canvas assignments`.
pub async fn run(
    globals: &Globals,
    course: String,
    bucket: Option<CliBucket>,
    search: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let now = now_timestamp();
    let origin = session.identity.origin.clone();
    let resolved = match call_resolve(&session, {
        let course = course.clone();
        move |conns| resolve_course(conns, &course, &origin, CommandClass::C)
    })
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(ResolveError::IncompleteDataset { .. })) if globals.offline => {
            return emit_error(
                globals.json,
                "offline",
                "course list is incomplete",
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
    };

    if let Some(client) = session.client.as_ref() {
        if let Err(e) = refresh_assignments(
            client,
            &session.open.store,
            resolved.id,
            ttl_assignments(),
            now,
            globals.fresh,
            globals.offline,
        )
        .await
        {
            return match e {
                SyncError::OfflineMiss => emit_error(
                    globals.json,
                    "offline",
                    "assignments dataset is not cached",
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
            };
        }
    } else if globals.offline {
        // cache only
    }

    let bucket = bucket.map_or(AssignmentBucket::Open, map_bucket);
    let course_id = resolved.id;
    let items = match session
        .open
        .store
        .call(move |conns| {
            load_course_assignments(conns, course_id, now, bucket, search.as_deref())
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

    let result = json!({
        "course_id": course_id.to_string(),
        "bucket": bucket.as_str(),
        "assignments": items.iter().map(|i| json!({
            "id": i.id.to_string(),
            "name": i.title,
            "due_at": i.due_at.map(|t| t.to_string()),
            "points_possible": i.points_possible,
            "status": {
                "submitted": i.status.submitted,
                "graded": i.status.graded,
                "missing": i.status.missing,
                "pending": i.status.pending,
            }
        })).collect::<Vec<_>>(),
    });
    let envelope = base_envelope(SCHEMA_ASSIGNMENTS, &session, result);
    emit(globals.json, &envelope, || {
        for item in &items {
            writeln!(io::stdout(), "{}  {}", item.id, item.title)?;
        }
        Ok(())
    })
}

fn map_bucket(b: CliBucket) -> AssignmentBucket {
    match b {
        CliBucket::Open => AssignmentBucket::Open,
        CliBucket::Upcoming => AssignmentBucket::Upcoming,
        CliBucket::Overdue => AssignmentBucket::Overdue,
        CliBucket::Past => AssignmentBucket::Past,
        CliBucket::Undated => AssignmentBucket::Undated,
        CliBucket::Unsubmitted => AssignmentBucket::Unsubmitted,
        CliBucket::Ungraded => AssignmentBucket::Ungraded,
        CliBucket::Future => AssignmentBucket::Future,
        CliBucket::All => AssignmentBucket::All,
    }
}

fn load_course_assignments(
    conns: &canvas_core::store::StoreConns,
    course_id: i64,
    now: Timestamp,
    bucket: AssignmentBucket,
    search: Option<&str>,
) -> Result<Vec<TodoItem>, canvas_core::store::DbError> {
    let scope = format!("course:{course_id}");
    let mut stmt = conns.cache.prepare(
        "SELECT a.id, a.name, a.due_at, a.points_possible, a.submitted, a.graded, a.missing, a.unlock_at
         FROM membership m
         INNER JOIN assignments a ON a.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'assignments' AND m.scope = ?1 AND m.entity_kind = 'assignment'
         ORDER BY m.position ASC",
    )?;
    let rows = stmt
        .query_map([&scope], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let needle = search.map(str::to_lowercase);
    let filters = TodoFilters {
        bucket: Some(bucket),
        search: needle.clone(),
        ..TodoFilters::default()
    };
    let _ = filters;
    let mut out = Vec::new();
    for (id, name, due_at, points, submitted, graded, missing, unlock_at) in rows {
        let title = name.unwrap_or_default();
        if let Some(ref n) = needle {
            if !title.to_lowercase().contains(n) {
                continue;
            }
        }
        let item = TodoItem {
            key: format!("assignment:{id}"),
            kind: TodoKind::Assignment,
            raw_type: "assignment".into(),
            id,
            assignment_id: Some(id),
            parent_assignment_id: None,
            course_id: Some(course_id),
            course_code: None,
            title,
            due_at: due_at.and_then(|s| s.parse().ok()),
            scheduled_at: None,
            points_possible: points,
            status: TodoStatus {
                submitted: submitted.map(|v| v != 0),
                graded: graded.map(|v| v != 0),
                missing: missing.is_some_and(|v| v != 0),
                pending: canvas_core::store::pending_for_assignment(&conns.state, id)?,
                ..TodoStatus::default()
            },
            availability: TodoAvailability {
                unlock_at: unlock_at.and_then(|s| s.parse().ok()),
                ..TodoAvailability::default()
            },
            marked_complete: false,
            dismissed: false,
            html_url: None,
        };
        if in_bucket(&item, bucket, now) {
            out.push(item);
        }
    }
    Ok(out)
}

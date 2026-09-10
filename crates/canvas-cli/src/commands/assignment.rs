//! `canvas assignment` (class C).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::markdown;
use canvas_core::resolve::{CommandClass, ResolveError, resolve_assignment, resolve_course};
use canvas_core::sync::{assignment_detail_path, refresh_assignments};
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit, emit_error, session_error};
use crate::output::{SCHEMA_ASSIGNMENT, now_timestamp};
use crate::session::ttl_assignments;

/// Run `canvas assignment`.
pub async fn run(globals: &Globals, target: String, assignment: Option<String>) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let now = now_timestamp();
    let origin = session.identity.origin.clone();

    let (course_id, assignment_input) = if let Some(a) = assignment {
        let course = target.clone();
        let resolved = match call_resolve(&session, {
            let origin = origin.clone();
            move |conns| resolve_course(conns, &course, &origin, CommandClass::C)
        })
        .await
        {
            Ok(Ok(c)) => c,
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
        (resolved.id, a)
    } else {
        match call_resolve(&session, {
            let target = target.clone();
            let origin = origin.clone();
            move |conns| {
                if !target.contains("://") {
                    return Err(ResolveError::AssignmentNotFound { candidates: vec![] });
                }
                let course = resolve_course(conns, &target, &origin, CommandClass::C)?;
                let a = resolve_assignment(conns, course.id, &target, &origin, CommandClass::C)?;
                Ok((course.id, a.id.to_string()))
            }
        })
        .await
        {
            Ok(Ok(pair)) => pair,
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
    };

    if let Some(client) = session.client.as_ref() {
        let _ = refresh_assignments(
            client,
            &session.open.store,
            course_id,
            ttl_assignments(),
            now,
            globals.fresh,
            globals.offline,
        )
        .await;
        if let Ok(id) = assignment_input.parse::<i64>() {
            let detail_path = assignment_detail_path(course_id, id);
            let _ = client
                .get::<canvas_api::models::Assignment>(&detail_path)
                .await;
        }
    }

    let origin2 = origin.clone();
    let resolved = match call_resolve(&session, {
        let input = assignment_input.clone();
        move |conns| resolve_assignment(conns, course_id, &input, &origin2, CommandClass::C)
    })
    .await
    {
        Ok(Ok(a)) => a,
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

    let row = match session
        .open
        .store
        .call(move |conns| {
            conns
                .cache
                .query_row(
                    "SELECT name, due_at, points_possible, description, html_url, can_submit,
                        submission_types, allowed_extensions
                 FROM assignments WHERE id = ?1",
                    [resolved.id],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<f64>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<String>>(4)?,
                            r.get::<_, Option<i64>>(5)?,
                            r.get::<_, Option<String>>(6)?,
                            r.get::<_, Option<String>>(7)?,
                        ))
                    },
                )
                .map_err(canvas_core::store::DbError::from)
        })
        .await
    {
        Ok(v) => v,
        Err(_) => (
            resolved.name.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
    };

    let (name, due_at, points, description, html_url, can_submit, submission_types, allowed_ext) =
        row;
    let description_markdown = match description.as_deref() {
        Some(html) => markdown::html_to_markdown(html).await.ok(),
        None => None,
    };
    let pending = session
        .open
        .store
        .call(move |conns| canvas_core::store::pending_for_assignment(&conns.state, resolved.id))
        .await
        .unwrap_or(false);

    let result = json!({
        "assignment": {
            "id": resolved.id.to_string(),
            "course_id": course_id.to_string(),
            "name": name.unwrap_or_else(|| resolved.name.clone().unwrap_or_default()),
            "due_at": due_at,
            "due_at_local": null,
            "points_possible": points,
            "submission_types": submission_types
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default(),
            "allowed_extensions": allowed_ext
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default(),
            "allowed_attempts": null,
            "group_assignment": false,
            "availability": {
                "locked": null,
                "lock_explanation": null,
                "submittable": can_submit.map(|v| v != 0),
                "external": false,
                "unlock_at": null,
                "unlock_at_local": null,
                "lock_at": null,
                "lock_at_local": null
            },
            "status": {
                "submitted": false,
                "graded": false,
                "score": null,
                "grade": null,
                "late": false,
                "missing": false,
                "excused": false,
                "workflow_state": null,
                "submitted_at": null,
                "submitted_at_local": null,
                "attempt": null,
                "posted_at": null,
                "pending": pending
            },
            "html_url": html_url,
            "description_markdown": description_markdown,
            "can_submit": can_submit.map(|v| v != 0),
            "extra_attempts": null,
            "rubric": [],
            "rubric_assessed": false,
            "rubric_assessment": [],
            "comments_count": 0,
            "external_tool_name": null
        }
    });

    let envelope = base_envelope(SCHEMA_ASSIGNMENT, &session, result.clone());
    emit(globals.json, &envelope, || {
        let name = result["assignment"]["name"].as_str().unwrap_or("");
        writeln!(io::stdout(), "{name}")?;
        if let Some(md) = result["assignment"]["description_markdown"].as_str() {
            writeln!(io::stdout(), "\n{md}")?;
        }
        Ok(())
    })
}

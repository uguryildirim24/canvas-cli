//! `canvas open` (class B).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{CommandClass, ResolveError, resolve_assignment, resolve_course};
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit, emit_error, session_error};
use crate::cli::OpenCommand;
use crate::output::SCHEMA_OPEN;

fn resolve_fail(
    globals: &Globals,
    session: &crate::session::Session,
    err: ResolveError,
) -> ExitCode {
    let (code, exit) = ("resolution", 6);
    let message = err.to_string();
    emit_error(
        globals.json,
        code,
        &message,
        exit,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn db_fail(globals: &Globals, session: &crate::session::Session, err: impl ToString) -> ExitCode {
    emit_error(
        globals.json,
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// Run `canvas open`.
pub async fn run(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let origin = session.identity.origin.clone();

    let (kind, id, url) = match command {
        Some(OpenCommand::Assignment { course, assignment }) => {
            let resolved = match call_resolve(&session, {
                let course = course.clone();
                let origin = origin.clone();
                move |conns| resolve_course(conns, &course, &origin, CommandClass::B)
            })
            .await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return resolve_fail(globals, &session, e),
                Err(e) => return db_fail(globals, &session, e),
            };
            let a = match call_resolve(&session, {
                let assignment = assignment.clone();
                let origin = origin.clone();
                let course_id = resolved.id;
                move |conns| {
                    resolve_assignment(conns, course_id, &assignment, &origin, CommandClass::B)
                }
            })
            .await
            {
                Ok(Ok(a)) => a,
                Ok(Err(e)) => return resolve_fail(globals, &session, e),
                Err(e) => return db_fail(globals, &session, e),
            };
            (
                "assignment",
                a.id.to_string(),
                format!(
                    "{}/courses/{}/assignments/{}",
                    origin.trim_end_matches('/'),
                    resolved.id,
                    a.id
                ),
            )
        }
        Some(OpenCommand::File { id }) => (
            "file",
            id.clone(),
            format!("{}/files/{id}", origin.trim_end_matches('/')),
        ),
        Some(OpenCommand::Announcement { course, id }) => {
            let resolved = match call_resolve(&session, {
                let course = course.clone();
                let origin = origin.clone();
                move |conns| resolve_course(conns, &course, &origin, CommandClass::B)
            })
            .await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return resolve_fail(globals, &session, e),
                Err(e) => return db_fail(globals, &session, e),
            };
            (
                "announcement",
                id.clone(),
                format!(
                    "{}/courses/{}/discussion_topics/{id}",
                    origin.trim_end_matches('/'),
                    resolved.id
                ),
            )
        }
        None => {
            let Some(target) = target else {
                return emit_error(
                    globals.json,
                    "usage",
                    "open requires a target",
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                );
            };
            if target.contains("://") {
                match canvas_core::resolve::canvas_url(&target, &origin) {
                    Ok(Some(_)) => {}
                    _ => return resolve_fail(globals, &session, ResolveError::OriginMismatch),
                }
                ("url", target.clone(), target)
            } else {
                let resolved = match call_resolve(&session, {
                    let target = target.clone();
                    let origin = origin.clone();
                    move |conns| resolve_course(conns, &target, &origin, CommandClass::B)
                })
                .await
                {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => return resolve_fail(globals, &session, e),
                    Err(e) => return db_fail(globals, &session, e),
                };
                (
                    "course",
                    resolved.id.to_string(),
                    format!("{}/courses/{}", origin.trim_end_matches('/'), resolved.id),
                )
            }
        }
    };

    let launched = open::that(&url).is_ok();
    let result = json!({
        "target_kind": kind,
        "id": id,
        "url": url,
        "launched": launched,
    });
    let envelope = base_envelope(SCHEMA_OPEN, &session, result);
    emit(globals.json, &envelope, || {
        writeln!(io::stdout(), "{kind} {url} launched={launched}")
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn browser_urls_require_exact_origin() {
        let origin = "https://canvas.example.test";
        for bad in [
            "https://canvas.example.test.evil.test/courses/1",
            "https://canvas.example.test@evil.test/courses/1",
            "https://canvas.example.test:444/courses/1",
            "http://canvas.example.test/courses/1",
            "https://user@canvas.example.test/courses/1",
        ] {
            assert!(
                canvas_core::resolve::canvas_url(bad, origin).is_err(),
                "{bad}"
            );
        }
        assert!(
            canvas_core::resolve::canvas_url("HTTPS://CANVAS.EXAMPLE.TEST:443/courses/1", origin)
                .unwrap()
                .is_some()
        );
    }
}

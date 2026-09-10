//! `canvas open` (class B).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::resolve::{CommandClass, ResolveError, resolve_assignment, resolve_course};
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit_error, session_error};
use super::handled::Handled;
use crate::cli::OpenCommand;
use crate::output::SCHEMA_OPEN;

fn resolve_fail(session: &crate::session::Session, err: ResolveError) -> Handled {
    let (code, exit) = ("resolution", 6);
    let message = err.to_string();
    emit_error(
        code,
        &message,
        exit,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

fn db_fail(session: &crate::session::Session, err: impl ToString) -> Handled {
    emit_error(
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// Run `canvas open` for the CLI: one envelope, one exit code.
pub async fn run(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
) -> ExitCode {
    handle(globals, command, target).await.emit(globals.json)
}

/// Run `canvas open`.
pub async fn handle(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
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
                Ok(Err(e)) => return resolve_fail(&session, e),
                Err(e) => return db_fail(&session, e),
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
                Ok(Err(e)) => return resolve_fail(&session, e),
                Err(e) => return db_fail(&session, e),
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
        Some(OpenCommand::File { id }) => {
            let Ok(id) = id.parse::<i64>() else {
                return resolve_fail(&session, ResolveError::NeedIdOrUrl);
            };
            (
                "file",
                id.to_string(),
                format!("{}/files/{id}", origin.trim_end_matches('/')),
            )
        }
        Some(OpenCommand::Announcement { course, id }) => {
            let Ok(id) = id.parse::<i64>() else {
                return resolve_fail(&session, ResolveError::NeedIdOrUrl);
            };
            let resolved = match call_resolve(&session, {
                let course = course.clone();
                let origin = origin.clone();
                move |conns| resolve_course(conns, &course, &origin, CommandClass::B)
            })
            .await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return resolve_fail(&session, e),
                Err(e) => return db_fail(&session, e),
            };
            (
                "announcement",
                id.to_string(),
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
                    _ => return resolve_fail(&session, ResolveError::OriginMismatch),
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
                    Ok(Err(e)) => return resolve_fail(&session, e),
                    Err(e) => return db_fail(&session, e),
                };
                (
                    "course",
                    resolved.id.to_string(),
                    format!("{}/courses/{}", origin.trim_end_matches('/'), resolved.id),
                )
            }
        }
    };

    let result = launch_result(kind, &id, &url, || open::that(&url).is_ok());
    let envelope = base_envelope(SCHEMA_OPEN, &session, result);
    Handled::new(envelope, move |envelope| {
        writeln!(io::stdout(), "{}", human_result(&envelope.result))
    })
}

fn launch_result(
    kind: &str,
    id: &str,
    url: &str,
    launch: impl FnOnce() -> bool,
) -> serde_json::Value {
    json!({
        "target_kind": kind,
        "id": id,
        "url": url,
        "launched": launch(),
    })
}

fn human_result(result: &serde_json::Value) -> String {
    format!(
        "{} {} launched={}",
        result["target_kind"].as_str().unwrap_or(""),
        result["url"].as_str().unwrap_or(""),
        result["launched"]
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn open_json_and_human_snapshots_report_launch_result() {
        let result = super::launch_result(
            "assignment",
            "2",
            "https://canvas.test/courses/1/assignments/2",
            || true,
        );
        insta::assert_json_snapshot!("open_json", result);
        insta::assert_snapshot!("open_human", super::human_result(&result));
        let failed = super::launch_result("file", "3", "https://canvas.test/files/3", || false);
        assert_eq!(failed["launched"], false);
    }

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

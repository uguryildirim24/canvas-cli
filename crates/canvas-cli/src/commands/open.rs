//! `canvas open` (class B).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{Body, Op, Reason};
use canvas_core::resolve::{CommandClass, ResolveError, resolve_assignment, resolve_course};
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, call_resolve, emit_error, session_error};
use super::handled::Handled;
use crate::bridge::client;
use crate::cli::OpenCommand;
use crate::output::{
    Envelope, FollowJson, FollowResult, Outcome, SCHEMA_FOLLOW, SCHEMA_OPEN, generated_at_now,
};

/// What `--follow` tells the person before the tab moves.
///
/// The API previews of this project promise not to change anything. Handing a
/// URL to the browser promises no such thing: Canvas' own page controllers
/// run, and the discussion controller marks a topic read when it renders it
/// (REPORT §3.3 step 7, source S13). The two are different acts and the
/// output says so.
pub const NAVIGATION_SIDE_EFFECTS: &str = "the browser loads the page, and Canvas' own page controllers run: a discussion page \
     marks itself read";

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

/// Whether the resolved URL is also handed to the browser.
///
/// `canvas open` launches it. The `open.url` tool resolves only: an agent
/// surface must not start a program on the user's machine (REPORT §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Yes,
    No,
}

/// Run `canvas open` for the CLI: one envelope, one exit code.
pub async fn run(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
) -> ExitCode {
    handle(globals, command, target, Launch::Yes)
        .await
        .emit(globals.json)
}

/// Run `canvas open`.
pub async fn handle(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
    browser: Launch,
) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let resolved = match resolve(&session, command, target).await {
        Ok(resolved) => resolved,
        Err(handled) => return handled,
    };
    let Resolved { kind, id, url } = resolved;
    let result = launch_result(kind, &id, &url, || browser == Launch::Yes && launch(&url));
    let envelope = base_envelope(SCHEMA_OPEN, &session, result);
    Handled::new(envelope, move |envelope| {
        writeln!(io::stdout(), "{}", human_result(&envelope.result))
    })
}

/// What a target resolved to, with no browser involved.
pub struct Resolved {
    /// `course`, `assignment`, `file`, `announcement`, or `url`.
    pub kind: &'static str,
    /// The resolved id, or the URL itself when the target was one.
    pub id: String,
    /// The canonical Canvas URL.
    pub url: String,
}

/// Resolve a target to its canonical Canvas URL, and nothing else.
///
/// It fetches nothing and it opens nothing. A target outside this identity's
/// Canvas is a resolution failure at exit 6, which is the same answer
/// `canvas open` gives, so `--follow` cannot reach an origin `open` refuses.
async fn resolve(
    session: &crate::session::Session,
    command: Option<OpenCommand>,
    target: Option<String>,
) -> Result<Resolved, Handled> {
    let origin = session.identity.origin.clone();

    let (kind, id, url) = match command {
        Some(OpenCommand::Assignment { course, assignment }) => {
            let resolved = match call_resolve(session, {
                let course = course.clone();
                let origin = origin.clone();
                move |conns| resolve_course(conns, &course, &origin, CommandClass::B)
            })
            .await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return Err(resolve_fail(session, e)),
                Err(e) => return Err(db_fail(session, e)),
            };
            let a = match call_resolve(session, {
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
                Ok(Err(e)) => return Err(resolve_fail(session, e)),
                Err(e) => return Err(db_fail(session, e)),
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
                return Err(resolve_fail(session, ResolveError::NeedIdOrUrl));
            };
            (
                "file",
                id.to_string(),
                format!("{}/files/{id}", origin.trim_end_matches('/')),
            )
        }
        Some(OpenCommand::Announcement { course, id }) => {
            let Ok(id) = id.parse::<i64>() else {
                return Err(resolve_fail(session, ResolveError::NeedIdOrUrl));
            };
            let resolved = match call_resolve(session, {
                let course = course.clone();
                let origin = origin.clone();
                move |conns| resolve_course(conns, &course, &origin, CommandClass::B)
            })
            .await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return Err(resolve_fail(session, e)),
                Err(e) => return Err(db_fail(session, e)),
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
                return Err(emit_error(
                    "usage",
                    "open requires a target",
                    2,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                ));
            };
            if target.contains("://") {
                match canvas_core::resolve::canvas_url(&target, &origin) {
                    Ok(Some(_)) => {}
                    _ => return Err(resolve_fail(session, ResolveError::OriginMismatch)),
                }
                ("url", target.clone(), target)
            } else {
                let resolved = match call_resolve(session, {
                    let target = target.clone();
                    let origin = origin.clone();
                    move |conns| resolve_course(conns, &target, &origin, CommandClass::B)
                })
                .await
                {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => return Err(resolve_fail(session, e)),
                    Err(e) => return Err(db_fail(session, e)),
                };
                (
                    "course",
                    resolved.id.to_string(),
                    format!("{}/courses/{}", origin.trim_end_matches('/'), resolved.id),
                )
            }
        }
    };
    Ok(Resolved { kind, id, url })
}

/// `canvas open <target> --follow` and `context.follow`.
///
/// The target is resolved through the ordinary `open` resolver — the same
/// code, the same cross-origin refusal at exit 6, and no fetch — and only
/// then is the companion asked to move the tab. What comes back is the
/// **dispatch acknowledgement**: the companion took the navigation. Whether
/// the page loaded is a separate fact, and it arrives later, on the `here@1`
/// bundle's `browser.follow` (REPORT §3.2).
pub async fn follow(
    globals: &Globals,
    command: Option<OpenCommand>,
    target: Option<String>,
    attachment: Option<String>,
    consumer: Option<String>,
    generation: Option<u64>,
) -> Handled {
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    // Resolve first. A target this identity's Canvas does not own never
    // reaches the browser, and it fails the way `canvas open` fails.
    let resolved = match resolve(&session, command, target).await {
        Ok(resolved) => resolved,
        Err(handled) => return handled,
    };
    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);

    let generation = match generation {
        Some(generation) => generation,
        None => match current_generation(&endpoint, attachment.clone(), consumer.clone()) {
            Ok(generation) => generation,
            Err(reason) => return follow_refusal(&session, &resolved, consumer, reason),
        },
    };

    let (status, attachment_id) = match client::call(
        &endpoint,
        Op::Follow {
            attachment_id: attachment,
            consumer: consumer.clone(),
            generation,
            url: resolved.url.clone(),
        },
    ) {
        Ok(Body::Followed {
            follow,
            attachment_id,
        }) => (*follow, attachment_id),
        Ok(_) => return follow_refusal(&session, &resolved, consumer, Reason::Protocol),
        Err(reason) => return follow_refusal(&session, &resolved, consumer, reason),
    };

    let result = FollowResult {
        attachment: Some(attachment_id),
        consumer,
        target_kind: resolved.kind.to_owned(),
        id: resolved.id.clone(),
        url: resolved.url.clone(),
        follow: Some(FollowJson::from(&status)),
        side_effects: vec![NAVIGATION_SIDE_EFFECTS.to_owned()],
        reason: None,
    };
    let mut envelope = base_envelope(SCHEMA_FOLLOW, &session, result);
    envelope.warnings = Vec::new();
    Handled::new(envelope, |envelope| {
        let _ = writeln!(io::stderr(), "{NAVIGATION_SIDE_EFFECTS}");
        writeln!(io::stdout(), "{}", follow_human(&envelope.result))
    })
}

/// The navigation generation the broker currently holds.
fn current_generation(
    endpoint: &Endpoint,
    attachment: Option<String>,
    consumer: Option<String>,
) -> Result<u64, Reason> {
    match client::call(
        endpoint,
        Op::Here {
            attachment_id: attachment,
            consumer,
            include_text: false,
        },
    )? {
        Body::Context(context) => Ok(context.navigation_generation),
        _ => Err(Reason::Protocol),
    }
}

/// The tab did not move, and the target is still named so the person can
/// open it themselves.
fn follow_refusal(
    session: &crate::session::Session,
    resolved: &Resolved,
    consumer: Option<String>,
    reason: Reason,
) -> Handled {
    let message = super::here::message_for(reason);
    let result = FollowResult {
        attachment: None,
        consumer,
        target_kind: resolved.kind.to_owned(),
        id: resolved.id.clone(),
        url: resolved.url.clone(),
        follow: None,
        side_effects: Vec::new(),
        reason: Some(reason.as_str().to_owned()),
    };
    let envelope: Envelope<FollowResult> = Envelope {
        schema: SCHEMA_FOLLOW.to_owned(),
        generated_at: generated_at_now(),
        profile: session.profile.clone(),
        identity: Some(session.identity_ref()),
        freshness: Vec::new(),
        requests: session.requests(),
        partial: Vec::new(),
        warnings: Vec::new(),
        outcome: Outcome::Refused,
        exit: 8,
        result,
    };
    Handled::new(envelope, move |envelope| {
        let _ = writeln!(io::stderr(), "{message}");
        writeln!(io::stdout(), "{}", follow_human(&envelope.result))
    })
}

fn follow_human(result: &FollowResult) -> String {
    let Some(follow) = result.follow.as_ref() else {
        return format!(
            "the tab did not move ({}); the page is {}",
            result.reason.as_deref().unwrap_or("unknown"),
            result.url
        );
    };
    format!(
        "dispatched {} in {} ms; load {}",
        follow.url, follow.dispatch_ms, follow.load
    )
}

/// Hand `url` to the desktop browser, unless a test build opted out.
///
/// `open` is otherwise the one v1 command an end-to-end run cannot exercise: it
/// spawns a real browser window. The opt-out carries the same
/// `debug_assertions` gate as `CANVAS_TEST_FORCE_FILE` and `CANVAS_NOW`, so a
/// release build always launches.
fn launch(url: &str) -> bool {
    if cfg!(debug_assertions) && std::env::var_os("CANVAS_TEST_NO_LAUNCH").is_some() {
        return false;
    }
    open::that(url).is_ok()
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

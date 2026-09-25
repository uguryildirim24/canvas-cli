//! Envelope emit helpers (JSON and human error paths).

use super::handled::Handled;
use crate::output::{
    Envelope, ErrorResult, IdentityRef, Outcome, SCHEMA_ERROR, error_envelope, generated_at_now,
};
use crate::session::{Session, SessionError};
use canvas_core::resolve::ResolveError;
use canvas_core::store::DbError;

/// Map a store `call` that returns nested resolve results.
pub async fn call_resolve<T, F>(session: &Session, f: F) -> Result<Result<T, ResolveError>, DbError>
where
    F: FnOnce(&canvas_core::store::StoreConns) -> Result<T, ResolveError> + Send + 'static,
    T: Send + 'static,
{
    session
        .open
        .store
        .call(move |conns| match f(conns) {
            Ok(v) => Ok(Ok(v)),
            Err(ResolveError::Db(e)) => Err(e),
            Err(e) => Ok(Err(e)),
        })
        .await
}

/// Build an `error@1` result for an aborted command (§7, §14).
pub fn emit_error(
    code: &str,
    message: &str,
    exit: u8,
    profile: Option<String>,
    identity: Option<IdentityRef>,
) -> Handled {
    Handled::error(code, message, exit, profile, identity)
}

/// Map [`SessionError`] to an exit.
pub fn session_error(err: SessionError, profile: Option<String>) -> Handled {
    match err {
        SessionError::Usage(message) => emit_error("usage", &message, 2, profile, None),
        SessionError::Auth(message) => emit_error("auth", &message, 3, profile, None),
        SessionError::Local(message) => emit_error("local", &message, 13, profile, None),
    }
}

/// Build a base envelope for a successful command result.
#[must_use]
pub fn base_envelope<T>(schema: &str, session: &Session, result: T) -> Envelope<T> {
    Envelope {
        schema: schema.to_owned(),
        generated_at: generated_at_now(),
        profile: session.profile.clone(),
        identity: Some(session.identity_ref()),
        freshness: Vec::new(),
        requests: session.requests(),
        partial: Vec::new(),
        warnings: Vec::new(),
        outcome: Outcome::Ok,
        exit: 0,
        result,
    }
}

/// Build an error envelope value without writing (tests / composition).
#[must_use]
#[allow(dead_code)]
pub fn error_result(code: &str, message: &str) -> ErrorResult {
    ErrorResult {
        code: code.to_owned(),
        message: message.to_owned(),
        http_status: None,
        server_errors: Vec::new(),
        details: serde_json::json!({}),
    }
}

/// Map schema constant for error documents.
#[must_use]
#[allow(dead_code)]
pub fn error_schema() -> &'static str {
    SCHEMA_ERROR
}

/// Require an online API client (rejects `--offline` and missing token).
pub fn require_client<'a>(
    globals: &super::Globals,
    session: &'a Session,
) -> Result<&'a canvas_api::Client, Handled> {
    if globals.offline {
        return Err(emit_error(
            "usage",
            "this command cannot run with --offline",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        ));
    }
    match &session.client {
        Some(client) => Ok(client),
        None => {
            if matches!(
                session.client_init_error(),
                Some(canvas_api::Error::Network)
            ) {
                Err(emit_error(
                    "network",
                    "network error",
                    4,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                ))
            } else {
                Err(emit_error(
                    "auth",
                    "no token; set CANVAS_TOKEN or run auth login",
                    3,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                ))
            }
        }
    }
}

/// Keep API variants, status, and invocation telemetry on every abort.
pub fn sync_error(session: &Session, err: &canvas_core::sync::SyncError) -> Handled {
    let (code, exit, status) = err.classification();
    let mut env = error_envelope(
        code,
        err.safe_message(),
        status,
        serde_json::json!({}),
        exit,
    );
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    let message = env.result.message.clone();
    Handled::error_envelope(env, message)
}

pub fn resolve_error(session: &Session, err: &canvas_core::resolve::ResolveError) -> Handled {
    use canvas_core::resolve::ResolveError;
    let candidates: Vec<_> = match err {
        ResolveError::NotFound { candidates } | ResolveError::Ambiguous { candidates } => {
            candidates
                .iter()
                .map(
                    |c| serde_json::json!({"id": c.id.to_string(), "code": c.code, "name": c.name}),
                )
                .collect()
        }
        ResolveError::AssignmentNotFound { candidates } | ResolveError::AssignmentAmbiguous { candidates } => candidates.iter().map(|c| serde_json::json!({"id": c.id.to_string(), "course_id": c.course_id.to_string(), "name":c.name})).collect(),
        ResolveError::QuizNotFound { candidates } | ResolveError::QuizAmbiguous { candidates } => candidates.iter().map(|c| serde_json::json!({"id": c.id.to_string(), "course_id": c.course_id.to_string(), "title": c.title})).collect(),
        _ => Vec::new(),
    };
    let mut env = error_envelope(
        "resolution",
        err.to_string(),
        None,
        serde_json::json!({"candidates": candidates}),
        6,
    );
    env.profile.clone_from(&session.profile);
    env.identity = Some(session.identity_ref());
    env.requests = session.requests();
    let mut message = err.to_string();
    for c in &candidates {
        use std::fmt::Write as _;
        let _ = write!(
            message,
            "\n{}  {}  {}",
            c["id"].as_str().unwrap_or(""),
            c["code"].as_str().unwrap_or(""),
            c["name"].as_str().unwrap_or("")
        );
    }
    Handled::error_envelope(env, message)
}

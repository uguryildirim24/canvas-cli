//! Envelope emit helpers (JSON and human error paths).

use std::io::{self, Write};
use std::process::ExitCode;

use serde::Serialize;

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

/// Write a success (or completed) envelope as JSON, or run the human renderer.
pub fn emit<T: Serialize>(
    json: bool,
    envelope: &Envelope<T>,
    mut human: impl FnMut() -> io::Result<()>,
) -> ExitCode {
    if json {
        if let Err(e) = envelope.write_json(io::stdout()) {
            let _ = writeln!(io::stderr(), "failed to write JSON: {e}");
            return ExitCode::from(1);
        }
        return envelope.exit_code();
    }
    for warning in &envelope.warnings {
        let _ = writeln!(io::stderr(), "warning: {warning}");
    }
    if let Err(e) = human() {
        let _ = writeln!(io::stderr(), "{e}");
        return ExitCode::from(1);
    }
    envelope.exit_code()
}

/// Emit an `error@1` envelope (JSON) or a human message on stderr.
pub fn emit_error(
    json: bool,
    code: &str,
    message: &str,
    exit: u8,
    profile: Option<String>,
    identity: Option<IdentityRef>,
) -> ExitCode {
    let mut env = error_envelope(code, message, None, serde_json::json!({}), exit);
    env.profile = if identity.is_some() { profile } else { None };
    env.identity = identity;
    if json {
        let _ = env.write_json(io::stdout());
    } else {
        let _ = writeln!(io::stderr(), "{message}");
    }
    ExitCode::from(exit)
}

/// Map [`SessionError`] to an exit.
pub fn session_error(json: bool, err: SessionError, profile: Option<String>) -> ExitCode {
    match err {
        SessionError::Usage(message) => emit_error(json, "usage", &message, 2, profile, None),
        SessionError::Auth(message) => emit_error(json, "auth", &message, 3, profile, None),
        SessionError::Local(message) => emit_error(json, "local", &message, 13, profile, None),
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

/// Keep API variants, status, and invocation telemetry on every abort.
pub fn sync_error(
    globals: &super::Globals,
    session: &Session,
    err: &canvas_core::sync::SyncError,
) -> ExitCode {
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
    emit(globals.json, &env, || {
        writeln!(io::stderr(), "{}", env.result.message)
    })
}

pub fn resolve_error(
    globals: &super::Globals,
    session: &Session,
    err: &canvas_core::resolve::ResolveError,
) -> ExitCode {
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
    emit(globals.json, &env, || {
        writeln!(io::stderr(), "{err}")?;
        for c in &candidates {
            writeln!(
                io::stderr(),
                "{}  {}  {}",
                c["id"].as_str().unwrap_or(""),
                c["code"].as_str().unwrap_or(""),
                c["name"].as_str().unwrap_or("")
            )?;
        }
        Ok(())
    })
}

//! Envelope emit helpers (JSON and human error paths).

use std::io::{self, Write};
use std::process::ExitCode;

use serde::Serialize;

use crate::output::{
    Envelope, ErrorResult, IdentityRef, Outcome, Requests, SCHEMA_ERROR, error_envelope,
    generated_at_now,
};
use crate::session::{Session, SessionError};

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
    env.profile = profile;
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
        requests: Requests::default(),
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

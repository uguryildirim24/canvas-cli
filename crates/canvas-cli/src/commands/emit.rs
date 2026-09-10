//! Envelope emit helpers (JSON and human error paths).

use std::io::{self, Write};
use std::process::ExitCode;

use serde::Serialize;

use crate::output::{
    Envelope, ErrorResult, IdentityRef, Outcome, SCHEMA_ERROR, error_envelope, generated_at_now,
};
use crate::session::{Session, SessionError, ValidateTokenError};

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

/// Map token validation failures.
pub fn validate_token_error(json: bool, err: ValidateTokenError, session: &Session) -> ExitCode {
    match err {
        ValidateTokenError::Auth => emit_error(
            json,
            "auth",
            "token rejected",
            3,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ValidateTokenError::Network => emit_error(
            json,
            "network",
            "network error",
            4,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ValidateTokenError::Api(canvas_api::Error::RateLimited) => emit_error(
            json,
            "rate_limited",
            "rate limited",
            5,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ValidateTokenError::Api(e) => emit_error(
            json,
            "network",
            &e.to_string(),
            4,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
        ValidateTokenError::Local(e) => emit_error(
            json,
            "local",
            &e.to_string(),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        ),
    }
}

/// Require an online API client (rejects `--offline` and missing token).
pub fn require_client<'a>(
    globals: &super::Globals,
    session: &'a Session,
) -> Result<&'a canvas_api::Client, ExitCode> {
    if globals.offline {
        return Err(emit_error(
            globals.json,
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
                    globals.json,
                    "network",
                    "network error",
                    4,
                    session.profile.clone(),
                    Some(session.identity_ref()),
                ))
            } else {
                Err(emit_error(
                    globals.json,
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

/// Parse a numeric Canvas id; non-numeric ids need M1-c resolution.
pub fn parse_numeric_id(raw: &str, label: &str) -> Result<i64, String> {
    raw.parse::<i64>()
        .map_err(|_| format!("{label} resolution needs M1-c; use a numeric id for now"))
}

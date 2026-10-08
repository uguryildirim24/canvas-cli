//! JSON envelope types per SPEC §7.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::ExitCode;

use crate::output::now::generated_at_now;
use crate::output::registry::SCHEMA_ERROR;

/// Machine-readable process outcome (§7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Partial,
    Recovery,
    Mismatch,
    Refused,
    Error,
}

/// Identity reference embedded in envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdentityRef {
    pub origin: String,
    pub user_id: String,
    pub key: String,
}

impl IdentityRef {
    /// Build from origin, numeric user id, and key.
    #[must_use]
    pub fn new(origin: impl Into<String>, user_id: i64, key: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            user_id: user_id.to_string(),
            key: key.into(),
        }
    }
}

/// Where a freshness row was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessSource {
    Cache,
    Network,
}

/// One dataset freshness entry (§7 / Appendix D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Freshness {
    pub dataset: String,
    pub scope: String,
    pub source: FreshnessSource,
    pub fetched_at: Option<String>,
    pub complete: bool,
    pub count: Option<u64>,
    pub stale: bool,
}

/// Request counters for the invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct Requests {
    pub api: u64,
    pub storage: u64,
    pub cost: Option<f64>,
}

/// A scope that returned a partial/denied result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PartialScope {
    pub scope: String,
    pub http_status: Option<u16>,
    pub message: String,
}

/// Fatal error payload for `canvas-cli/error@1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorResult {
    pub code: String,
    pub message: String,
    pub http_status: Option<u16>,
    pub server_errors: Vec<String>,
    pub details: serde_json::Value,
}

/// Single-document JSON envelope for one invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Envelope<T> {
    pub schema: String,
    pub generated_at: String,
    pub profile: Option<String>,
    pub identity: Option<IdentityRef>,
    pub freshness: Vec<Freshness>,
    pub requests: Requests,
    pub partial: Vec<PartialScope>,
    pub warnings: Vec<String>,
    pub outcome: Outcome,
    pub exit: u8,
    pub result: T,
}

impl Envelope<serde_json::Value> {
    /// Build a new envelope with an empty JSON object result.
    ///
    /// `generated_at` uses `CANVAS_NOW` when set (tests), else wall-clock UTC.
    #[must_use]
    pub fn new(
        schema: impl Into<String>,
        profile: Option<&str>,
        identity: Option<IdentityRef>,
    ) -> Self {
        Self {
            schema: schema.into(),
            generated_at: generated_at_now(),
            profile: profile.map(str::to_owned),
            identity,
            freshness: Vec::new(),
            requests: Requests::default(),
            partial: Vec::new(),
            warnings: Vec::new(),
            outcome: Outcome::Ok,
            exit: 0,
            result: serde_json::json!({}),
        }
    }

    /// Build a `canvas-cli/error@1` envelope with a JSON-object result.
    #[must_use]
    pub fn error(
        code: &str,
        message: &str,
        exit: u8,
        http_status: Option<u16>,
        profile: Option<&str>,
        identity: Option<IdentityRef>,
    ) -> Self {
        Self::new(SCHEMA_ERROR, profile, identity)
            .with_result(serde_json::json!({
                "code": code,
                "message": message,
                "http_status": http_status,
                "server_errors": [],
                "details": {},
            }))
            .with_outcome("error", exit)
    }
}

impl<T> Envelope<T> {
    /// Replace the result payload, preserving envelope metadata.
    #[must_use]
    pub fn with_result<U>(self, result: U) -> Envelope<U> {
        Envelope {
            schema: self.schema,
            generated_at: self.generated_at,
            profile: self.profile,
            identity: self.identity,
            freshness: self.freshness,
            requests: self.requests,
            partial: self.partial,
            warnings: self.warnings,
            outcome: self.outcome,
            exit: self.exit,
            result,
        }
    }

    /// Write one JSON document (plus trailing newline) to `w`.
    pub fn write_json(&self, mut w: impl Write) -> std::io::Result<()>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        w.write_all(&bytes)?;
        w.write_all(b"\n")?;
        Ok(())
    }

    /// Print the envelope as one pretty JSON document on stdout.
    pub fn print_json(&self)
    where
        T: Serialize,
    {
        match serde_json::to_string_pretty(self) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("failed to encode JSON envelope: {e}"),
        }
    }

    /// Record API request count.
    #[must_use]
    pub fn with_api_requests(mut self, api: u64) -> Self {
        self.requests.api = api;
        self
    }

    /// Set outcome from a `snake_case` label and exit code.
    #[must_use]
    pub fn with_outcome(mut self, outcome: &str, exit: u8) -> Self {
        self.outcome = match outcome {
            "ok" => Outcome::Ok,
            "partial" => Outcome::Partial,
            "recovery" => Outcome::Recovery,
            "mismatch" => Outcome::Mismatch,
            "refused" => Outcome::Refused,
            _ => Outcome::Error,
        };
        self.exit = exit;
        self
    }

    /// Process exit code carried by this envelope.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.exit)
    }
}

/// Print human text when not in `--json` mode.
pub fn print_human(lines: impl IntoIterator<Item = impl AsRef<str>>) {
    for line in lines {
        println!("{}", line.as_ref());
    }
}

/// Build an `error@1` envelope.
#[must_use]
pub fn error_envelope(
    code: impl Into<String>,
    message: impl Into<String>,
    http_status: Option<u16>,
    details: serde_json::Value,
    exit: u8,
) -> Envelope<ErrorResult> {
    Envelope {
        schema: SCHEMA_ERROR.to_string(),
        generated_at: generated_at_now(),
        profile: None,
        identity: None,
        freshness: Vec::new(),
        requests: Requests::default(),
        partial: Vec::new(),
        warnings: Vec::new(),
        outcome: Outcome::Error,
        exit,
        result: ErrorResult {
            code: code.into(),
            message: message.into(),
            http_status,
            server_errors: Vec::new(),
            details,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::now::with_canvas_now;

    #[test]
    fn envelope_new_honors_canvas_now() {
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let env = Envelope::new("canvas-cli/courses@1", Some("example"), None);
            assert_eq!(env.schema, "canvas-cli/courses@1");
            assert_eq!(env.generated_at, "2026-09-09T17:05:12Z");
            assert_eq!(env.profile.as_deref(), Some("example"));
            assert!(env.identity.is_none());
            assert_eq!(env.outcome, Outcome::Ok);
            assert_eq!(env.exit, 0);
            assert_eq!(env.requests.api, 0);
        });
    }

    #[test]
    fn with_result_and_write_json_round_trip() {
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let env = Envelope::new("canvas-cli/courses@1", None, None)
                .with_result(serde_json::json!({ "courses": [] }));
            let mut buf = Vec::new();
            env.write_json(&mut buf).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
            assert_eq!(v["schema"], "canvas-cli/courses@1");
            assert_eq!(v["outcome"], "ok");
            assert_eq!(v["result"]["courses"], serde_json::json!([]));
            assert_eq!(env.exit_code(), ExitCode::from(0));
        });
    }

    #[test]
    fn error_envelope_uses_error_schema() {
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let env = error_envelope("auth", "unauthorized", Some(401), serde_json::json!({}), 3);
            assert_eq!(env.schema, SCHEMA_ERROR);
            assert_eq!(env.outcome, Outcome::Error);
            assert_eq!(env.exit, 3);
            assert_eq!(env.result.code, "auth");
            assert_eq!(env.result.http_status, Some(401));
        });
    }
}

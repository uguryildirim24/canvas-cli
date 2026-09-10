//! JSON output envelope (§7 / Appendix D).

#![allow(dead_code)] // M1-b will extend the builder; keep the full surface.

use serde::Serialize;
use serde_json::{json, Value};

/// Identity reference embedded in the envelope.
#[derive(Debug, Clone, Serialize)]
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

/// Versioned command envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub schema: String,
    pub generated_at: String,
    pub profile: Option<String>,
    pub identity: Option<IdentityRef>,
    pub freshness: Vec<Value>,
    pub requests: Requests,
    pub partial: Vec<Value>,
    pub warnings: Vec<String>,
    pub outcome: String,
    pub exit: u8,
    pub result: Value,
}

/// Request counters.
#[derive(Debug, Clone, Serialize)]
pub struct Requests {
    pub api: u64,
    pub storage: u64,
    pub cost: Option<f64>,
}

impl Envelope {
    /// Start an envelope for a schema such as `canvas-cli/auth_login@1`.
    #[must_use]
    pub fn new(schema: &str, profile: Option<&str>, identity: Option<IdentityRef>) -> Self {
        let generated_at = jiff::Timestamp::now().to_string();
        Self {
            schema: schema.to_owned(),
            generated_at,
            profile: profile.map(str::to_owned),
            identity,
            freshness: Vec::new(),
            requests: Requests {
                api: 0,
                storage: 0,
                cost: None,
            },
            partial: Vec::new(),
            warnings: Vec::new(),
            outcome: "ok".into(),
            exit: 0,
            result: Value::Null,
        }
    }

    /// Attach the command `result` object.
    #[must_use]
    pub fn with_result(mut self, result: Value) -> Self {
        self.result = result;
        self
    }

    /// Set outcome and exit code.
    #[must_use]
    pub fn with_outcome(mut self, outcome: &str, exit: u8) -> Self {
        self.outcome = outcome.to_owned();
        self.exit = exit;
        self
    }

    /// Record API request count.
    #[must_use]
    pub fn with_api_requests(mut self, api: u64) -> Self {
        self.requests.api = api;
        self
    }

    /// Print the envelope as one JSON document on stdout.
    pub fn print_json(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("failed to encode JSON envelope: {e}"),
        }
    }

    /// Build a `canvas-cli/error@1` envelope.
    #[must_use]
    pub fn error(
        code: &str,
        message: &str,
        exit: u8,
        http_status: Option<u16>,
        profile: Option<&str>,
        identity: Option<IdentityRef>,
    ) -> Self {
        let result = json!({
            "code": code,
            "message": message,
            "http_status": http_status,
            "server_errors": [],
            "details": {},
        });
        Self::new("canvas-cli/error@1", profile, identity)
            .with_result(result)
            .with_outcome("error", exit)
    }
}

/// Print human text when not in `--json` mode.
pub fn print_human(lines: impl IntoIterator<Item = impl AsRef<str>>) {
    for line in lines {
        println!("{}", line.as_ref());
    }
}

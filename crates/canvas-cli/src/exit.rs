//! Exit codes and CLI errors (§14).

use std::fmt;
use std::process::ExitCode;

/// Process exit kinds used by M0-c commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    /// Generic failure.
    Generic = 1,
    /// Usage / argument error.
    Usage = 2,
    /// Auth / identity selection error.
    Auth = 3,
    /// Network failure.
    Network = 4,
    /// Exhausted rate-limit retries.
    RateLimited = 5,
    /// Completed diagnostics with failures.
    Partial = 12,
    /// User cancelled a confirmation.
    Cancelled = 11,
    /// Local persistence / credential store error.
    Local = 13,
}

impl ExitKind {
    /// Convert to a process [`ExitCode`].
    #[must_use]
    pub fn code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

/// Application error with a SPEC exit code.
#[derive(Debug)]
pub struct CliError {
    pub kind: ExitKind,
    pub message: String,
    pub http_status: Option<u16>,
    pub profile: Option<String>,
    pub identity: Option<Box<crate::output::IdentityRef>>,
    pub requests: canvas_api::Telemetry,
}

impl CliError {
    /// Build an error with an exit kind and message.
    #[must_use]
    pub fn new(kind: ExitKind, message: impl AsRef<str>) -> Self {
        Self {
            kind,
            message: message.as_ref().to_owned(),
            http_status: None,
            profile: None,
            identity: None,
            requests: canvas_api::Telemetry::default(),
        }
    }

    /// Usage error (exit 2).
    #[must_use]
    pub fn usage(message: impl AsRef<str>) -> Self {
        Self::new(ExitKind::Usage, message)
    }

    /// Auth error (exit 3).
    #[must_use]
    pub fn auth(message: impl AsRef<str>) -> Self {
        Self::new(ExitKind::Auth, message)
    }

    /// Network error (exit 4).
    #[must_use]
    pub fn network(message: impl AsRef<str>) -> Self {
        Self::new(ExitKind::Network, message)
    }

    /// Local persistence error (exit 13).
    #[must_use]
    pub fn local(message: impl AsRef<str>) -> Self {
        Self::new(ExitKind::Local, message)
    }

    pub fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    pub fn with_requests(mut self, requests: canvas_api::Telemetry) -> Self {
        self.requests = requests;
        self
    }

    pub fn for_selected(mut self, selected: &crate::selection::Selected) -> Self {
        self.profile = selected.profile_name.clone();
        self.identity = Some(Box::new(crate::output::IdentityRef::new(
            &selected.identity.origin,
            selected.identity.user_id,
            selected.identity.key.as_str(),
        )));
        self
    }

    /// Render exactly one abort envelope; empty messages mean a command already rendered its result.
    pub fn exit_with_json(self, json: bool) -> ExitCode {
        if json && !self.message.is_empty() {
            let code = match self.kind {
                ExitKind::Usage => "usage",
                ExitKind::Auth => "auth",
                ExitKind::Network => "network",
                ExitKind::RateLimited => "rate_limited",
                ExitKind::Local => "local",
                ExitKind::Cancelled => "cancelled",
                _ => "generic",
            };
            let mut envelope = crate::output::Envelope::error(
                code,
                &self.message,
                self.kind as u8,
                self.http_status,
                self.profile.as_deref(),
                self.identity.as_deref().cloned(),
            );
            envelope.requests.api = self.requests.api;
            envelope.requests.storage = self.requests.storage;
            envelope.requests.cost = self.requests.cost;
            envelope.print_json();
        }
        self.exit()
    }

    /// Convert to a process exit code after printing the message to stderr.
    pub fn exit(self) -> ExitCode {
        if !self.message.is_empty() {
            eprintln!("{}", self.message);
        }
        self.kind.code()
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<canvas_core::identity::IdentityError> for CliError {
    fn from(value: canvas_core::identity::IdentityError) -> Self {
        match value {
            canvas_core::identity::IdentityError::Changed => Self::local("identity changed"),
            canvas_core::identity::IdentityError::LockTimeout => {
                Self::local("identity lock timed out")
            }
            canvas_core::identity::IdentityError::Mismatch { reason } => {
                Self::local(format!("identity mismatch: {reason}"))
            }
            other => Self::local(other.to_string()),
        }
    }
}

impl From<canvas_core::store::StoreError> for CliError {
    fn from(value: canvas_core::store::StoreError) -> Self {
        Self::local(value.to_string())
    }
}

impl From<canvas_core::store::DbError> for CliError {
    fn from(value: canvas_core::store::DbError) -> Self {
        Self::local(value.to_string())
    }
}

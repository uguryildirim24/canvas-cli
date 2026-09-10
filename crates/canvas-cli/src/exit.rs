//! Exit codes and CLI errors (§14).

use std::fmt;
use std::process::ExitCode;

/// Process exit kinds used by M0-c commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    /// Success.
    Ok = 0,
    /// Generic failure.
    Generic = 1,
    /// Usage / argument error.
    Usage = 2,
    /// Auth / identity selection error.
    Auth = 3,
    /// Network failure.
    Network = 4,
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
}

impl CliError {
    /// Build an error with an exit kind and message.
    #[must_use]
    pub fn new(kind: ExitKind, message: impl AsRef<str>) -> Self {
        Self {
            kind,
            message: message.as_ref().to_owned(),
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

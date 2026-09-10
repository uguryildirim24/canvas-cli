//! Canvas LMS HTTP client, secrets, and API models.

pub mod error;

use std::fmt;

pub use error::Error;

/// An API token that never prints its value.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Wrap a raw token string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the raw token for authenticated requests.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// HTTP client stub; stores connection identity only.
#[derive(Debug)]
pub struct Client {
    origin: String,
    token: Secret,
    user_agent: String,
}

impl Client {
    /// Build a client that stores `origin`, `token`, and `user_agent`.
    pub fn new(origin: impl Into<String>, token: Secret, user_agent: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            token,
            user_agent: user_agent.into(),
        }
    }

    /// Canonical Canvas origin.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Redacted token handle.
    pub fn token(&self) -> &Secret {
        &self.token
    }

    /// User-Agent header value.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

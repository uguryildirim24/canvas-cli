//! Canvas API error variants (SPEC §11).

use thiserror::Error;

/// Errors returned by the Canvas API client.
#[derive(Debug, Error)]
pub enum Error {
    /// Missing or rejected credentials.
    #[error("unauthorized")]
    Unauthorized,

    /// Access forbidden; may be a rate-limit body.
    #[error("forbidden (rate_limited={rate_limited})")]
    Forbidden {
        /// True when the body indicates a rate limit.
        rate_limited: bool,
        /// Response body text.
        body: String,
    },

    /// Origin Canvas denied the transfer with this status.
    #[error("denied (status={status})")]
    Denied {
        /// HTTP status code.
        status: u16,
    },

    /// Resource was not found.
    #[error("not found")]
    NotFound,

    /// Rate limit retries were exhausted.
    #[error("rate limited")]
    RateLimited,

    /// Canvas returned a validation error body.
    #[error("validation (status={status})")]
    Validation {
        /// HTTP status code.
        status: u16,
        /// Canvas error messages.
        errors: Vec<String>,
    },

    /// A URL left the client origin.
    #[error("cross origin")]
    CrossOrigin,

    /// Redirect behaviour was not allowed for this method.
    #[error("unexpected redirect")]
    UnexpectedRedirect,

    /// Upload completion handoff failed.
    #[error("upload incomplete (status={status})")]
    UploadIncomplete {
        /// HTTP status of the incomplete upload response.
        status: u16,
    },

    /// Storage URL expired and must be refreshed.
    #[error("storage expired")]
    StorageExpired,

    /// Downloaded size did not match expectations.
    #[error("size mismatch")]
    SizeMismatch,

    /// Transport or DNS failure.
    #[error("network")]
    Network,

    /// Request timed out.
    #[error("timeout")]
    Timeout,

    /// Response body could not be decoded.
    #[error("decode")]
    Decode,
}

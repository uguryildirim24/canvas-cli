//! Sync errors and refresh outcome types.

use thiserror::Error;

use crate::store::{DbError, IngestError};

/// Failure modes for dataset refresh.
#[derive(Debug, Error)]
pub enum SyncError {
    /// `--offline` and no complete cached row exists (exit 7).
    #[error("offline cache miss")]
    OfflineMiss,
    /// Canvas API failure during fetch.
    #[error(transparent)]
    Api(#[from] canvas_api::Error),
    /// Store / `SQLite` failure.
    #[error(transparent)]
    Db(#[from] DbError),
    /// Ingest / epoch abort.
    #[error(transparent)]
    Ingest(#[from] IngestError),
}

/// Where freshness data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessSource {
    /// Served from a prior `fetch_log` row.
    Cache,
    /// Freshly downloaded and ingested.
    Network,
}

impl FreshnessSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Network => "network",
        }
    }
}

/// One freshness entry for command envelopes (§7).
#[derive(Debug, Clone)]
pub struct FreshnessInfo {
    pub dataset: String,
    pub scope: String,
    pub source: FreshnessSource,
    pub fetched_at: jiff::Timestamp,
    pub complete: bool,
    pub count: i64,
    pub stale: bool,
}

/// Result of a hit-predicate refresh attempt.
#[derive(Debug, Clone)]
pub struct RefreshOutcome {
    pub freshness: FreshnessInfo,
    pub requests: u32,
    pub error: Option<String>,
}

impl SyncError {
    /// Stable, credential-free diagnostic and process classification.
    pub fn classification(&self) -> (&'static str, u8, Option<u16>) {
        use canvas_api::Error as Api;
        match self {
            Self::OfflineMiss => ("offline", 7, None),
            Self::Db(_) | Self::Ingest(_) => ("local", 13, None),
            Self::Api(e) => match e {
                Api::Unauthorized => ("auth", 3, Some(401)),
                Api::Network | Api::Timeout => ("network", 4, None),
                Api::RateLimited => ("rate_limited", 5, Some(429)),
                Api::Forbidden {
                    rate_limited: true, ..
                } => ("rate_limited", 5, Some(403)),
                Api::Forbidden { .. } => ("refused", 8, Some(403)),
                Api::Denied { status } | Api::Validation { status, .. } => {
                    ("refused", 8, Some(*status))
                }
                Api::NotFound => ("not_found", 6, Some(404)),
                Api::CrossOrigin => ("resolution", 6, None),
                _ => ("response", 1, None),
            },
        }
    }
    pub fn safe_message(&self) -> String {
        match self {
            Self::Api(canvas_api::Error::Validation { status, .. }) => {
                format!("validation (status={status})")
            }
            _ => self.to_string(),
        }
    }
}

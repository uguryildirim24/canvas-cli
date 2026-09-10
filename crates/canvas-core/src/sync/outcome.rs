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

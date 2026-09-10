//! Local cache and state store.

mod dataset;
mod db;
mod migrate;
mod ops;

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

pub use dataset::{
    Dataset, EntityIngest, FakeEntity, FakeEntityPage, FakeEntityRow, FieldGroup, FieldWrite,
    IngestError, IngestOpts, IngestPage, LookupResult, Supplied, apply_field_writes,
    field_observed_at, upsert_enrollment_grades,
};
pub(crate) use db::auxiliary_sqlite;
pub use db::{DbError, Store, StoreConns};
pub use migrate::{CACHE_USER_VERSION, STATE_USER_VERSION};
pub use ops::{
    CACHE_TABLES, CacheStats, EpochAbort, FetchLogRow, LookupQuery, WindowQuery, bump_epochs,
    cache_clear, cache_path, cache_stats, load_fetch_log, lookup, lookup_dataset,
    pending_for_assignment, pending_journals_for_assignment, read_scope_epoch,
};

use crate::identity::{IdentityDocument, IdentityError, IdentityLock, Paths};

/// Errors opening or using the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("epoch advanced since refresh began")]
    EpochAbort,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Opened identity: shared lock + store handles.
pub struct OpenIdentity {
    /// Shared identity lock held for the process lifetime.
    pub lock: std::sync::Arc<IdentityLock>,
    /// Verified identity document (generation kept in memory).
    pub identity: IdentityDocument,
    /// Cache and state databases.
    pub store: Store,
}

impl OpenIdentity {
    /// Take the shared identity lock, re-verify, and open both databases.
    pub fn open(paths: &Paths, identity: &IdentityDocument) -> Result<Self, StoreError> {
        identity.verify()?;
        let store = Store::open(paths, identity)?;
        let lock = std::sync::Arc::clone(&store.lock);
        Ok(Self {
            lock,
            identity: identity.clone(),
            store,
        })
    }
}

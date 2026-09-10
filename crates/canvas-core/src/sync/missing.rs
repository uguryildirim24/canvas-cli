//! `missing` dataset (`all`).

#![allow(clippy::unnecessary_literal_bound)]

use canvas_api::models::Assignment;
use jiff::{Span, Timestamp};
use rusqlite::Transaction;

use crate::store::{Dataset, EntityIngest, IngestError, IngestPage};

use super::assignments::{assignment_to_entity, upsert_assignment};

/// Default TTL: `ttl_missing` = 10 minutes.
#[must_use]
pub fn default_ttl_missing() -> Span {
    Span::new().minutes(10)
}

/// Missing-submissions dataset.
#[derive(Debug, Clone)]
pub struct MissingDataset {
    pub ttl: Span,
}

impl MissingDataset {
    #[must_use]
    pub fn new(ttl: Span) -> Self {
        Self { ttl }
    }

    #[must_use]
    pub fn with_default_ttl() -> Self {
        Self::new(default_ttl_missing())
    }
}

impl Dataset for MissingDataset {
    fn name(&self) -> &'static str {
        "missing"
    }

    fn scope_key(&self) -> &str {
        "all"
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "assignment"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_assignment(tx, None, entity, fetched_at)
    }
}

/// Path for missing submissions (no `filter[]`).
#[must_use]
pub fn missing_path() -> String {
    "/api/v1/users/self/missing_submissions?include[]=planner_overrides&include[]=course&per_page=100"
        .into()
}

/// Convert missing-submission rows into an ingest page.
#[must_use]
pub fn missing_to_ingest_page(items: &[Assignment], fetched_at: Timestamp) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: items
            .iter()
            .map(|a| {
                let mut entity = assignment_to_entity(a, a.course_id);
                // Missing list implies missing = true.
                entity.fields.retain(|f| f.name != "missing");
                entity.fields.push(crate::store::FieldWrite {
                    name: "missing",
                    group: crate::store::FieldGroup::Status,
                    value: Some("true".into()),
                });
                entity
            })
            .collect(),
    }
}

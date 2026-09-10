//! Single-assignment coverage without replacing list membership.
use super::refresh::{FetchBundle, refresh_dataset};
use super::wire::Observed;
use super::{AssignmentsDataset, RefreshOutcome, SyncError, assignment_detail_path};
use crate::store::{Dataset, EntityIngest, IngestError, IngestPage, Store};
use canvas_api::{Client, models::Assignment};
use jiff::{Span, Timestamp};
use rusqlite::Transaction;

#[derive(Clone)]
pub struct AssignmentDetailDataset {
    pub course_id: i64,
    pub assignment_id: i64,
    scope: String,
    assignments: AssignmentsDataset,
}
impl AssignmentDetailDataset {
    pub fn new(course_id: i64, assignment_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            assignment_id,
            scope: format!("assignment:{assignment_id}"),
            assignments: AssignmentsDataset::new(course_id, ttl),
        }
    }
}
impl Dataset for AssignmentDetailDataset {
    fn name(&self) -> &'static str {
        "assignment"
    }
    fn scope_key(&self) -> &str {
        &self.scope
    }
    fn epoch_scope(&self) -> String {
        self.assignments.epoch_scope()
    }
    fn ttl(&self) -> Span {
        self.assignments.ttl
    }
    fn entity_kind(&self) -> &'static str {
        "assignment"
    }
    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        at: Timestamp,
    ) -> Result<(), IngestError> {
        self.assignments.upsert_entity(tx, entity, at)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn refresh_assignment(
    client: &Client,
    store: &Store,
    course_id: i64,
    assignment_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = AssignmentDetailDataset::new(course_id, assignment_id, ttl);
    refresh_dataset(
        client,
        store,
        &dataset,
        now,
        fresh,
        offline,
        None,
        None,
        || async {
            let row: Observed<Assignment> = client
                .get(&assignment_detail_path(course_id, assignment_id))
                .await?;
            if row.model.id != assignment_id
                || row.model.course_id.is_some_and(|id| id != course_id)
            {
                return Err(canvas_api::Error::Decode.into());
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![row.entity(Some(course_id))],
                }],
            })
        },
    )
    .await
}

//! Single-course detail coverage, kept separate from list memberships.
use super::refresh::{FetchBundle, refresh_dataset};
use super::wire::Observed;
use super::{CoursesDataset, CoursesScope, RefreshOutcome, SyncError};
use crate::store::{Dataset, DbError, EntityIngest, IngestError, IngestPage, Store};
use canvas_api::{Client, models::Course};
use jiff::{Span, Timestamp};
use rusqlite::{Connection, Transaction};

#[derive(Clone)]
pub struct CourseDetailDataset {
    id: i64,
    scope: String,
    courses: CoursesDataset,
}
impl CourseDetailDataset {
    pub fn new(id: i64, ttl: Span) -> Self {
        Self {
            id,
            scope: format!("course:{id}"),
            courses: CoursesDataset::new(CoursesScope::Active, ttl),
        }
    }
}
impl Dataset for CourseDetailDataset {
    fn name(&self) -> &'static str {
        "course"
    }
    fn scope_key(&self) -> &str {
        &self.scope
    }
    fn ttl(&self) -> Span {
        self.courses.ttl
    }
    fn entity_kind(&self) -> &'static str {
        "course"
    }
    fn current_epoch(&self, state: &Connection) -> Result<i64, DbError> {
        self.courses.current_epoch(state)
    }
    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        at: Timestamp,
    ) -> Result<(), IngestError> {
        if entity.entity_key != self.id.to_string() {
            return Err(DbError::Message("course response id mismatch".into()).into());
        }
        self.courses.upsert_entity(tx, entity, at)
    }
    fn finish_refresh(
        &self,
        tx: &Transaction<'_>,
        state: &Connection,
        pages: &[IngestPage],
    ) -> Result<(), IngestError> {
        super::courses::publish_derived(tx, state, pages, self.scope_key())
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn refresh_course(
    client: &Client,
    store: &Store,
    id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = CourseDetailDataset::new(id, ttl);
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
            let path = format!("/api/v1/courses/{id}?include[]=term&include[]=syllabus_body&include[]=teachers&include[]=total_scores&include[]=current_grading_period_scores");
            let course: Observed<Course> = client.get(&path).await?;
            if course.model.id != id {
                return Err(canvas_api::Error::Decode.into());
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![course.entity("").await?],
                }],
            })
        },
    )
    .await
}

//! Hit-predicate refresh helpers.

use canvas_api::Client;
use canvas_api::models::{Course, Enrollment, GradingPeriod};
use futures_util::StreamExt;
use jiff::{Span, Timestamp};

use crate::store::{
    Dataset, DbError, FetchLogRow, IngestOpts, IngestPage, LookupResult, Store, lookup_dataset,
    read_scope_epoch,
};

use super::courses::{CoursesDataset, CoursesScope, courses_path, courses_to_ingest_page};
use super::enrollment_grades::{
    EnrollmentGradesDataset, PeriodKey, enrollment_grades_path, enrollments_to_ingest_page,
};
use super::grading_periods::{
    GradingPeriodsDataset, grading_periods_path, grading_periods_to_ingest_page,
};
use super::outcome::{FreshnessInfo, FreshnessSource, RefreshOutcome, SyncError};

/// Refresh the `courses` dataset for `scope`.
pub async fn refresh_courses(
    client: &Client,
    store: &Store,
    scope: CoursesScope,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = CoursesDataset::new(scope, ttl);
    refresh_dataset(store, &dataset, now, fresh, offline, || async {
        let mut pages = Vec::new();
        let mut requests = 0u32;
        for state in scope.enrollment_states() {
            let path = courses_path(state);
            let mut items = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Course>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                requests = requests.saturating_add(1);
                items.extend(page.items);
            }
            pages.push(courses_to_ingest_page(&items, state, now));
        }
        Ok(FetchBundle { pages, requests })
    })
    .await
}

/// Refresh `enrollment_grades` for a period scope.
pub async fn refresh_enrollment_grades(
    client: &Client,
    store: &Store,
    period: PeriodKey,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = EnrollmentGradesDataset::new(period, ttl);
    refresh_dataset(store, &dataset, now, fresh, offline, || async {
        let path = enrollment_grades_path(period);
        let mut items = Vec::new();
        let mut requests = 0u32;
        let mut stream = std::pin::pin!(client.get_all::<Enrollment>(&path));
        while let Some(page) = stream.next().await {
            let page = page?;
            requests = requests.saturating_add(1);
            items.extend(page.items);
        }
        Ok(FetchBundle {
            pages: vec![enrollments_to_ingest_page(&items, period, now)],
            requests: requests.max(1),
        })
    })
    .await
}

/// Refresh `grading_periods` for one course.
pub async fn refresh_grading_periods(
    client: &Client,
    store: &Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = GradingPeriodsDataset::new(course_id, ttl);
    refresh_dataset(store, &dataset, now, fresh, offline, || async {
        let path = grading_periods_path(course_id);
        let mut items = Vec::new();
        let mut requests = 0u32;
        let mut stream = std::pin::pin!(client.get_all_wrapped::<GradingPeriod>(&path));
        while let Some(page) = stream.next().await {
            let page = page?;
            requests = requests.saturating_add(1);
            items.extend(page.items);
        }
        Ok(FetchBundle {
            pages: vec![grading_periods_to_ingest_page(&items, course_id, now)],
            requests: requests.max(1),
        })
    })
    .await
}

struct FetchBundle {
    pages: Vec<IngestPage>,
    requests: u32,
}

async fn refresh_dataset<D, F, Fut>(
    store: &Store,
    dataset: &D,
    now: Timestamp,
    fresh: bool,
    offline: bool,
    fetch: F,
) -> Result<RefreshOutcome, SyncError>
where
    D: Dataset + Clone + Send + Sync + 'static,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<FetchBundle, SyncError>>,
{
    let lookup = store
        .call({
            let dataset = dataset.clone();
            move |conns| lookup_dataset(conns, &dataset, now, None)
        })
        .await?;

    if let LookupResult::Hit(row) = &lookup
        && !fresh
    {
        return Ok(cache_outcome(row, false, 0, None));
    }

    if offline {
        return offline_outcome(lookup);
    }

    let epoch_seen = store
        .call({
            let scope = dataset.epoch_scope();
            move |conns| read_scope_epoch(&conns.state, &scope)
        })
        .await?;

    match fetch().await {
        Ok(bundle) => {
            let count = bundle.pages.iter().map(|p| p.entities.len()).sum::<usize>();
            let count = i64::try_from(count).unwrap_or(i64::MAX);
            let requests = bundle.requests;
            let pages = bundle.pages;
            let dataset_for_ingest = dataset.clone();
            store
                .call(move |conns| {
                    dataset_for_ingest
                        .ingest(
                            &pages,
                            &IngestOpts {
                                epoch_seen,
                                complete: true,
                                stale: false,
                                error: None,
                                window: None,
                                contexts: None,
                            },
                            conns,
                        )
                        .map_err(|e| ingest_err(&e))
                })
                .await?;

            Ok(RefreshOutcome {
                freshness: FreshnessInfo {
                    dataset: dataset.name().to_owned(),
                    scope: dataset.scope_key().to_owned(),
                    source: FreshnessSource::Network,
                    fetched_at: now,
                    complete: true,
                    count,
                    stale: false,
                },
                requests,
                error: None,
            })
        }
        Err(err) => {
            let message = sanitize_error(&err);
            let dataset_fail = dataset.clone();
            let fail_msg = message.clone();
            let _ = store
                .call(move |conns| {
                    dataset_fail
                        .ingest(
                            &[],
                            &IngestOpts {
                                epoch_seen,
                                complete: false,
                                stale: true,
                                error: Some(fail_msg.as_str()),
                                window: None,
                                contexts: None,
                            },
                            conns,
                        )
                        .map_err(|e| ingest_err(&e))
                })
                .await;

            match complete_row(lookup) {
                Some(row) => Ok(cache_outcome(&row, true, 0, Some(message))),
                None => Err(err),
            }
        }
    }
}

fn offline_outcome(lookup: LookupResult) -> Result<RefreshOutcome, SyncError> {
    match complete_row(lookup) {
        Some(row) => Ok(cache_outcome(&row, true, 0, None)),
        None => Err(SyncError::OfflineMiss),
    }
}

fn complete_row(lookup: LookupResult) -> Option<FetchLogRow> {
    match lookup {
        LookupResult::Hit(row) | LookupResult::Stale(row) if row.complete => Some(row),
        LookupResult::Hit(_) | LookupResult::Stale(_) | LookupResult::Miss => None,
    }
}

fn cache_outcome(
    row: &FetchLogRow,
    stale: bool,
    requests: u32,
    error: Option<String>,
) -> RefreshOutcome {
    RefreshOutcome {
        freshness: FreshnessInfo {
            dataset: row.dataset.clone(),
            scope: row.scope.clone(),
            source: FreshnessSource::Cache,
            fetched_at: row.fetched_at,
            complete: row.complete,
            count: row.count,
            stale,
        },
        requests,
        error,
    }
}

fn ingest_err(err: &crate::store::IngestError) -> DbError {
    DbError::Message(err.to_string())
}

/// Prefer a short, non-credential summary for `fetch_log.error`.
fn sanitize_error(err: &SyncError) -> String {
    match err {
        SyncError::Api(e) => e.to_string(),
        SyncError::OfflineMiss => "offline cache miss".into(),
        SyncError::Db(e) => e.to_string(),
        SyncError::Ingest(e) => e.to_string(),
    }
}

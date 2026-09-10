//! Hit-predicate refresh helpers.

#![allow(clippy::too_many_arguments, clippy::too_many_lines)]

use canvas_api::Client;
use canvas_api::models::{Assignment, Course, Enrollment, GradingPeriod};
use futures_util::StreamExt;
use jiff::{Span, Timestamp};

use crate::store::{
    Dataset, DbError, FetchLogRow, IngestOpts, IngestPage, LookupResult, Store, WindowQuery,
    lookup_dataset,
};

use super::assignments::{AssignmentsDataset, assignments_path};
use super::courses::{CoursesDataset, CoursesScope, courses_path};
use super::enrollment_grades::{EnrollmentGradesDataset, PeriodKey, enrollment_grades_path};
use super::grading_periods::{GradingPeriodsDataset, grading_periods_path};
use super::missing::{MissingDataset, missing_path};
use super::outcome::{FreshnessInfo, FreshnessSource, RefreshOutcome, SyncError};
use super::planner::{PlannerDataset, PlannerWindow, planner_path};
use super::submission::{SubmissionDataset, observed_submission, submission_path};
use super::wire::Observed;

/// Optional window metadata written into `fetch_log`.
#[derive(Debug, Clone)]
pub struct WindowMeta {
    pub start: Timestamp,
    pub end: Timestamp,
    pub contexts: String,
}

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
            let mut pages = Vec::new();
            for state in scope.enrollment_states() {
                let path = courses_path(state);
                let mut items = Vec::new();
                let mut stream = std::pin::pin!(client.get_all::<Observed<Course>>(&path));
                while let Some(page) = stream.next().await {
                    let page = page?;
                    items.extend(page.items);
                }
                let mut entities = Vec::new();
                for item in items {
                    entities.push(item.entity(state).await?);
                }
                pages.push(IngestPage {
                    fetched_at: now,
                    entities,
                });
            }
            Ok(FetchBundle { pages })
        },
    )
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
            let path = enrollment_grades_path(period);
            let mut items = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Observed<Enrollment>>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                items.extend(page.items);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: items
                        .into_iter()
                        .map(|e| e.entity(&period.period_value()))
                        .collect(),
                }],
            })
        },
    )
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
            let path = grading_periods_path(course_id);
            let mut items = Vec::new();
            let mut stream =
                std::pin::pin!(client.get_all_wrapped::<Observed<GradingPeriod>>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                items.extend(page.items);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: items.into_iter().map(|p| p.entity(course_id)).collect(),
                }],
            })
        },
    )
    .await
}

/// Refresh `assignments` for one course.
pub async fn refresh_assignments(
    client: &Client,
    store: &Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = AssignmentsDataset::new(course_id, ttl);
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
            let path = assignments_path(course_id);
            let mut items = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Observed<Assignment>>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                items.extend(page.items);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: items
                        .into_iter()
                        .map(|a| a.entity(Some(course_id)))
                        .collect(),
                }],
            })
        },
    )
    .await
}

/// Refresh `missing` (`all`).
pub async fn refresh_missing(
    client: &Client,
    store: &Store,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = MissingDataset::new(ttl);
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
            let path = missing_path();
            let mut items = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Observed<Assignment>>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                items.extend(page.items);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: items.into_iter().map(|o| o.entity(None)).collect(),
                }],
            })
        },
    )
    .await
}

/// Refresh `planner` for a UTC-day window.
pub async fn refresh_planner(
    client: &Client,
    store: &Store,
    window: PlannerWindow,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = PlannerDataset::new(window.clone(), ttl);
    let meta = WindowMeta {
        start: window.start_timestamp(),
        end: window.end_timestamp(),
        contexts: String::new(),
    };
    let lookup = Some((meta.start, meta.end, meta.contexts.clone()));
    refresh_dataset(
        client,
        store,
        &dataset,
        now,
        fresh,
        offline,
        lookup,
        Some(meta),
        || async {
            let path = planner_path(&window);
            let mut items = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<serde_json::Value>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                items.extend(page.items);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: items
                        .into_iter()
                        .map(|raw| super::planner::observed_planner(&raw))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect(),
                }],
            })
        },
    )
    .await
}

/// Refresh `submission` for one assignment.
pub async fn refresh_submission(
    client: &Client,
    store: &Store,
    course_id: i64,
    assignment_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = SubmissionDataset::new(course_id, assignment_id, ttl);
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
            let path = submission_path(course_id, assignment_id);
            let submission = observed_submission(&client.get(&path).await?, assignment_id)?;
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![submission],
                }],
            })
        },
    )
    .await
}

pub(super) struct FetchBundle {
    pub pages: Vec<IngestPage>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn refresh_dataset<D, F, Fut>(
    client: &Client,
    store: &Store,
    dataset: &D,
    now: Timestamp,
    fresh: bool,
    offline: bool,
    window_lookup: Option<(Timestamp, Timestamp, String)>,
    window_meta: Option<WindowMeta>,
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
            let window_lookup = window_lookup.clone();
            move |conns| {
                let window = window_lookup
                    .as_ref()
                    .map(|(start, end, contexts)| WindowQuery {
                        start: *start,
                        end: *end,
                        contexts: contexts.as_str(),
                    });
                lookup_dataset(conns, &dataset, now, window)
            }
        })
        .await?;

    if let LookupResult::Hit(row) = &lookup
        && !fresh
        && !offline
    {
        return Ok(cache_outcome(row, false, 0, None));
    }

    if offline {
        return offline_outcome(lookup);
    }

    let epoch_seen = store
        .call({
            let dataset = dataset.clone();
            move |conns| dataset.current_epoch(&conns.state)
        })
        .await?;

    let before = client.telemetry().api;
    match fetch().await {
        Ok(bundle) => {
            let count = bundle
                .pages
                .iter()
                .flat_map(|p| &p.entities)
                .map(|e| &e.entity_key)
                .collect::<std::collections::HashSet<_>>()
                .len();
            let count = i64::try_from(count).unwrap_or(i64::MAX);
            let requests =
                u32::try_from(client.telemetry().api.saturating_sub(before)).unwrap_or(u32::MAX);
            let pages = bundle.pages;
            let dataset_for_ingest = dataset.clone();
            let window_pair = window_meta.as_ref().map(|m| (m.start, m.end));
            let contexts = window_meta.map(|m| m.contexts);
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
                                window: window_pair,
                                contexts: contexts.as_deref(),
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
            if matches!(err, SyncError::Api(canvas_api::Error::Unauthorized)) {
                return Err(err);
            }
            let message = sanitize_error(&err);
            let dataset_fail = dataset.clone();
            let fail_msg = message.clone();
            let window_pair = window_meta.as_ref().map(|m| (m.start, m.end));
            let contexts = window_meta.map(|m| m.contexts);
            store
                .call(move |conns| {
                    dataset_fail
                        .ingest(
                            &[],
                            &IngestOpts {
                                epoch_seen,
                                complete: false,
                                stale: true,
                                error: Some(fail_msg.as_str()),
                                window: window_pair,
                                contexts: contexts.as_deref(),
                            },
                            conns,
                        )
                        .map_err(|e| ingest_err(&e))
                })
                .await?;

            match complete_row(lookup) {
                Some(row) => Ok(cache_outcome(
                    &row,
                    true,
                    u32::try_from(client.telemetry().api.saturating_sub(before))
                        .unwrap_or(u32::MAX),
                    Some(message),
                )),
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
    err.safe_message()
}

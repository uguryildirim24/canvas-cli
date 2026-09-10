//! Hit-predicate refresh helpers.

#![allow(clippy::too_many_arguments, clippy::too_many_lines)]

use canvas_api::Client;
use canvas_api::models::{
    Assignment, Course, Enrollment, File, Folder, GradingPeriod, Module, ModuleItem,
};
use futures_util::StreamExt;
use jiff::{Span, Timestamp};

use crate::store::{
    Dataset, DbError, FetchLogRow, IngestOpts, IngestPage, LookupResult, Store, WindowQuery,
    lookup_dataset,
};

use super::assignments::{AssignmentsDataset, assignments_path};
use super::courses::{CoursesDataset, CoursesScope, courses_path};
use super::enrollment_grades::{EnrollmentGradesDataset, PeriodKey, enrollment_grades_path};
use super::files::{FilesDataset, files_path};
use super::folders::{FoldersDataset, folders_path};
use super::grading_periods::{GradingPeriodsDataset, grading_periods_path};
use super::missing::{MissingDataset, missing_path};
use super::modules::{ModulesDataset, module_items_path, modules_path, needs_items_fetch};
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

/// Refresh `folders` for one course (listing denial classification).
pub async fn refresh_folders(
    client: &Client,
    store: &Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = FoldersDataset::new(course_id, ttl);
    refresh_listing_with_denial(client, store, &dataset, now, fresh, offline, || async {
        let path = folders_path(course_id);
        let mut items = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<Observed<Folder>>(&path));
        while let Some(page) = stream.next().await {
            let page = page?;
            items.extend(page.items);
        }
        Ok(FetchBundle {
            pages: vec![IngestPage {
                fetched_at: now,
                entities: items.into_iter().map(|f| f.entity(course_id)).collect(),
            }],
        })
    })
    .await
}

/// Refresh `files` for one course (listing denial classification).
pub async fn refresh_files(
    client: &Client,
    store: &Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = FilesDataset::new(course_id, ttl);
    refresh_listing_with_denial(client, store, &dataset, now, fresh, offline, || async {
        let path = files_path(course_id);
        let mut items = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<Observed<File>>(&path));
        while let Some(page) = stream.next().await {
            let page = page?;
            items.extend(page.items);
        }
        Ok(FetchBundle {
            pages: vec![IngestPage {
                fetched_at: now,
                entities: items.into_iter().map(|f| f.entity(course_id)).collect(),
            }],
        })
    })
    .await
}

/// Refresh `modules` for one course (inline-item completeness).
pub async fn refresh_modules(
    client: &Client,
    store: &Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<RefreshOutcome, SyncError> {
    let dataset = ModulesDataset::new(course_id, ttl);
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
            let path = modules_path(course_id);
            let mut modules = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Observed<Module>>(&path));
            while let Some(page) = stream.next().await {
                let page = page?;
                modules.extend(page.items);
            }
            let mut entities = Vec::new();
            for observed in modules {
                entities.push(resolve_module_entity(client, course_id, observed).await?);
            }
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities,
                }],
            })
        },
    )
    .await
}

async fn resolve_module_entity(
    client: &Client,
    course_id: i64,
    observed: Observed<Module>,
) -> Result<crate::store::EntityIngest, SyncError> {
    let module_id = observed.model.id;
    let needs = needs_items_fetch(&observed.model);
    let (items, items_complete) = if needs {
        let path = module_items_path(course_id, module_id);
        let mut fetched = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<Observed<ModuleItem>>(&path));
        while let Some(page) = stream.next().await {
            let page = page?;
            fetched.extend(page.items);
        }
        (fetched, true)
    } else {
        (observed.inline_items()?, true)
    };
    let items: Vec<_> = items
        .into_iter()
        .map(|item| item.entity(course_id, module_id, observed.model.state.as_deref()))
        .collect();
    Ok(observed.entity(course_id, items_complete, &items))
}

/// How a folders/files listing API error should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialAction {
    /// Propagate as auth failure (exit 3).
    Auth,
    /// Propagate as throttle (exit 5); do not record as listing denial.
    Throttle,
    /// Record a complete denial with this HTTP status.
    RecordDenial { status: u16 },
    /// Use mark-stale / serve-complete-row behavior.
    Propagate,
}

/// Map a Canvas API error onto listing denial classification.
#[must_use]
pub fn classify_listing_denial(err: &canvas_api::Error) -> DenialAction {
    use canvas_api::Error as Api;
    match err {
        Api::Unauthorized => DenialAction::Auth,
        Api::RateLimited
        | Api::Forbidden {
            rate_limited: true, ..
        } => DenialAction::Throttle,
        Api::Forbidden {
            rate_limited: false,
            ..
        }
        | Api::Denied { status: 403 } => DenialAction::RecordDenial { status: 403 },
        Api::Denied { status: 404 } | Api::NotFound => DenialAction::RecordDenial { status: 404 },
        _ => DenialAction::Propagate,
    }
}

/// Parse `unavailable:NNN` from a `fetch_log` error string.
#[must_use]
pub fn listing_denial_status(error: &str) -> Option<u16> {
    error
        .strip_prefix("unavailable:")
        .and_then(|rest| rest.parse().ok())
        .filter(|status| matches!(status, 403 | 404))
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
            ingest_success(
                client,
                store,
                dataset,
                now,
                before,
                epoch_seen,
                bundle,
                window_meta,
            )
            .await
        }
        Err(err) => {
            if matches!(err, SyncError::Api(canvas_api::Error::Unauthorized)) {
                return Err(err);
            }
            mark_stale_or_serve(
                client,
                store,
                dataset,
                before,
                epoch_seen,
                lookup,
                err,
                window_meta,
            )
            .await
        }
    }
}

/// Listing refresh with denial classification for folders/files.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn refresh_listing_with_denial<D, F, Fut>(
    client: &Client,
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
        && !offline
    {
        return Ok(cache_outcome(row, false, 0, row.error.clone()));
    }

    if offline {
        return match complete_row(lookup) {
            Some(row) => Ok(cache_outcome(&row, true, 0, row.error.clone())),
            None => Err(SyncError::OfflineMiss),
        };
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
            ingest_success(
                client, store, dataset, now, before, epoch_seen, bundle, None,
            )
            .await
        }
        Err(err) => {
            let SyncError::Api(ref api_err) = err else {
                return mark_stale_or_serve(
                    client, store, dataset, before, epoch_seen, lookup, err, None,
                )
                .await;
            };
            match classify_listing_denial(api_err) {
                DenialAction::Auth | DenialAction::Throttle => Err(err),
                DenialAction::RecordDenial { status } => {
                    let denial = format!("unavailable:{status}");
                    let requests = u32::try_from(client.telemetry().api.saturating_sub(before))
                        .unwrap_or(u32::MAX);
                    let dataset_for_ingest = dataset.clone();
                    let denial_for_ingest = denial.clone();
                    store
                        .call(move |conns| {
                            dataset_for_ingest
                                .ingest(
                                    &[IngestPage {
                                        fetched_at: now,
                                        entities: Vec::new(),
                                    }],
                                    &IngestOpts {
                                        epoch_seen,
                                        complete: true,
                                        stale: false,
                                        error: Some(denial_for_ingest.as_str()),
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
                            count: 0,
                            stale: false,
                        },
                        requests,
                        error: Some(denial),
                    })
                }
                DenialAction::Propagate => {
                    mark_stale_or_serve(
                        client, store, dataset, before, epoch_seen, lookup, err, None,
                    )
                    .await
                }
            }
        }
    }
}

pub(super) async fn ingest_success<D: Dataset + Clone + Send + Sync + 'static>(
    client: &Client,
    store: &Store,
    dataset: &D,
    now: Timestamp,
    before: u64,
    epoch_seen: i64,
    bundle: FetchBundle,
    window_meta: Option<WindowMeta>,
) -> Result<RefreshOutcome, SyncError> {
    let count = bundle
        .pages
        .iter()
        .flat_map(|p| &p.entities)
        .map(|e| &e.entity_key)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    let requests = u32::try_from(client.telemetry().api.saturating_sub(before)).unwrap_or(u32::MAX);
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

pub(super) async fn mark_stale_or_serve<D: Dataset + Clone + Send + Sync + 'static>(
    client: &Client,
    store: &Store,
    dataset: &D,
    before: u64,
    epoch_seen: i64,
    lookup: LookupResult,
    err: SyncError,
    window_meta: Option<WindowMeta>,
) -> Result<RefreshOutcome, SyncError> {
    // A failed retry changes freshness, not the availability of the retained listing.
    let denial = match &lookup {
        LookupResult::Hit(row) | LookupResult::Stale(row)
            if matches!(dataset.name(), "files" | "folders")
                && row
                    .error
                    .as_deref()
                    .and_then(listing_denial_status)
                    .is_some() =>
        {
            row.error.clone()
        }
        _ => None,
    };
    let message = denial.unwrap_or_else(|| sanitize_error(&err));
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
            u32::try_from(client.telemetry().api.saturating_sub(before)).unwrap_or(u32::MAX),
            Some(message),
        )),
        None => Err(err),
    }
}

pub(super) fn offline_outcome(lookup: LookupResult) -> Result<RefreshOutcome, SyncError> {
    match complete_row(lookup) {
        Some(row) => Ok(cache_outcome(&row, true, 0, None)),
        None => Err(SyncError::OfflineMiss),
    }
}

pub(super) fn complete_row(lookup: LookupResult) -> Option<FetchLogRow> {
    match lookup {
        LookupResult::Hit(row) | LookupResult::Stale(row) if row.complete => Some(row),
        LookupResult::Hit(_) | LookupResult::Stale(_) | LookupResult::Miss => None,
    }
}

pub(super) fn cache_outcome(
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

pub(super) fn ingest_err(err: &crate::store::IngestError) -> DbError {
    DbError::Message(err.to_string())
}

/// Prefer a short, non-credential summary for `fetch_log.error`.
fn sanitize_error(err: &SyncError) -> String {
    err.safe_message()
}

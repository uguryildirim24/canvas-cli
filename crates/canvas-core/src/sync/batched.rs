//! Batch-isolating refresh for the window datasets (§12.5, §12.6).
//!
//! Canvas takes at most ten `context_codes[]` per request, and one course the
//! student cannot read fails the whole batch. A non-throttle denial therefore
//! retries the batch one context at a time: the contexts that answer are
//! stored, the ones that still fail are recorded. Coverage stays complete
//! because every batch was stored or isolated, and the failures are reported as
//! `partial[]` instead of hiding behind an empty list.

use canvas_api::Client;
use canvas_api::models::{Announcement, CalendarEvent};
use futures_util::StreamExt;
use jiff::{Span, Timestamp};

use crate::store::{
    Dataset, EntityIngest, IngestOpts, IngestPage, LookupResult, Store, WindowQuery,
    encode_context_denials, lookup_dataset, parse_context_denials,
};

use super::announcements::{AnnouncementsDataset, announcements_path};
use super::calendar_events::{CalendarEventsDataset, calendar_events_path};
use super::context_window::ContextWindow;
use super::outcome::{FreshnessInfo, FreshnessSource, RefreshOutcome, SyncError};
use super::refresh::{
    DenialAction, FetchBundle, WindowMeta, cache_outcome, classify_listing_denial, complete_row,
    ingest_err, ingest_success, mark_stale_or_serve,
};
use super::wire::Observed;

/// One context that stayed unreadable after its batch was isolated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDenial {
    /// Canvas context code, for example `course_45679`.
    pub context: String,
    /// HTTP status the isolated request returned.
    pub http_status: u16,
}

impl ContextDenial {
    /// Course id, when the context is a course.
    #[must_use]
    pub fn course_id(&self) -> Option<i64> {
        super::announcements::course_id_from_context(&self.context)
    }
}

/// A batched refresh: the dataset outcome plus the contexts it could not read.
#[derive(Debug, Clone)]
pub struct BatchOutcome {
    pub outcome: RefreshOutcome,
    pub denials: Vec<ContextDenial>,
}

/// Refresh `announcements` for a window over a set of courses.
pub async fn refresh_announcements(
    client: &Client,
    store: &Store,
    window: ContextWindow,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<BatchOutcome, SyncError> {
    let dataset = AnnouncementsDataset::new(window.clone(), ttl);
    refresh_batched(
        client,
        store,
        &dataset,
        &window,
        now,
        fresh,
        offline,
        |batch| {
            let path = announcements_path(&batch, &window);
            async move {
                let mut entities = Vec::new();
                let mut stream = std::pin::pin!(client.get_all::<Observed<Announcement>>(&path));
                while let Some(page) = stream.next().await {
                    for item in page?.items {
                        entities.push(item.entity());
                    }
                }
                Ok(entities)
            }
        },
    )
    .await
}

/// Refresh `calendar_events` for a window over a set of contexts.
pub async fn refresh_calendar_events(
    client: &Client,
    store: &Store,
    window: ContextWindow,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<BatchOutcome, SyncError> {
    let dataset = CalendarEventsDataset::new(window.clone(), ttl);
    refresh_batched(
        client,
        store,
        &dataset,
        &window,
        now,
        fresh,
        offline,
        |batch| {
            let path = calendar_events_path(&batch, &window);
            async move {
                let mut entities = Vec::new();
                let mut stream = std::pin::pin!(client.get_all::<Observed<CalendarEvent>>(&path));
                while let Some(page) = stream.next().await {
                    for item in page?.items {
                        entities.push(item.entity());
                    }
                }
                Ok(entities)
            }
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn refresh_batched<D, F, Fut>(
    client: &Client,
    store: &Store,
    dataset: &D,
    window: &ContextWindow,
    now: Timestamp,
    fresh: bool,
    offline: bool,
    fetch_batch: F,
) -> Result<BatchOutcome, SyncError>
where
    D: Dataset + Clone + Send + Sync + 'static,
    F: Fn(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<EntityIngest>, SyncError>>,
{
    let query = (
        window.window().start_timestamp(),
        window.window().end_timestamp(),
        window.context_hash().to_owned(),
    );
    let lookup = store
        .call({
            let dataset = dataset.clone();
            let query = query.clone();
            move |conns| {
                lookup_dataset(
                    conns,
                    &dataset,
                    now,
                    Some(WindowQuery {
                        start: query.0,
                        end: query.1,
                        contexts: query.2.as_str(),
                    }),
                )
            }
        })
        .await?;

    if let LookupResult::Hit(row) = &lookup
        && !fresh
        && !offline
    {
        return Ok(cached(cache_outcome(row, false, 0, row.error.clone())));
    }
    if offline {
        return match complete_row(lookup) {
            Some(row) => Ok(cached(cache_outcome(&row, true, 0, row.error.clone()))),
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

    let (entities, denials) = match fetch_batches(window, &fetch_batch).await? {
        Batched::Complete { entities, denials } => (entities, denials),
        Batched::Propagate(err) => {
            return propagate(
                client, store, dataset, before, epoch_seen, lookup, err, window,
            )
            .await;
        }
    };

    let error = encode_context_denials(&denials);
    let bundle = FetchBundle {
        pages: vec![IngestPage {
            fetched_at: now,
            entities,
        }],
    };
    let outcome = match error {
        None => {
            ingest_success(
                client,
                store,
                dataset,
                now,
                before,
                epoch_seen,
                bundle,
                Some(window_meta(window)),
            )
            .await?
        }
        Some(error) => {
            ingest_with_denials(
                client, store, dataset, now, before, epoch_seen, bundle, window, &error,
            )
            .await?
        }
    };
    let denials = decode_denials(outcome.error.as_deref());
    Ok(BatchOutcome { outcome, denials })
}

/// Outcome of walking every batch: either full coverage (with any isolated
/// contexts named), or an error that belongs to the dataset rather than to one
/// context.
enum Batched {
    Complete {
        entities: Vec<EntityIngest>,
        denials: Vec<(String, u16)>,
    },
    Propagate(SyncError),
}

/// Fetch every batch, isolating the ones a per-context denial breaks.
async fn fetch_batches<F, Fut>(
    window: &ContextWindow,
    fetch_batch: &F,
) -> Result<Batched, SyncError>
where
    F: Fn(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<EntityIngest>, SyncError>>,
{
    let mut entities = Vec::new();
    let mut denials: Vec<(String, u16)> = Vec::new();
    for batch in window.batches() {
        let err = match fetch_batch(batch.clone()).await {
            Ok(items) => {
                entities.extend(items);
                continue;
            }
            Err(err) => err,
        };
        match batch_action(&err) {
            // Auth and throttling are about the caller, not the context.
            BatchAction::Abort => return Err(err),
            BatchAction::Propagate => return Ok(Batched::Propagate(err)),
            BatchAction::Isolate => {}
        }
        // One request per context in the batch (§12.6).
        for context in batch {
            let err = match fetch_batch(vec![context.clone()]).await {
                Ok(items) => {
                    entities.extend(items);
                    continue;
                }
                Err(err) => err,
            };
            match batch_action(&err) {
                BatchAction::Abort => return Err(err),
                BatchAction::Propagate => return Ok(Batched::Propagate(err)),
                BatchAction::Isolate => denials.push((context, isolated_status(&err))),
            }
        }
    }
    Ok(Batched::Complete { entities, denials })
}

/// Store a complete refresh that carries per-context denials.
#[allow(clippy::too_many_arguments)]
async fn ingest_with_denials<D: Dataset + Clone + Send + Sync + 'static>(
    client: &Client,
    store: &Store,
    dataset: &D,
    now: Timestamp,
    before: u64,
    epoch_seen: i64,
    bundle: FetchBundle,
    window: &ContextWindow,
    error: &str,
) -> Result<RefreshOutcome, SyncError> {
    let count = bundle
        .pages
        .iter()
        .flat_map(|page| &page.entities)
        .map(|entity| &entity.entity_key)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    let requests = u32::try_from(client.telemetry().api.saturating_sub(before)).unwrap_or(u32::MAX);
    let meta = window_meta(window);
    let pages = bundle.pages;
    let dataset_for_ingest = dataset.clone();
    let error_for_ingest = error.to_owned();
    store
        .call(move |conns| {
            dataset_for_ingest
                .ingest(
                    &pages,
                    &IngestOpts {
                        epoch_seen,
                        complete: true,
                        stale: false,
                        error: Some(error_for_ingest.as_str()),
                        window: Some((meta.start, meta.end)),
                        contexts: Some(meta.contexts.as_str()),
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
        error: Some(error.to_owned()),
    })
}

#[allow(clippy::too_many_arguments)]
async fn propagate<D: Dataset + Clone + Send + Sync + 'static>(
    client: &Client,
    store: &Store,
    dataset: &D,
    before: u64,
    epoch_seen: i64,
    lookup: LookupResult,
    err: SyncError,
    window: &ContextWindow,
) -> Result<BatchOutcome, SyncError> {
    let outcome = mark_stale_or_serve(
        client,
        store,
        dataset,
        before,
        epoch_seen,
        lookup,
        err,
        Some(window_meta(window)),
    )
    .await?;
    Ok(cached(outcome))
}

fn window_meta(window: &ContextWindow) -> WindowMeta {
    WindowMeta {
        start: window.window().start_timestamp(),
        end: window.window().end_timestamp(),
        contexts: window.context_hash().to_owned(),
    }
}

fn cached(outcome: RefreshOutcome) -> BatchOutcome {
    let denials = decode_denials(outcome.error.as_deref());
    BatchOutcome { outcome, denials }
}

fn decode_denials(error: Option<&str>) -> Vec<ContextDenial> {
    error
        .map(parse_context_denials)
        .unwrap_or_default()
        .into_iter()
        .map(|(context, http_status)| ContextDenial {
            context,
            http_status,
        })
        .collect()
}

/// What a batch failure means for the refresh.
enum BatchAction {
    /// Auth or throttling: the whole refresh stops.
    Abort,
    /// A per-context denial: retry one context at a time, then record it.
    Isolate,
    /// Anything else (network, decode): mark stale or serve the cached row.
    Propagate,
}

fn batch_action(err: &SyncError) -> BatchAction {
    let SyncError::Api(api) = err else {
        return BatchAction::Propagate;
    };
    match classify_listing_denial(api) {
        DenialAction::Auth | DenialAction::Throttle => BatchAction::Abort,
        DenialAction::RecordDenial { .. } => BatchAction::Isolate,
        DenialAction::Propagate => BatchAction::Propagate,
    }
}

fn isolated_status(err: &SyncError) -> u16 {
    match err {
        SyncError::Api(api) => match classify_listing_denial(api) {
            DenialAction::RecordDenial { status } => status,
            _ => 403,
        },
        _ => 403,
    }
}

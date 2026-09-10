//! Adaptive request throttle (SPEC §11).

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{Notify, Semaphore};
use tokio::time::{Instant, sleep};

/// Seconds of header silence that reset the estimate (SPEC §11).
const SILENCE_SECS: u64 = 60;

/// How long a published cooldown stays in force for another process.
///
/// SPEC §11 gives cooldown a five-second probe wait and no explicit lifetime.
/// A shared flag needs one, or a process that dies in cooldown would hold every
/// other process there forever. The owner republishes the flag on each of its
/// own admissions, so a live cooldown never lapses, and a vanished owner's
/// cooldown expires one probe wait after its last write.
const COOLDOWN_MILLIS: i64 = 5_000;

/// Lane for admission control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Canvas `/api/v1` requests.
    Api,
    /// Storage upload/download transfers.
    Storage,
}

/// A held lane slot. Dropping it releases the slot.
///
/// The concrete guard is opaque on purpose: an in-process implementation hands
/// back a semaphore permit, and a cross-process one hands back a locked file.
pub type LaneSlot = Box<dyn Any + Send>;

/// Slot acquisition seam (SPEC §11 concurrency caps).
///
/// The default implementation is one semaphore per lane, which is what the
/// governor has always used. `canvas-core` supplies a cross-process
/// implementation; this crate stays disk-free (§13).
pub trait Permits: Send + Sync + 'static {
    /// Acquire one slot on `lane`, waiting until one is free.
    fn acquire(&self, lane: Lane) -> Pin<Box<dyn Future<Output = LaneSlot> + Send + '_>>;
}

/// The governor values that cross process boundaries (SPEC §11).
///
/// Times are Unix milliseconds so the row survives a process restart, unlike
/// the monotonic instants the in-process state keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GovernorSnapshot {
    /// Remaining-cost estimate.
    pub estimate: f64,
    /// Highest issue number whose sample was applied.
    pub watermark: u64,
    /// Cooldown deadline, or `None` when no process is in cooldown.
    pub cooldown_until: Option<i64>,
    /// Observed refill per second, never above 10.
    pub refill: f64,
    /// When the row was last written.
    pub updated_at: i64,
}

/// Load/store hook for the shared governor row.
///
/// The default implementation keeps everything in this process, which is what
/// the governor has always done. `canvas-core` stores the row in
/// `state.sqlite`. An implementation must not wait on the network inside its
/// transaction: `update` is handed a pure merge closure and nothing else.
pub trait GovernorState: Send + Sync + 'static {
    /// Read the shared row without taking a write transaction.
    fn load(&self) -> Option<GovernorSnapshot>;

    /// Read, merge, and write the shared row under one exclusive transaction.
    ///
    /// `merge` receives the stored row, if any, and returns the row to store.
    /// Returning `None` leaves the row unchanged.
    fn update(&self, merge: &mut dyn FnMut(Option<GovernorSnapshot>) -> Option<GovernorSnapshot>);
}

/// The default lane slots: one semaphore per lane, this process only.
struct InProcessPermits {
    api: Arc<Semaphore>,
    storage: Arc<Semaphore>,
}

impl Permits for InProcessPermits {
    fn acquire(&self, lane: Lane) -> Pin<Box<dyn Future<Output = LaneSlot> + Send + '_>> {
        let sem = match lane {
            Lane::Api => self.api.clone(),
            Lane::Storage => self.storage.clone(),
        };
        Box::pin(async move {
            let permit = sem.acquire_owned().await.expect("lane semaphore");
            Box::new(permit) as LaneSlot
        })
    }
}

/// Optional cross-process seams for [`Governor::with_seams`].
#[derive(Default, Clone)]
pub struct Seams {
    /// Slot acquisition; `None` keeps the in-process semaphores.
    pub permits: Option<Arc<dyn Permits>>,
    /// Shared governor row; `None` keeps the values in this process.
    pub state: Option<Arc<dyn GovernorState>>,
}

impl fmt::Debug for Seams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Seams")
            .field("permits", &self.permits.is_some())
            .field("state", &self.state.is_some())
            .finish()
    }
}

/// Governor tuning knobs.
#[derive(Debug, Clone)]
pub struct GovernorConfig {
    /// Max concurrent API requests (default 4, clamped to 8).
    pub api_concurrency: usize,
    /// Max concurrent storage transfers (default 4).
    pub storage_concurrency: usize,
    /// Full remaining estimate after reset (default 700).
    pub full_remaining: f64,
    /// Apply ±25% jitter to retry delays (default true; false for exact tests).
    pub jitter: bool,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            api_concurrency: 4,
            storage_concurrency: 4,
            full_remaining: 700.0,
            jitter: true,
        }
    }
}

impl GovernorConfig {
    fn clamped_api_concurrency(&self) -> usize {
        self.api_concurrency.clamp(1, 8)
    }
}

/// Cumulative request counters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Telemetry {
    /// Admitted API requests.
    pub api: u64,
    /// Admitted storage transfers.
    pub storage: u64,
    /// Sum of observed `X-Request-Cost` values.
    pub cost: Option<f64>,
}

/// RAII admission permit. Dropping releases the lane slot.
pub struct AdmissionPermit {
    issue: u64,
    lane: Lane,
    route_key: String,
    inner: Arc<Inner>,
    _permit: LaneSlot,
}

impl fmt::Debug for AdmissionPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdmissionPermit")
            .field("issue", &self.issue)
            .field("lane", &self.lane)
            .field("route_key", &self.route_key)
            .finish_non_exhaustive()
    }
}

impl AdmissionPermit {
    /// Issue sequence number for this admission.
    #[must_use]
    pub fn issue(&self) -> u64 {
        self.issue
    }

    /// Lane that was admitted.
    #[must_use]
    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// Route key used for cost pre-charge.
    #[must_use]
    pub fn route_key(&self) -> &str {
        &self.route_key
    }
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.inner.on_release(self.issue);
    }
}

/// Rate-limit governor with observation watermark and cooldown.
#[derive(Clone)]
pub struct Governor {
    inner: Arc<Inner>,
}

impl fmt::Debug for Governor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Governor")
            .field("api_concurrency", &self.inner.config.api_concurrency)
            .field(
                "storage_concurrency",
                &self.inner.config.storage_concurrency,
            )
            .finish_non_exhaustive()
    }
}

struct Inner {
    config: GovernorConfig,
    permits: Arc<dyn Permits>,
    shared: Option<Arc<dyn GovernorState>>,
    admission: tokio::sync::Mutex<()>,
    released: Notify,
    state: Mutex<State>,
    next_issue: AtomicU64,
    api_count: AtomicU64,
    storage_count: AtomicU64,
}

struct State {
    estimate: f64,
    estimate_at: Instant,
    refill: f64,
    watermark: u64,
    in_flight: u64,
    in_cooldown: bool,
    last_header_at: Option<Instant>,
    /// When a shared row was last read, when there is one.
    ///
    /// With a shared row the header-silence window belongs to the row, not to
    /// this process: another process may be making every request and seeing
    /// every header. Without this, a `watch` that had been quiet for a minute
    /// would reset its own estimate to full on its next admission and drop a
    /// cooldown a live process had just published.
    shared_seen_at: Option<Instant>,
    started_at: Instant,
    route_costs: HashMap<String, f64>,
    cost_sum: f64,
    cost_seen: bool,
    outstanding: HashMap<u64, Outstanding>,
    last_applied: Option<AppliedSample>,
}

struct Outstanding {
    admit_at: Instant,
    route_key: String,
}

struct AppliedSample {
    issue: u64,
    remaining: f64,
    completed_at: Instant,
}

impl Governor {
    /// Build a governor from `config`.
    #[must_use]
    pub fn new(config: GovernorConfig) -> Self {
        Self::with_seams(config, &Seams::default())
    }

    /// Build a governor with optional cross-process seams.
    ///
    /// With both seams unset this is exactly [`Governor::new`].
    #[must_use]
    pub fn with_seams(config: GovernorConfig, seams: &Seams) -> Self {
        let api_n = config.clamped_api_concurrency();
        let storage_n = config.storage_concurrency.max(1);
        let permits = seams.permits.clone().unwrap_or_else(|| {
            Arc::new(InProcessPermits {
                api: Arc::new(Semaphore::new(api_n)),
                storage: Arc::new(Semaphore::new(storage_n)),
            }) as Arc<dyn Permits>
        });
        let now = Instant::now();
        let full = config.full_remaining;
        Self {
            inner: Arc::new(Inner {
                permits,
                shared: seams.state.clone(),
                admission: tokio::sync::Mutex::new(()),
                released: Notify::new(),
                config,
                state: Mutex::new(State {
                    estimate: full,
                    estimate_at: now,
                    refill: 0.0,
                    watermark: 0,
                    in_flight: 0,
                    in_cooldown: false,
                    last_header_at: None,
                    shared_seen_at: None,
                    started_at: now,
                    route_costs: HashMap::new(),
                    cost_sum: 0.0,
                    cost_seen: false,
                    outstanding: HashMap::new(),
                    last_applied: None,
                }),
                next_issue: AtomicU64::new(1),
                api_count: AtomicU64::new(0),
                storage_count: AtomicU64::new(0),
            }),
        }
    }

    /// Admit a request on `lane` for `route_key` (pre-charges expected cost).
    pub async fn admit(&self, lane: Lane, route_key: &str) -> AdmissionPermit {
        // Wait for the lane before serializing admission so a full API lane
        // does not occupy the admission lock needed by storage (and vice versa).
        let permit = self.inner.permits.acquire(lane).await;
        let _admission = self.inner.admission.lock().await;
        loop {
            // The shared row is read before admission, never inside a wait.
            self.inner.adopt_shared();
            let (cooldown, in_flight, wait) = {
                let mut state = self.inner.state.lock().expect("governor state");
                Inner::reset_silence_locked(&self.inner.config, &mut state);
                Inner::grow_locked(&mut state);
                state.in_cooldown |= state.estimate < 150.0;
                let wait = if state.refill > 0.0 {
                    Duration::from_secs_f64(
                        ((350.0 - state.estimate).max(0.0) / state.refill).min(5.0),
                    )
                } else {
                    Duration::from_secs(5)
                };
                (state.in_cooldown, state.in_flight, wait)
            };
            if cooldown && in_flight > 0 {
                // Existing normal admissions must drain before the single probe.
                // notify_one retains a permit if release races this await.
                self.inner.released.notified().await;
                continue;
            }
            if cooldown {
                // Serialize the timer as well as the probe: queued callers must
                // not reuse a timer that elapsed while another request ran.
                sleep(wait).await;
            }
            let cost = {
                let mut state = self.inner.state.lock().expect("governor state");
                Inner::reset_silence_locked(&self.inner.config, &mut state);
                Inner::grow_locked(&mut state);
                state.route_costs.get(route_key).copied().unwrap_or(1.0)
            };
            // Pre-charge the cost, on the shared row when there is one.
            self.inner.charge(cost);
            let mut state = self.inner.state.lock().expect("governor state");
            let issue = self.inner.next_issue.fetch_add(1, Ordering::Relaxed);
            state.outstanding.insert(
                issue,
                Outstanding {
                    admit_at: Instant::now(),
                    route_key: route_key.to_owned(),
                },
            );
            state.in_flight += 1;
            match lane {
                Lane::Api => {
                    self.inner.api_count.fetch_add(1, Ordering::Relaxed);
                }
                Lane::Storage => {
                    self.inner.storage_count.fetch_add(1, Ordering::Relaxed);
                }
            }
            return AdmissionPermit {
                issue,
                lane,
                route_key: route_key.to_owned(),
                inner: self.inner.clone(),
                _permit: permit,
            };
        }
    }

    /// Apply a rate-limit observation for `issue`.
    pub fn observe(&self, issue: u64, remaining: f64, cost: Option<f64>) {
        if !remaining.is_finite() || remaining < 0.0 {
            if let Some(cost) = cost {
                self.observe_cost_only(issue, cost);
            }
            return;
        }
        let cost = cost.filter(|c| c.is_finite() && *c >= 0.0);
        {
            let mut state = self.inner.state.lock().expect("governor state");
            Inner::apply_observation(&mut state, issue, remaining, cost);
        }
        // §11 values are updated after each response, under one transaction.
        self.inner.publish();
    }

    /// Record cost telemetry without a remaining sample.
    pub fn observe_cost_only(&self, issue: u64, cost: f64) {
        if !cost.is_finite() || cost < 0.0 {
            return;
        }
        let mut state = self.inner.state.lock().expect("governor state");
        state.last_header_at = Some(Instant::now());
        state.cost_sum += cost;
        state.cost_seen = true;
        if let Some(o) = state.outstanding.get(&issue) {
            let key = o.route_key.clone();
            state.route_costs.insert(key, cost);
        }
    }

    /// The values this governor would publish to a shared row (tests).
    #[must_use]
    pub fn snapshot(&self) -> GovernorSnapshot {
        let mut state = self.inner.state.lock().expect("governor state");
        Inner::grow_locked(&mut state);
        Inner::snapshot_locked(&state, unix_millis())
    }

    /// Current telemetry counters.
    #[must_use]
    pub fn telemetry(&self) -> Telemetry {
        let state = self.inner.state.lock().expect("governor state");
        Telemetry {
            api: self.inner.api_count.load(Ordering::Relaxed),
            storage: self.inner.storage_count.load(Ordering::Relaxed),
            cost: state.cost_seen.then_some(state.cost_sum),
        }
    }

    /// Current remaining estimate (tests).
    #[must_use]
    pub fn estimate(&self) -> f64 {
        let mut state = self.inner.state.lock().expect("governor state");
        Inner::grow_locked(&mut state);
        state.estimate
    }

    /// Highest applied issue watermark (tests).
    #[must_use]
    pub fn watermark(&self) -> u64 {
        self.inner.state.lock().expect("governor state").watermark
    }

    /// In-flight admissions (tests).
    #[must_use]
    pub fn in_flight(&self) -> u64 {
        self.inner.state.lock().expect("governor state").in_flight
    }

    /// Override the estimate (tests).
    pub fn set_estimate_for_test(&self, estimate: f64) {
        let mut state = self.inner.state.lock().expect("governor state");
        state.estimate = estimate;
        state.estimate_at = Instant::now();
        state.in_cooldown = estimate < 150.0;
    }

    /// Apply an observation without an admission permit (tests).
    pub fn apply_observation_for_test(&self, issue: u64, remaining: f64, cost: Option<f64>) {
        self.observe(issue, remaining, cost);
    }

    /// Merge the shared row into local state without admitting (tests).
    pub fn adopt_shared_for_test(&self) {
        self.inner.adopt_shared();
    }

    /// Whether jitter is enabled on retry delays.
    #[must_use]
    pub fn jitter(&self) -> bool {
        self.inner.config.jitter
    }
}

impl Inner {
    /// Merge the shared row into local state before an admission decision.
    fn adopt_shared(&self) {
        let Some(shared) = &self.shared else { return };
        let Some(row) = shared.load() else { return };
        let mut state = self.state.lock().expect("governor state");
        Self::merge_shared_locked(&self.config, &mut state, row, unix_millis());
    }

    /// Pre-charge `cost`, merging and republishing the shared row when there
    /// is one. The closure runs inside the implementation's transaction and
    /// touches nothing but memory.
    fn charge(&self, cost: f64) {
        let Some(shared) = &self.shared else {
            let mut state = self.state.lock().expect("governor state");
            Self::charge_locked(&mut state, cost);
            return;
        };
        let now_ms = unix_millis();
        shared.update(&mut |stored| {
            let mut state = self.state.lock().expect("governor state");
            if let Some(row) = stored {
                Self::merge_shared_locked(&self.config, &mut state, row, now_ms);
            }
            Self::charge_locked(&mut state, cost);
            Some(Self::snapshot_locked(&state, now_ms))
        });
    }

    /// Republish the local §11 values onto the shared row after a response.
    fn publish(&self) {
        let Some(shared) = &self.shared else { return };
        let now_ms = unix_millis();
        shared.update(&mut |stored| {
            let mut state = self.state.lock().expect("governor state");
            if let Some(row) = stored {
                Self::merge_shared_locked(&self.config, &mut state, row, now_ms);
            }
            Some(Self::snapshot_locked(&state, now_ms))
        });
    }

    fn charge_locked(state: &mut State, cost: f64) {
        state.estimate -= cost;
        state.in_cooldown |= state.estimate < 150.0;
        state.estimate_at = Instant::now();
    }

    fn snapshot_locked(state: &State, now_ms: i64) -> GovernorSnapshot {
        GovernorSnapshot {
            estimate: state.estimate,
            watermark: state.watermark,
            cooldown_until: state
                .in_cooldown
                .then(|| now_ms.saturating_add(COOLDOWN_MILLIS)),
            refill: state.refill,
            updated_at: now_ms,
        }
    }

    /// Apply the §11 rules to a shared row.
    ///
    /// A lower estimate always applies; a higher one only above the watermark.
    /// Refill is never assumed above 10/s. Cooldown is shared. A row nobody has
    /// written for the header-silence window resets exactly as §11 says, and
    /// nothing more: it is the same reset [`Inner::reset_silence_locked`] does,
    /// so a vanished owner never hands anyone an invented full bucket while a
    /// request of its own is still in flight here.
    fn merge_shared_locked(
        config: &GovernorConfig,
        state: &mut State,
        row: GovernorSnapshot,
        now_ms: i64,
    ) {
        state.shared_seen_at = Some(Instant::now());
        let silent_for = now_ms.saturating_sub(row.updated_at);
        if silent_for >= i64::try_from(SILENCE_SECS).unwrap_or(60) * 1_000 {
            if state.in_flight == 0 {
                Self::reset_locked(config, state);
            }
            return;
        }
        if row.estimate < state.estimate || row.watermark > state.watermark {
            state.estimate = row.estimate;
            state.estimate_at = Instant::now();
        }
        if row.watermark > state.watermark {
            state.watermark = row.watermark;
        }
        if row.refill > 0.0 {
            state.refill = row.refill.min(10.0);
        }
        if row.cooldown_until.is_some_and(|until| until > now_ms) {
            state.in_cooldown = true;
        }
    }

    fn reset_locked(config: &GovernorConfig, state: &mut State) {
        state.estimate = config.full_remaining;
        state.estimate_at = Instant::now();
        state.watermark = 0;
        state.in_cooldown = false;
        state.refill = 0.0;
        state.last_applied = None;
        state.last_header_at = None;
        state.shared_seen_at = None;
        state.started_at = Instant::now();
    }

    fn reset_silence_locked(config: &GovernorConfig, state: &mut State) {
        if state.in_flight != 0 {
            return;
        }
        // A shared row that was written inside the silence window means some
        // process is live: `merge_shared_locked` owns the reset for it, and it
        // has already run for this admission. Resetting here as well would
        // hand this process an invented full bucket and drop a shared cooldown
        // (SPEC §11, REPORT §3.6).
        if state
            .shared_seen_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(SILENCE_SECS))
        {
            return;
        }
        let silent = match state.last_header_at {
            Some(t) => t.elapsed() >= Duration::from_secs(SILENCE_SECS),
            None => state.started_at.elapsed() >= Duration::from_secs(SILENCE_SECS),
        };
        if silent {
            Self::reset_locked(config, state);
        }
    }

    fn grow_locked(state: &mut State) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(state.estimate_at);
        if state.refill > 0.0 && elapsed > Duration::ZERO {
            state.estimate += state.refill * elapsed.as_secs_f64();
            state.estimate_at = now;
        }
    }

    fn apply_observation(state: &mut State, issue: u64, remaining: f64, cost: Option<f64>) {
        Self::grow_locked(state);
        let now = Instant::now();
        state.last_header_at = Some(now);

        let outstanding = state.outstanding.get(&issue);

        if let Some(c) = cost {
            state.cost_sum += c;
            state.cost_seen = true;
            if let Some(o) = outstanding {
                state.route_costs.insert(o.route_key.clone(), c);
            }
        }

        let current = state.estimate;
        let apply = if remaining < current {
            true
        } else {
            // Higher or equal samples apply only when newer than the watermark.
            issue > state.watermark
        };

        if apply {
            if let Some(prev) = &state.last_applied {
                let non_overlap = !state.outstanding.contains_key(&prev.issue)
                    && outstanding.is_some_and(|o| o.admit_at >= prev.completed_at);
                if non_overlap && remaining > prev.remaining {
                    let dt = now
                        .saturating_duration_since(prev.completed_at)
                        .as_secs_f64();
                    if dt > 0.0 {
                        let observed = (remaining - prev.remaining) / dt;
                        if observed > 0.0 {
                            state.refill = observed.min(10.0);
                        }
                    }
                }
            }

            state.estimate = remaining;
            state.estimate_at = now;
            if issue > state.watermark {
                state.watermark = issue;
            }
            state.last_applied = Some(AppliedSample {
                issue,
                remaining,
                completed_at: now,
            });

            if remaining >= 300.0 {
                state.in_cooldown = false;
            } else if remaining < 150.0 {
                state.in_cooldown = true;
            }
        }
    }

    fn on_release(&self, issue: u64) {
        let mut state = self.state.lock().expect("governor state");
        state.in_flight = state.in_flight.saturating_sub(1);
        state.outstanding.remove(&issue);
        if let Some(sample) = state.last_applied.as_mut()
            && sample.issue == issue
        {
            sample.completed_at = Instant::now();
        }
        self.released.notify_one();
    }
}

/// Delay before retry attempt `attempt` (0 = first retry after the initial try).
///
/// Delays are 1, 2, 4, 8 seconds for attempts 0..3 (attempt ≥ 3 clamps to 8),
/// with optional ±25% jitter, unless `retry_after` is set.
pub async fn retry_delays(attempt: u32, retry_after: Option<Duration>, jitter: bool) -> Duration {
    if let Some(d) = retry_after {
        sleep(d).await;
        return d;
    }
    let base_secs = 1u64 << attempt.min(3);
    let delay = if jitter {
        let base = Duration::from_secs(base_secs);
        let millis = base.as_secs_f64() * 1000.0;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let factor = 0.75 + f64::from(nanos % 501) / 1000.0;
        Duration::from_secs_f64((millis * factor) / 1000.0)
    } else {
        Duration::from_secs(base_secs)
    };
    sleep(delay).await;
    delay
}

/// Wall-clock milliseconds since the Unix epoch.
///
/// The shared row outlives a process, so it cannot use a monotonic instant.
#[must_use]
pub fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

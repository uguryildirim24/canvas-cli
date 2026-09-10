//! Adaptive request throttle (SPEC §11).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, sleep};

/// Lane for admission control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Canvas `/api/v1` requests.
    Api,
    /// Storage upload/download transfers.
    Storage,
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
    _permit: OwnedSemaphorePermit,
    _cooldown: Option<OwnedSemaphorePermit>,
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
    api: Arc<Semaphore>,
    storage: Arc<Semaphore>,
    cooldown_gate: Arc<Semaphore>,
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
    remaining: f64,
    completed_at: Instant,
}

impl Governor {
    /// Build a governor from `config`.
    #[must_use]
    pub fn new(config: GovernorConfig) -> Self {
        let api_n = config.clamped_api_concurrency();
        let storage_n = config.storage_concurrency.max(1);
        let now = Instant::now();
        let full = config.full_remaining;
        Self {
            inner: Arc::new(Inner {
                api: Arc::new(Semaphore::new(api_n)),
                storage: Arc::new(Semaphore::new(storage_n)),
                cooldown_gate: Arc::new(Semaphore::new(1)),
                config,
                state: Mutex::new(State {
                    estimate: full,
                    estimate_at: now,
                    refill: 0.0,
                    watermark: 0,
                    in_flight: 0,
                    in_cooldown: false,
                    last_header_at: None,
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
    #[allow(clippy::too_many_lines)]
    pub async fn admit(&self, lane: Lane, route_key: &str) -> AdmissionPermit {
        loop {
            {
                let mut state = self.inner.state.lock().expect("governor state");
                Inner::reset_silence_locked(&self.inner.config, &mut state);
                Inner::grow_locked(&mut state);
            }

            let (wait_target, in_cooldown) = {
                let state = self.inner.state.lock().expect("governor state");
                let in_cd = state.in_cooldown || state.estimate < 150.0;
                if !in_cd {
                    (Duration::ZERO, false)
                } else if state.refill <= 0.0 {
                    (Duration::from_secs(5), true)
                } else {
                    let need = (350.0 - state.estimate).max(0.0);
                    let secs = need / state.refill;
                    if secs > 5.0 || need == 0.0 && state.estimate < 350.0 {
                        // wait>5s → probe; already at/above 350 → admit
                        if state.estimate >= 350.0 {
                            (Duration::ZERO, true)
                        } else if secs > 5.0 {
                            (Duration::from_secs(5), true)
                        } else {
                            (Duration::from_secs_f64(secs), true)
                        }
                    } else if state.estimate >= 350.0 {
                        (Duration::ZERO, true)
                    } else {
                        (Duration::from_secs_f64(secs), true)
                    }
                }
            };

            if wait_target > Duration::ZERO {
                sleep(wait_target).await;
                let mut state = self.inner.state.lock().expect("governor state");
                Inner::grow_locked(&mut state);
            }

            let cooldown_permit = if in_cooldown {
                Some(
                    self.inner
                        .cooldown_gate
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("cooldown semaphore"),
                )
            } else {
                None
            };

            let sem = match lane {
                Lane::Api => self.inner.api.clone(),
                Lane::Storage => self.inner.storage.clone(),
            };
            let permit = sem.acquire_owned().await.expect("lane semaphore");

            let issue = {
                let mut state = self.inner.state.lock().expect("governor state");
                Inner::reset_silence_locked(&self.inner.config, &mut state);
                Inner::grow_locked(&mut state);

                let now_cd = state.in_cooldown || state.estimate < 150.0;
                if now_cd && cooldown_permit.is_none() {
                    drop(permit);
                    continue;
                }
                if !now_cd {
                    // Drop unused cooldown permit if we left cooldown while waiting.
                    drop(cooldown_permit);
                    let cost = state.route_costs.get(route_key).copied().unwrap_or(1.0);
                    state.estimate -= cost;
                    if state.estimate < 150.0 {
                        state.in_cooldown = true;
                    }
                    state.estimate_at = Instant::now();
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
                        _cooldown: None,
                    };
                }

                let cost = state.route_costs.get(route_key).copied().unwrap_or(1.0);
                state.estimate -= cost;
                if state.estimate < 150.0 {
                    state.in_cooldown = true;
                }
                state.estimate_at = Instant::now();
                let issue = self.inner.next_issue.fetch_add(1, Ordering::Relaxed);
                state.outstanding.insert(
                    issue,
                    Outstanding {
                        admit_at: Instant::now(),
                        route_key: route_key.to_owned(),
                    },
                );
                state.in_flight += 1;
                issue
            };

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
                _cooldown: cooldown_permit,
            };
        }
    }

    /// Apply a rate-limit observation for `issue`.
    pub fn observe(&self, issue: u64, remaining: f64, cost: Option<f64>) {
        let mut state = self.inner.state.lock().expect("governor state");
        Inner::apply_observation(&mut state, issue, remaining, cost);
    }

    /// Record cost telemetry without a remaining sample.
    pub fn observe_cost_only(&self, issue: u64, cost: f64) {
        let mut state = self.inner.state.lock().expect("governor state");
        state.last_header_at = Some(Instant::now());
        state.cost_sum += cost;
        state.cost_seen = true;
        if let Some(o) = state.outstanding.get(&issue) {
            let key = o.route_key.clone();
            state.route_costs.insert(key, cost);
        }
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

    /// Whether jitter is enabled on retry delays.
    #[must_use]
    pub fn jitter(&self) -> bool {
        self.inner.config.jitter
    }
}

impl Inner {
    fn reset_silence_locked(config: &GovernorConfig, state: &mut State) {
        if state.in_flight != 0 {
            return;
        }
        let silent = match state.last_header_at {
            Some(t) => t.elapsed() >= Duration::from_secs(60),
            None => state.started_at.elapsed() >= Duration::from_secs(60),
        };
        if silent {
            state.estimate = config.full_remaining;
            state.estimate_at = Instant::now();
            state.watermark = 0;
            state.in_cooldown = false;
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

        let outstanding = state.outstanding.remove(&issue);

        if let Some(c) = cost {
            state.cost_sum += c;
            state.cost_seen = true;
            if let Some(ref o) = outstanding {
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
                let non_overlap = outstanding
                    .as_ref()
                    .is_none_or(|o| o.admit_at >= prev.completed_at);
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
        let nanos = Instant::now().elapsed().subsec_nanos();
        let factor = 0.75 + f64::from(nanos % 501) / 1000.0;
        Duration::from_secs_f64((millis * factor) / 1000.0)
    } else {
        Duration::from_secs(base_secs)
    };
    sleep(delay).await;
    delay
}

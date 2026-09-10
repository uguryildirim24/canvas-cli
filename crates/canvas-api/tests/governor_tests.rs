//! Governor and retry delay tests.

mod common;

use std::time::Duration;

use canvas_api::governor::{Governor, Lane, retry_delays};
use canvas_api::{Error, GovernorConfig};
use serde::Deserialize;
use serde_json::json;
use tokio::time::{Instant, advance, pause};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Deserialize)]
struct OkBody {
    ok: bool,
}

fn gov_config() -> GovernorConfig {
    GovernorConfig {
        jitter: false,
        full_remaining: 700.0,
        ..Default::default()
    }
}

async fn wall_sleep(ms: u64) {
    let _ =
        tokio::task::spawn_blocking(move || std::thread::sleep(Duration::from_millis(ms))).await;
}

async fn wait_for_received(server: &MockServer, n: usize) {
    for _ in 0..10_000 {
        if server.received_requests().await.map_or(0, |r| r.len()) >= n {
            return;
        }
        wall_sleep(2).await;
    }
    panic!("timed out waiting for {n} received request(s)");
}

/// Advance virtual time until `handle` completes.
///
/// reqwest + `start_paused` auto-advance races with request timeouts, so HTTP
/// retry tests use [`pause`] and drive time explicitly.
async fn drive_while_advancing<T>(handle: tokio::task::JoinHandle<T>, max: Duration) -> T {
    let start = Instant::now();
    while !handle.is_finished() {
        advance(Duration::from_millis(50)).await;
        tokio::task::yield_now().await;
        assert!(
            start.elapsed() <= max,
            "driven request did not finish within {max:?}"
        );
    }
    handle.await.expect("join driven request")
}

#[tokio::test(start_paused = true)]
async fn delayed_high_sample_discarded() {
    let gov = Governor::new(gov_config());
    let p1 = gov.admit(Lane::Api, "r").await;
    let p2 = gov.admit(Lane::Api, "r").await;
    let issue1 = p1.issue();
    let issue2 = p2.issue();

    gov.observe(issue2, 100.0, Some(1.0));
    assert!((gov.estimate() - 100.0).abs() < 1e-6);
    assert_eq!(gov.watermark(), issue2);

    gov.observe(issue1, 500.0, Some(1.0));
    assert!(
        (gov.estimate() - 100.0).abs() < 1e-6,
        "stale high sample must not raise estimate; got {}",
        gov.estimate()
    );
    assert_eq!(gov.watermark(), issue2);

    drop(p1);
    drop(p2);
}

#[tokio::test(start_paused = true)]
async fn header_silence_resets_when_nothing_in_flight() {
    let gov = Governor::new(gov_config());
    gov.set_estimate_for_test(200.0);
    assert!((gov.estimate() - 200.0).abs() < 1e-6);

    advance(Duration::from_secs(60)).await;

    let permit = gov.admit(Lane::Api, "r").await;
    drop(permit);

    assert!(
        (gov.estimate() - 699.0).abs() < 1e-6,
        "expected ~699 after silence reset + cost; got {}",
        gov.estimate()
    );
}

#[tokio::test(start_paused = true)]
async fn header_silence_does_not_reset_while_in_flight() {
    let gov = Governor::new(gov_config());
    gov.set_estimate_for_test(200.0);
    let permit = gov.admit(Lane::Api, "r").await;
    let after_admit = gov.estimate();
    assert!((after_admit - 199.0).abs() < 1e-6);
    assert_eq!(gov.in_flight(), 1);

    advance(Duration::from_secs(60)).await;

    assert!(
        (gov.estimate() - after_admit).abs() < 1e-6,
        "estimate must not reset to full while a permit is held; got {}",
        gov.estimate()
    );
    assert!(gov.estimate() < 300.0);

    drop(permit);
}

#[tokio::test(start_paused = true)]
async fn cooldown_recovery_under_climbing_samples() {
    let gov = Governor::new(gov_config());
    gov.set_estimate_for_test(100.0);
    assert!(gov.estimate() < 150.0, "in cooldown via low estimate");

    gov.apply_observation_for_test(1, 200.0, Some(1.0));
    assert!((gov.estimate() - 200.0).abs() < 1e-6);
    assert!(gov.estimate() < 300.0);

    gov.apply_observation_for_test(2, 320.0, Some(1.0));
    assert!(
        gov.estimate() >= 300.0,
        "sample >= 300 should raise estimate out of cooldown band; got {}",
        gov.estimate()
    );
}

#[tokio::test]
async fn retry_after_header_pauses_about_three_seconds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/x"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client_with_governor(&server, gov_config());
    pause();

    let handle = tokio::spawn(async move { client.get::<OkBody>("/api/v1/x").await });

    // Reach the first 429 without advancing (real I/O via wall_sleep yields).
    wait_for_received(&server, 1).await;
    wall_sleep(50).await;

    let start = Instant::now();
    let body = drive_while_advancing(handle, Duration::from_secs(10))
        .await
        .expect("get after retry");
    assert!(body.ok);
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_secs(3) && elapsed < Duration::from_secs(8),
        "expected ~3s Retry-After delay, got {elapsed:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn exponential_retries_delay_fifteen_seconds_before_success() {
    let server = MockServer::start().await;

    for _ in 0..4 {
        Mock::given(method("GET"))
            .and(path("/api/v1/x"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/api/v1/x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let client = common::test_client_with_governor(&server, gov_config());
    pause();

    let handle = tokio::spawn(async move { client.get::<OkBody>("/api/v1/x").await });

    wait_for_received(&server, 1).await;
    wall_sleep(50).await;

    let start = Instant::now();
    let body = drive_while_advancing(handle, Duration::from_secs(40))
        .await
        .expect("get after retries");
    assert!(body.ok);
    let elapsed = start.elapsed();
    // Delays 1+2+4+8=15s; extra virtual time covers inter-request scheduling.
    assert!(
        elapsed >= Duration::from_secs(15) && elapsed < Duration::from_secs(35),
        "expected >=15s exponential delays, got {elapsed:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn exhausted_retries_return_rate_limited() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/x"))
        .respond_with(ResponseTemplate::new(429))
        .expect(5)
        .mount(&server)
        .await;

    let client = common::test_client_with_governor(&server, gov_config());
    pause();

    let handle = tokio::spawn(async move { client.get::<serde_json::Value>("/api/v1/x").await });

    wait_for_received(&server, 1).await;
    wall_sleep(50).await;

    let err = drive_while_advancing(handle, Duration::from_secs(40))
        .await
        .expect_err("retries exhausted");
    assert!(matches!(err, Error::RateLimited), "{err:?}");
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test(start_paused = true)]
async fn retry_delays_without_retry_after_are_powers_of_two() {
    let d0 = retry_delays(0, None, false).await;
    assert_eq!(d0, Duration::from_secs(1));
    let d1 = retry_delays(1, None, false).await;
    assert_eq!(d1, Duration::from_secs(2));
    let d2 = retry_delays(2, None, false).await;
    assert_eq!(d2, Duration::from_secs(4));
    let d3 = retry_delays(3, None, false).await;
    assert_eq!(d3, Duration::from_secs(8));
}

#[tokio::test(start_paused = true)]
async fn older_low_sample_keeps_watermark_and_rejects_middle_high() {
    let gov = Governor::new(gov_config());
    let first = gov.admit(Lane::Api, "r").await;
    let middle = gov.admit(Lane::Api, "r").await;
    let last = gov.admit(Lane::Api, "r").await;
    gov.observe(last.issue(), 100.0, Some(1.0));
    gov.observe(first.issue(), 80.0, Some(1.0));
    gov.observe(middle.issue(), 500.0, Some(1.0));
    assert_eq!(gov.watermark(), last.issue());
    assert!((gov.estimate() - 80.0).abs() < 1e-6);
    assert_eq!(gov.telemetry().cost, Some(3.0));
}

#[tokio::test(start_paused = true)]
async fn cooldown_drains_existing_requests_and_spaces_queued_probes() {
    let gov = Governor::new(gov_config());
    let old = gov.admit(Lane::Api, "r").await;
    let low = gov.admit(Lane::Api, "r").await;
    gov.observe(low.issue(), 100.0, Some(1.0));
    drop(low);
    let mut first = Box::pin(gov.admit(Lane::Api, "r"));
    assert!(futures_util::poll!(&mut first).is_pending());
    advance(Duration::from_secs(5)).await;
    assert!(
        futures_util::poll!(&mut first).is_pending(),
        "old request still running"
    );
    drop(old);
    assert!(
        futures_util::poll!(&mut first).is_pending(),
        "probe timer starts after draining"
    );
    advance(Duration::from_secs(5)).await;
    let first = first.await;
    assert_eq!(gov.in_flight(), 1);
    let mut second = Box::pin(gov.admit(Lane::Storage, "s"));
    assert!(futures_util::poll!(&mut second).is_pending());
    advance(Duration::from_secs(5)).await;
    assert!(futures_util::poll!(&mut second).is_pending());
    gov.observe(first.issue(), 90.0, Some(1.0));
    drop(first);
    assert!(futures_util::poll!(&mut second).is_pending());
    advance(Duration::from_secs(4)).await;
    assert!(futures_util::poll!(&mut second).is_pending());
    advance(Duration::from_secs(1)).await;
    let _second = second.await;
    assert_eq!(gov.in_flight(), 1);
}

#[tokio::test(start_paused = true)]
async fn probes_bootstrap_refill_and_recover_under_continuous_cost_one() {
    let gov = Governor::new(gov_config());
    let first = gov.admit(Lane::Api, "r").await;
    gov.observe(first.issue(), 100.0, Some(1.0));
    drop(first);
    let mut remaining = 100.0;
    let mut last = Instant::now();
    for n in 0..5 {
        let probe = gov.admit(Lane::Api, "r").await;
        let elapsed = last.elapsed();
        if n == 0 {
            assert_eq!(elapsed, Duration::from_secs(5));
        }
        remaining += elapsed.as_secs_f64() * 10.0 - 1.0;
        gov.observe(probe.issue(), remaining, Some(1.0));
        drop(probe);
        last = Instant::now();
    }
    assert!(
        remaining >= 300.0,
        "continuous requests must escape cooldown: {remaining}"
    );
    let a = gov.admit(Lane::Api, "r").await;
    let b = gov.admit(Lane::Api, "r").await;
    assert_eq!(last.elapsed(), Duration::ZERO);
    assert_eq!(gov.in_flight(), 2);
    drop((a, b));
}

#[tokio::test(start_paused = true)]
async fn silence_reset_starts_a_new_epoch_and_does_not_repeat_per_request() {
    let gov = Governor::new(gov_config());
    let p = gov.admit(Lane::Api, "r").await;
    gov.observe(p.issue(), 200.0, None);
    drop(p);
    advance(Duration::from_secs(60)).await;
    drop(gov.admit(Lane::Api, "r").await);
    drop(gov.admit(Lane::Api, "r").await);
    assert!((gov.estimate() - 698.0).abs() < 1e-6);
    assert_eq!(gov.watermark(), 0);
}

#[tokio::test(start_paused = true)]
async fn silence_cannot_reset_on_admission_with_existing_in_flight() {
    let gov = Governor::new(gov_config());
    let p = gov.admit(Lane::Api, "r").await;
    gov.observe(p.issue(), 200.0, None);
    advance(Duration::from_secs(60)).await;
    let _q = gov.admit(Lane::Storage, "s").await;
    assert!((gov.estimate() - 199.0).abs() < 1e-6);
    drop(p);
}

#[tokio::test(start_paused = true)]
async fn invalid_headers_do_not_poison_estimate_or_telemetry() {
    let gov = Governor::new(gov_config());
    let p = gov.admit(Lane::Api, "r").await;
    gov.observe(p.issue(), f64::NAN, Some(f64::INFINITY));
    gov.observe_cost_only(p.issue(), -5.0);
    assert_eq!(gov.telemetry().cost, None);
    assert!((gov.estimate() - 699.0).abs() < 1e-6);
}

#[tokio::test(start_paused = true)]
async fn http_retries_have_exact_delays_and_exhaust_429_and_throttle_403() {
    // Keep paused time from auto-advancing while real socket I/O is pending.
    let keep_awake = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    for status in [429, 403] {
        let server = MockServer::start().await;
        Mock::given(path("/retry"))
            .respond_with(ResponseTemplate::new(status).set_body_string("Rate Limit Exceeded"))
            .expect(5)
            .mount(&server)
            .await;
        let client = common::test_client(&server);
        let state = client.clone();
        let request = tokio::spawn(async move { client.get::<serde_json::Value>("/retry").await });
        wait_for_received(&server, 1).await;
        let mut times = vec![Instant::now()];
        for (index, seconds) in [1, 2, 4, 8].into_iter().enumerate() {
            while state.governor().in_flight() != 0 {
                wall_sleep(1).await;
            }
            wall_sleep(2).await;
            advance(
                Duration::from_secs(seconds)
                    .checked_sub(Duration::from_millis(1))
                    .unwrap(),
            )
            .await;
            wall_sleep(2).await;
            assert_eq!(server.received_requests().await.unwrap().len(), index + 1);
            advance(Duration::from_millis(1)).await;
            wait_for_received(&server, index + 2).await;
            times.push(Instant::now());
        }
        assert!(matches!(request.await.unwrap(), Err(Error::RateLimited)));
        let delays: Vec<_> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert_eq!(delays, [1, 2, 4, 8].map(Duration::from_secs));
        assert_eq!(state.telemetry().api, 5);
    }
    keep_awake.abort();
}

#[tokio::test]
async fn ordinary_403_is_not_retried_and_validation_keeps_messages() {
    let server = MockServer::start().await;
    Mock::given(path("/forbidden"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/invalid"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"errors":{"name":[{"message":"is required"}]}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = common::test_client(&server);
    assert!(matches!(
        client.get::<serde_json::Value>("/forbidden").await,
        Err(Error::Forbidden {
            rate_limited: false,
            ..
        })
    ));
    let Error::Validation { status, errors } = client
        .get::<serde_json::Value>("/invalid")
        .await
        .unwrap_err()
    else {
        panic!("expected validation");
    };
    assert_eq!(status, 422);
    assert_eq!(errors, vec!["name: is required"]);
}

#[tokio::test(start_paused = true)]
async fn retry_must_pass_cooldown_admission_again() {
    let keep_awake = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let server = MockServer::start().await;
    Mock::given(path("/retry"))
        .respond_with(ResponseTemplate::new(429).insert_header("X-Rate-Limit-Remaining", "100"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/retry"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true}))
                .insert_header("X-Rate-Limit-Remaining", "350"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = common::test_client(&server);
    let state = client.clone();
    let request = tokio::spawn(async move { client.get::<OkBody>("/retry").await });
    wait_for_received(&server, 1).await;
    while state.governor().in_flight() > 0 {
        wall_sleep(1).await;
    }
    wall_sleep(2).await;
    advance(Duration::from_secs(1)).await;
    wall_sleep(2).await;
    assert_eq!(state.telemetry().api, 1);
    advance(Duration::from_secs(4)).await;
    wall_sleep(2).await;
    assert_eq!(state.telemetry().api, 1);
    advance(Duration::from_secs(1)).await;
    wait_for_received(&server, 2).await;
    assert!(request.await.unwrap().unwrap().ok);
    assert_eq!(state.telemetry().api, 2);
    keep_awake.abort();
}

// ------------------------------------------------------------- M6-c seams

mod seams {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use canvas_api::{
        Governor, GovernorConfig, GovernorSnapshot, GovernorState, Lane, LaneSlot, Permits, Seams,
    };

    /// Counts acquisitions and hands back a plain guard.
    struct CountingPermits {
        api: AtomicUsize,
        storage: AtomicUsize,
    }

    impl Permits for CountingPermits {
        fn acquire(
            &self,
            lane: Lane,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = LaneSlot> + Send + '_>> {
            match lane {
                Lane::Api => self.api.fetch_add(1, Ordering::Relaxed),
                Lane::Storage => self.storage.fetch_add(1, Ordering::Relaxed),
            };
            Box::pin(async move { Box::new(()) as LaneSlot })
        }
    }

    /// An in-memory stand-in for the `governor` row.
    #[derive(Default)]
    struct MemoryState {
        row: Mutex<Option<GovernorSnapshot>>,
        writes: AtomicUsize,
    }

    impl GovernorState for MemoryState {
        fn load(&self) -> Option<GovernorSnapshot> {
            *self.row.lock().expect("row")
        }

        fn update(
            &self,
            merge: &mut dyn FnMut(Option<GovernorSnapshot>) -> Option<GovernorSnapshot>,
        ) {
            let mut row = self.row.lock().expect("row");
            if let Some(next) = merge(*row) {
                *row = Some(next);
                self.writes.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn config() -> GovernorConfig {
        GovernorConfig {
            api_concurrency: 2,
            storage_concurrency: 2,
            full_remaining: 700.0,
            jitter: false,
        }
    }

    #[tokio::test]
    async fn a_supplied_permits_implementation_replaces_the_lane_semaphores() {
        let permits = Arc::new(CountingPermits {
            api: AtomicUsize::new(0),
            storage: AtomicUsize::new(0),
        });
        let governor = Governor::with_seams(
            config(),
            &Seams {
                permits: Some(permits.clone()),
                state: None,
            },
        );
        // Three API admissions at concurrency two: the seam, not the semaphore,
        // decides, so all three are handed a slot.
        let permits_held = vec![
            governor.admit(Lane::Api, "GET /a").await,
            governor.admit(Lane::Api, "GET /a").await,
            governor.admit(Lane::Api, "GET /a").await,
        ];
        let transfer = governor.admit(Lane::Storage, "transfer:download").await;
        assert_eq!(permits.api.load(Ordering::Relaxed), 3);
        assert_eq!(permits.storage.load(Ordering::Relaxed), 1);
        assert_eq!(governor.in_flight(), 4);
        drop(permits_held);
        drop(transfer);
        assert_eq!(governor.in_flight(), 0);
    }

    #[tokio::test]
    async fn the_shared_row_pre_charges_cost_and_carries_a_lower_estimate_across_governors() {
        let shared = Arc::new(MemoryState::default());
        let seams = Seams {
            permits: None,
            state: Some(shared.clone()),
        };
        let first = Governor::with_seams(config(), &seams);
        let permit = first.admit(Lane::Api, "GET /a").await;
        // The pre-charge is on the shared row, not only in this process.
        let row = shared.load().expect("row written at admission");
        assert!((row.estimate - 699.0).abs() < f64::EPSILON, "{row:?}");
        drop(permit);

        // A lower sample from one process applies in the other.
        first.apply_observation_for_test(1, 200.0, None);
        let second = Governor::with_seams(config(), &seams);
        let permit = second.admit(Lane::Api, "GET /b").await;
        assert!(second.estimate() < 210.0, "{}", second.estimate());
        drop(permit);
    }

    #[tokio::test]
    async fn a_higher_shared_estimate_applies_only_above_the_watermark() {
        let shared = Arc::new(MemoryState::default());
        let seams = Seams {
            permits: None,
            state: Some(shared.clone()),
        };
        let governor = Governor::with_seams(config(), &seams);
        governor.set_estimate_for_test(200.0);
        // A stale row at or below the watermark cannot raise the estimate.
        *shared.row.lock().expect("row") = Some(GovernorSnapshot {
            estimate: 690.0,
            watermark: 0,
            cooldown_until: None,
            refill: 0.0,
            updated_at: canvas_api::governor::unix_millis(),
        });
        let permit = governor.admit(Lane::Api, "GET /a").await;
        assert!(governor.estimate() <= 200.0, "{}", governor.estimate());
        drop(permit);

        // The same value above the watermark does apply.
        *shared.row.lock().expect("row") = Some(GovernorSnapshot {
            estimate: 690.0,
            watermark: 99,
            cooldown_until: None,
            refill: 0.0,
            updated_at: canvas_api::governor::unix_millis(),
        });
        let permit = governor.admit(Lane::Api, "GET /a").await;
        assert!(governor.estimate() > 600.0, "{}", governor.estimate());
        drop(permit);
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_process_cannot_reset_a_shared_row_another_one_keeps_fresh() {
        let shared = Arc::new(MemoryState::default());
        let seams = Seams {
            permits: None,
            state: Some(shared.clone()),
        };
        let governor = Governor::with_seams(config(), &seams);
        // This process has seen no header of its own for well past the silence
        // window: `watch` between two polls looks exactly like this.
        tokio::time::advance(std::time::Duration::from_secs(120)).await;
        // Another process is live and has just published a low estimate and a
        // cooldown. The silence window belongs to the row, not to this process.
        *shared.row.lock().expect("row") = Some(GovernorSnapshot {
            estimate: 40.0,
            watermark: 12,
            cooldown_until: Some(canvas_api::governor::unix_millis() + 5_000),
            refill: 0.0,
            updated_at: canvas_api::governor::unix_millis(),
        });
        // Admission must see the shared cooldown, not a full bucket: with no
        // refill the §11 probe wait is five seconds before a single request.
        let started = tokio::time::Instant::now();
        let permit = governor.admit(Lane::Api, "GET /a").await;
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(5),
            "a quiet process admitted through a shared cooldown after {:?}",
            started.elapsed()
        );
        assert!(
            governor.estimate() < 100.0,
            "a quiet process invented a full bucket: {}",
            governor.estimate()
        );
        assert_eq!(governor.watermark(), 12);
        drop(permit);
    }

    #[tokio::test]
    async fn a_stale_shared_row_resets_exactly_as_the_silence_rule_says() {
        let shared = Arc::new(MemoryState::default());
        let seams = Seams {
            permits: None,
            state: Some(shared.clone()),
        };
        let governor = Governor::with_seams(config(), &seams);
        // The owner died in cooldown with a low estimate, 61 seconds ago.
        *shared.row.lock().expect("row") = Some(GovernorSnapshot {
            estimate: 40.0,
            watermark: 12,
            cooldown_until: Some(canvas_api::governor::unix_millis() + 5_000),
            refill: 3.0,
            updated_at: canvas_api::governor::unix_millis() - 61_000,
        });
        let permit = governor.admit(Lane::Api, "GET /a").await;
        assert!(governor.estimate() > 690.0, "{}", governor.estimate());
        assert_eq!(governor.watermark(), 0);
        drop(permit);

        // The same row, written a second ago, is adopted rather than reset.
        let governor = Governor::with_seams(config(), &seams);
        *shared.row.lock().expect("row") = Some(GovernorSnapshot {
            estimate: 40.0,
            watermark: 12,
            cooldown_until: None,
            refill: 3.0,
            updated_at: canvas_api::governor::unix_millis() - 1_000,
        });
        governor.adopt_shared_for_test();
        assert!(governor.estimate() < 100.0, "{}", governor.estimate());
    }
}

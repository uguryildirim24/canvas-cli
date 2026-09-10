//! Frozen-clock helper for `CANVAS_NOW` (SPEC §16 / tests).

use std::cell::RefCell;

use jiff::Timestamp;

thread_local! {
    static NOW_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Resolve "now" as a [`Timestamp`].
///
/// Order: test override → `CANVAS_NOW` env → wall clock.
///
/// SPEC §16 makes the frozen clock a test-build affordance, and the other
/// test-only environment variables (`CANVAS_TEST_ALLOW_HTTP`,
/// `CANVAS_TEST_FORCE_FILE`, `CANVAS_TEST_CRASH_AFTER`) carry the same
/// `debug_assertions` gate. A release build must not let the environment move
/// the clock: due dates, TTL freshness and the §12.2 thirty-minute
/// `--assume-not-submitted` window are all derived from it.
#[must_use]
pub fn now_timestamp() -> Timestamp {
    if let Some(raw) = current_override() {
        return parse_now(&raw);
    }
    if cfg!(debug_assertions)
        && let Ok(raw) = std::env::var("CANVAS_NOW")
        && !raw.is_empty()
    {
        return parse_now(&raw);
    }
    Timestamp::now()
}

/// RFC 3339 UTC string for envelope `generated_at`.
#[must_use]
pub fn generated_at_now() -> String {
    now_timestamp().to_string()
}

fn current_override() -> Option<String> {
    NOW_OVERRIDE.with(|slot| slot.borrow().clone())
}

fn parse_now(raw: &str) -> Timestamp {
    raw.parse().unwrap_or_else(|_| Timestamp::now())
}

/// Run `f` with a frozen `CANVAS_NOW` override (tests).
#[cfg(test)]
pub fn with_canvas_now<R>(value: &str, f: impl FnOnce() -> R) -> R {
    NOW_OVERRIDE.with(|slot| {
        let previous = slot.replace(Some(value.to_string()));
        let result = f();
        slot.replace(previous);
        result
    })
}

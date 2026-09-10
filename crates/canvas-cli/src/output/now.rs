//! Frozen-clock helper for `CANVAS_NOW` (SPEC §16 / tests).

use std::cell::RefCell;

use jiff::Timestamp;

thread_local! {
    static NOW_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Resolve "now" as a [`Timestamp`].
///
/// Order: test override → `CANVAS_NOW` env → wall clock.
#[must_use]
pub fn now_timestamp() -> Timestamp {
    if let Some(raw) = current_override() {
        return parse_now(&raw);
    }
    match std::env::var("CANVAS_NOW") {
        Ok(raw) if !raw.is_empty() => parse_now(&raw),
        _ => Timestamp::now(),
    }
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

//! Turning one §7 envelope into an MCP result.
//!
//! A tool result carries the whole envelope: `structuredContent` is the
//! document `--json` prints, and the text block is the same document
//! serialized, so a host that shows only text still shows the contract. A
//! domain failure keeps the envelope and its `outcome`/`exit`; only a
//! protocol or argument failure becomes a JSON-RPC error (§3.2).

use jiff::Timestamp;
use rmcp::model::{CacheScope, CallToolResult, MetaObject};
use serde_json::{Value, json};

use crate::commands::handled::JsonEnvelope;
use crate::output::{Freshness, Outcome};
use crate::session;

/// `_meta` key carrying the cache scope of a tool result.
///
/// The 2026-07-28 caching rules put `ttlMs` and `cacheScope` on discovery,
/// list, and `resources/read` results, and nowhere else, so a tool result
/// reports the same two values under this server's own `_meta` keys instead
/// of inventing wire fields.
pub const META_CACHE_SCOPE: &str = "dev.canvas-cli/cacheScope";
/// `_meta` key carrying the freshness budget of a tool result, in ms.
pub const META_TTL_MS: &str = "dev.canvas-cli/ttlMs";

/// How long a result may be treated as fresh, in milliseconds.
///
/// The budget is the remaining TTL of the least fresh dataset the envelope
/// names. Anything unresolved — no dataset, a stale or incomplete row, an
/// unknown dataset, or a fetch time in the future — is zero, so a host
/// re-reads instead of trusting a value the cache cannot support.
#[must_use]
pub fn ttl_ms(freshness: &[Freshness], now: Timestamp) -> u64 {
    if freshness.is_empty() {
        return 0;
    }
    let mut budget = u64::MAX;
    for row in freshness {
        let remaining = row_ttl_ms(row, now);
        if remaining == 0 {
            return 0;
        }
        budget = budget.min(remaining);
    }
    if budget == u64::MAX { 0 } else { budget }
}

fn row_ttl_ms(row: &Freshness, now: Timestamp) -> u64 {
    if row.stale || !row.complete {
        return 0;
    }
    let (Some(fetched_at), Some(ttl)) = (
        row.fetched_at
            .as_deref()
            .and_then(|at| at.parse::<Timestamp>().ok()),
        dataset_ttl(&row.dataset),
    ) else {
        return 0;
    };
    let Ok(expires) = fetched_at.checked_add(ttl) else {
        return 0;
    };
    // Integer milliseconds: a freshness budget must never round up.
    let remaining = expires.as_millisecond() - now.as_millisecond();
    u64::try_from(remaining).unwrap_or(0)
}

/// The configured TTL of one dataset (§10), or `None` when it has no group.
fn dataset_ttl(dataset: &str) -> Option<jiff::Span> {
    Some(match dataset {
        "courses" | "terms" => session::ttl_courses(),
        "enrollment_grades" | "course_totals" | "grading_periods" => session::ttl_grades(),
        "assignments" | "submission" | "assignment_groups" => session::ttl_assignments(),
        "missing" => session::ttl_missing(),
        "planner" => session::ttl_planner(),
        "folders" | "files" => session::ttl_files(),
        "modules" | "module_items" => session::ttl_modules(),
        "announcements" | "announcement" => session::ttl_announcements(),
        "calendar_events" => session::ttl_calendar(),
        _ => return None,
    })
}

/// A domain failure the host should show: the envelope is still the answer.
///
/// `recovery` belongs here. §14 ranks exit 9 above `mismatch` (10) and
/// `refused` (8) in its precedence for a completed command, so a result the
/// CLI ranks highest cannot be the one an agent host renders as a plain
/// success — a submission whose outcome is unknown is the case §12.2 is most
/// careful about.
///
/// `partial` does not. Exit 12 means some of the answer is missing and the
/// rest is usable; `partial` names what failed, and a host that hid the
/// answer would lose the part that worked.
fn is_domain_failure(outcome: Outcome) -> bool {
    match outcome {
        Outcome::Error | Outcome::Refused | Outcome::Mismatch | Outcome::Recovery => true,
        Outcome::Ok | Outcome::Partial => false,
    }
}

/// Build the tool result for a finished command.
#[must_use]
pub fn tool_result(envelope: &dyn JsonEnvelope, now: Timestamp) -> CallToolResult {
    let document = envelope.to_value();
    from_envelope_value(
        document,
        envelope.outcome(),
        ttl_ms(envelope.freshness(), now),
    )
}

/// Build a tool result from an envelope document that is already a value.
#[must_use]
pub fn from_envelope_value(document: Value, outcome: Outcome, ttl_ms: u64) -> CallToolResult {
    // `structured` also puts the serialized document in a text block, which is
    // what a host that renders text alone shows the model.
    let mut result = if is_domain_failure(outcome) {
        CallToolResult::structured_error(document)
    } else {
        CallToolResult::structured(document)
    };
    result.meta = Some(cache_meta(ttl_ms));
    result
}

/// Every result of this server is one identity's private data.
fn cache_meta(ttl_ms: u64) -> MetaObject {
    let mut meta = MetaObject::new();
    meta.0.insert(
        META_CACHE_SCOPE.to_owned(),
        serde_json::to_value(CacheScope::Private).unwrap_or(json!("private")),
    );
    meta.0.insert(META_TTL_MS.to_owned(), json!(ttl_ms));
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::FreshnessSource;

    fn row(dataset: &str, fetched_at: &str, stale: bool, complete: bool) -> Freshness {
        Freshness {
            dataset: dataset.to_owned(),
            scope: "active".to_owned(),
            source: FreshnessSource::Cache,
            fetched_at: Some(fetched_at.to_owned()),
            complete,
            count: Some(1),
            stale,
        }
    }

    #[test]
    fn an_envelope_without_datasets_is_immediately_stale() {
        let now: Timestamp = "2026-09-09T17:05:12Z".parse().unwrap();
        assert_eq!(ttl_ms(&[], now), 0);
    }

    #[test]
    fn the_budget_is_the_least_fresh_dataset() {
        let now: Timestamp = "2026-09-09T17:05:12Z".parse().unwrap();
        // courses: 6 h by default, announcements: 15 min.
        let rows = [
            row("courses", "2026-09-09T17:00:12Z", false, true),
            row("announcements", "2026-09-09T17:00:12Z", false, true),
        ];
        let budget = ttl_ms(&rows, now);
        assert!(
            (590_000..=600_000).contains(&budget),
            "announcements has 10 minutes left, got {budget}"
        );
    }

    #[test]
    fn stale_incomplete_and_unknown_datasets_are_zero() {
        let now: Timestamp = "2026-09-09T17:05:12Z".parse().unwrap();
        for rows in [
            vec![row("courses", "2026-09-09T17:00:12Z", true, true)],
            vec![row("courses", "2026-09-09T17:00:12Z", false, false)],
            vec![row("unknown_dataset", "2026-09-09T17:00:12Z", false, true)],
            vec![row("courses", "not-a-timestamp", false, true)],
            vec![
                row("courses", "2026-09-09T17:00:12Z", false, true),
                row("announcements", "2026-01-01T00:00:00Z", false, true),
            ],
        ] {
            assert_eq!(ttl_ms(&rows, now), 0, "{rows:?}");
        }
    }

    #[test]
    fn a_domain_failure_is_an_error_result_but_keeps_the_envelope() {
        let document = json!({ "schema": "canvas-cli/error@1", "exit": 8 });
        let result = from_envelope_value(document.clone(), Outcome::Refused, 0);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, Some(document));
        let meta = result.meta.expect("cache hints");
        assert_eq!(meta.0[META_CACHE_SCOPE], json!("private"));
        assert_eq!(meta.0[META_TTL_MS], json!(0));
    }

    #[test]
    fn a_partial_result_is_not_an_error() {
        let result = from_envelope_value(json!({ "exit": 12 }), Outcome::Partial, 5);
        assert_eq!(result.is_error, Some(false));
    }

    /// Exit 9 outranks every other completed outcome in §14, so it is never
    /// the one a host renders as a plain success.
    #[test]
    fn an_unresolved_submission_is_an_error_result() {
        let document = json!({ "schema": "canvas-cli/submit@1", "exit": 9 });
        let result = from_envelope_value(document.clone(), Outcome::Recovery, 0);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content, Some(document));
    }
}

//! Window + context scope keys for the batched datasets (§10).
//!
//! `announcements` and `calendar_events` are fetched for a date window over a
//! set of Canvas contexts, ten per request. Two requests for the same window
//! over different contexts are different coverage, so the scope key carries a
//! digest of the context set as well as the window.

use sha2::{Digest, Sha256};

use super::planner::PlannerWindow;

/// Canvas caps `context_codes[]` at ten per request (§12.5, §12.6).
pub const CONTEXT_BATCH: usize = 10;

/// A date window plus the context set it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextWindow {
    window: PlannerWindow,
    contexts: Vec<String>,
    hash: String,
    scope_key: String,
}

impl ContextWindow {
    /// Scope for `announcements`: contexts are `course_<id>`, and the digest
    /// covers the sorted course ids (§10).
    #[must_use]
    pub fn courses(window: PlannerWindow, course_ids: &[i64]) -> Self {
        let mut ids: Vec<i64> = course_ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let digest = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let contexts = ids.iter().map(|id| format!("course_{id}")).collect();
        Self::build(window, contexts, &digest)
    }

    /// Scope for `calendar_events`: the digest covers the sorted context codes
    /// (§10), which include `user_<id>` as well as the courses.
    #[must_use]
    pub fn contexts(window: PlannerWindow, contexts: &[String]) -> Self {
        let mut codes: Vec<String> = contexts.to_vec();
        codes.sort();
        codes.dedup();
        let digest = codes.join(",");
        Self::build(window, codes, &digest)
    }

    fn build(window: PlannerWindow, contexts: Vec<String>, digest: &str) -> Self {
        let hash = format!("{:x}", Sha256::digest(digest.as_bytes()));
        let scope_key = format!("{}:ctx:{hash}", window.scope_key());
        Self {
            window,
            contexts,
            hash,
            scope_key,
        }
    }

    #[must_use]
    pub fn window(&self) -> &PlannerWindow {
        &self.window
    }

    /// Sorted, deduplicated context codes.
    #[must_use]
    pub fn context_codes(&self) -> &[String] {
        &self.contexts
    }

    /// Hex sha256 recorded in `fetch_log.contexts`.
    #[must_use]
    pub fn context_hash(&self) -> &str {
        &self.hash
    }

    /// `window:<start>..<end>:ctx:<sha256>`.
    #[must_use]
    pub fn scope_key(&self) -> &str {
        &self.scope_key
    }

    /// The request batches, ten contexts each, in scope-key order.
    #[must_use]
    pub fn batches(&self) -> Vec<Vec<String>> {
        self.contexts
            .chunks(CONTEXT_BATCH)
            .map(<[String]>::to_vec)
            .collect()
    }
}

/// `context_codes[]` query fragment for one batch.
#[must_use]
pub fn context_codes_query(batch: &[String]) -> String {
    batch.iter().fold(String::new(), |mut query, code| {
        query.push_str("&context_codes[]=");
        query.push_str(code);
        query
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn window() -> PlannerWindow {
        PlannerWindow {
            start: date(2026, 9, 1),
            end: date(2026, 9, 14),
        }
    }

    #[test]
    fn scope_key_carries_the_window_and_a_context_digest() {
        let a = ContextWindow::courses(window(), &[3, 1, 2]);
        let b = ContextWindow::courses(window(), &[2, 3, 1, 1]);
        assert_eq!(
            a.scope_key(),
            b.scope_key(),
            "order and repeats must not matter"
        );
        assert_eq!(a.context_codes(), ["course_1", "course_2", "course_3"]);
        assert!(
            a.scope_key()
                .starts_with("window:2026-09-01..2026-09-14:ctx:")
        );
        assert_eq!(a.context_hash().len(), 64);

        let different = ContextWindow::courses(window(), &[1, 2]);
        assert_ne!(a.scope_key(), different.scope_key());
    }

    #[test]
    fn course_digest_and_context_digest_are_distinct_scopes() {
        let by_id = ContextWindow::courses(window(), &[1]);
        let by_code = ContextWindow::contexts(window(), &["course_1".into()]);
        assert_eq!(by_id.context_codes(), by_code.context_codes());
        // The digests cover different canonical forms, so the two datasets
        // never collide on one another's coverage.
        assert_ne!(by_id.context_hash(), by_code.context_hash());
    }

    #[test]
    fn batches_are_capped_at_ten() {
        let ids: Vec<i64> = (1..=23).collect();
        let cw = ContextWindow::courses(window(), &ids);
        let batches = cw.batches();
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 10);
        assert_eq!(batches[2].len(), 3);
        assert_eq!(
            context_codes_query(&batches[2]),
            "&context_codes[]=course_21&context_codes[]=course_22&context_codes[]=course_23"
        );
    }
}

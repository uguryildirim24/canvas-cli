//! Bounds and sanitization applied to whatever the browser observed.
//!
//! The extension applies all of this before a byte leaves Chrome (the design note
//! §3.3). The host applies it again, because the host is the side that knows
//! the CLI identity and it must not depend on the extension being the one it
//! installed.

use crate::bridge::wire::Extract;

/// The total browser payload ceiling, in UTF-8 bytes.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Query parameters that can carry a capability and never a route.
///
/// `x-amz-` is a prefix: the signed-URL family spells several of them.
pub const CAPABILITY_PARAMS: &[&str] = &[
    "verifier",
    "signature",
    "token",
    "access_token",
    "sig",
    "policy",
    "expires",
    "session_token",
];

/// Whether a query parameter name carries a capability.
///
/// The comparison is case-insensitive: Canvas spells `verifier` lower case
/// and the signed-storage parameters spell `Signature`, `Policy`, `Expires`
/// and `X-Amz-*` in mixed case.
#[must_use]
pub fn is_capability_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("x-amz-") || CAPABILITY_PARAMS.contains(&lower.as_str())
}

/// Rewrite a URL with every capability-bearing parameter removed.
///
/// A URL that cannot be parsed is reduced to nothing rather than passed
/// through: the companion has no use for a URL it cannot inspect.
#[must_use]
pub fn sanitize_url(raw: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| !is_capability_param(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    {
        let mut pairs = url.query_pairs_mut();
        pairs.clear();
        for (name, value) in &kept {
            pairs.append_pair(name, value);
        }
    }
    if url.query().is_some_and(str::is_empty) {
        url.set_query(None);
    }
    // A fragment is page state the companion never needs, and Canvas puts
    // module item state in it.
    url.set_fragment(None);
    // Credentials in a URL are never shared.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Some(url.to_string())
}

/// Cut `text` to at most `max` bytes, on a character boundary.
///
/// Returns the kept slice and whether anything was dropped.
#[must_use]
pub fn truncate_utf8(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// Apply the total payload ceiling to one extract.
///
/// The selection is what the person pointed at, so it is kept first and the
/// surrounding excerpt takes what is left. `truncated` stays set once set:
/// the extension may already have cut the text before sending it.
pub fn bound_extract(extract: &mut Extract) {
    let mut budget = MAX_PAYLOAD_BYTES;
    if let Some(selection) = extract.selection.as_mut() {
        let (kept, cut) = truncate_utf8(selection, budget);
        if cut {
            selection.truncate(kept.len());
            extract.truncated = true;
        }
        budget -= selection.len();
    }
    if let Some(text) = extract.text.as_mut() {
        let (kept, cut) = truncate_utf8(text, budget);
        if cut {
            text.truncate(kept.len());
            extract.truncated = true;
        }
    }
}

/// The UTF-8 length of an extract, for the lengths the bundle reports.
#[must_use]
pub fn extract_bytes(extract: &Extract) -> usize {
    extract.selection.as_ref().map_or(0, String::len) + extract.text.as_ref().map_or(0, String::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_parameters_are_named_case_insensitively() {
        for name in [
            "verifier",
            "Verifier",
            "Signature",
            "X-Amz-Signature",
            "x-amz-credential",
            "token",
            "sig",
            "Policy",
            "Expires",
        ] {
            assert!(is_capability_param(name), "{name}");
        }
        for name in ["page", "per_page", "module_item_id", "anchor"] {
            assert!(!is_capability_param(name), "{name}");
        }
    }

    #[test]
    fn a_sanitized_url_keeps_the_route_and_drops_the_capability() {
        let url = sanitize_url(
            "https://school.test/courses/1/files/9/download?verifier=abc123&wrap=1&X-Amz-Signature=deadbeef#frag",
        )
        .expect("a canvas url");
        assert!(url.contains("wrap=1"), "{url}");
        assert!(!url.contains("verifier"), "{url}");
        assert!(!url.to_ascii_lowercase().contains("amz"), "{url}");
        assert!(!url.contains('#'), "{url}");
    }

    #[test]
    fn a_url_with_no_parameters_left_carries_no_empty_query() {
        let url = sanitize_url("https://school.test/courses/1?verifier=abc").expect("a url");
        assert_eq!(url, "https://school.test/courses/1");
    }

    #[test]
    fn credentials_in_a_url_are_never_shared() {
        let url = sanitize_url("https://user:pass@school.test/courses/1").expect("a url");
        assert!(!url.contains("user"), "{url}");
        assert!(!url.contains("pass"), "{url}");
    }

    #[test]
    fn a_url_this_release_cannot_inspect_is_dropped() {
        assert_eq!(sanitize_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_url("not a url"), None);
        assert_eq!(sanitize_url("data:text/html,<b>x</b>"), None);
    }

    /// M7-a acceptance: byte bounds fall on character boundaries.
    #[test]
    fn the_bound_never_splits_a_character() {
        // Four-byte characters, so no multiple of the budget lands on a
        // boundary by accident.
        let text: String = "😀".repeat(MAX_PAYLOAD_BYTES);
        let (kept, cut) = truncate_utf8(&text, MAX_PAYLOAD_BYTES);
        assert!(cut);
        assert!(kept.len() <= MAX_PAYLOAD_BYTES);
        assert!(std::str::from_utf8(kept.as_bytes()).is_ok());
        assert_eq!(kept.len() % 4, 0, "a 4-byte character was split");
    }

    #[test]
    fn the_total_payload_is_bounded_across_both_fields() {
        let mut extract = Extract {
            selection: Some("é".repeat(MAX_PAYLOAD_BYTES)),
            text: Some("x".repeat(MAX_PAYLOAD_BYTES)),
            truncated: false,
        };
        bound_extract(&mut extract);
        assert!(extract.truncated);
        assert!(extract_bytes(&extract) <= MAX_PAYLOAD_BYTES);
        assert!(
            std::str::from_utf8(extract.selection.as_ref().unwrap().as_bytes()).is_ok(),
            "the selection stays valid UTF-8"
        );
    }

    #[test]
    fn a_payload_inside_the_bound_is_untouched() {
        let mut extract = Extract {
            selection: Some("a short passage".to_owned()),
            text: Some("the visible excerpt".to_owned()),
            truncated: false,
        };
        bound_extract(&mut extract);
        assert!(!extract.truncated);
        assert_eq!(extract.selection.as_deref(), Some("a short passage"));
    }

    /// A shorter payload that the extension already cut stays marked.
    #[test]
    fn a_truncation_flag_survives_a_second_bound() {
        let mut extract = Extract {
            selection: None,
            text: Some("short".to_owned()),
            truncated: true,
        };
        bound_extract(&mut extract);
        assert!(extract.truncated);
    }
}

//! Notes the panel displays, and the source refs they may carry.
//!
//! A note is **inert**. It is text an agent already produced, held by the
//! broker for the attachment's lifetime and shown to the person in the
//! extension-owned panel. It reaches no Canvas page, it runs nothing, and it
//! decides nothing: the approval path in `crate::plan` never reads a note,
//! and the socket protocol has no approval message at all, so a note whose
//! text or refs are shaped like an approval payload changes exactly nothing.
//!
//! Two bounds are applied here, before a note is stored:
//!
//! - the text is at most [`MAX_NOTE_BYTES`] of UTF-8, and an oversize note is
//!   refused rather than silently cut, because a note the person reads must
//!   be the note the agent wrote;
//! - every source ref is `https://<the granted origin>/…` or `canvas://…`.
//!   Anything else is refused. Links **inside** the text are a separate
//!   matter: the panel's renderer decides those, and it never builds a link
//!   to anywhere but the same two places.

use serde::{Deserialize, Serialize};

use crate::bridge::ipc::Reason;

/// The largest note the broker accepts, in UTF-8 bytes (REPORT §3.2).
pub const MAX_NOTE_BYTES: usize = 8 * 1024;

/// The largest number of source refs one note may carry.
///
/// A note is a paragraph with citations, not a link dump. The bound exists so
/// one note cannot fill the panel with rows.
pub const MAX_SOURCE_REFS: usize = 16;

/// The scheme this project's own references use.
pub const CANVAS_SCHEME: &str = "canvas://";

/// One note, as the broker holds it and the panel displays it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Opaque id, so the panel can key a row without a position.
    pub note_id: String,
    /// Which consumer wrote it. The panel names it; a note is never anonymous.
    pub consumer: String,
    /// The note itself, as Markdown source. It is rendered, never executed.
    pub text: String,
    /// Where the note says its facts came from.
    pub source_refs: Vec<String>,
    /// When the broker accepted it.
    pub at: String,
    /// The navigation generation the note was written against.
    pub generation: u64,
}

/// Whether a source ref is one of the two forms a note may cite.
///
/// `origin` is the granted origin of the attachment. A ref to any other host,
/// and any other scheme — `javascript:`, `data:`, `file:`, plain `http:` —
/// is not a source ref this companion will show as a link.
#[must_use]
pub fn is_allowed_ref(value: &str, origin: &str) -> bool {
    if let Some(rest) = value.strip_prefix(CANVAS_SCHEME) {
        // `canvas://` names something inside this project, so it carries no
        // host at all. An empty path names nothing.
        return !rest.is_empty() && !rest.contains(|c: char| c.is_control());
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    // A ref the panel shows must read as what it is. `https://help@school
    // .test/…` resolves to the granted origin, but a person reading the row
    // sees the userinfo first, so it is not a ref this companion will show.
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Ok(granted) = reqwest::Url::parse(origin) else {
        return false;
    };
    // Compare the origin itself, not a prefix of the text: `https://x.test`
    // is a prefix of `https://x.test.evil.test`, and only one of them is the
    // granted origin.
    url.origin() == granted.origin()
}

/// Check one note's text and refs against the bounds above.
///
/// The refusals are separate so a caller can say which bound was crossed
/// without the person having to guess.
pub fn check(text: &str, source_refs: &[String], origin: &str) -> Result<(), Reason> {
    if text.trim().is_empty() {
        return Err(Reason::NoteRejected);
    }
    if text.len() > MAX_NOTE_BYTES {
        return Err(Reason::NoteTooLarge);
    }
    if source_refs.len() > MAX_SOURCE_REFS {
        return Err(Reason::NoteRejected);
    }
    if source_refs.iter().any(|r| !is_allowed_ref(r, origin)) {
        return Err(Reason::SourceRefRejected);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://courses.example.test";

    #[test]
    fn a_ref_is_the_granted_origin_or_this_project_and_nothing_else() {
        for good in [
            "https://courses.example.test/courses/1/assignments/2",
            "https://courses.example.test/",
            "canvas://receipts/0199",
            "canvas://assignment/45679/98765",
        ] {
            assert!(is_allowed_ref(good, ORIGIN), "{good}");
        }
        for bad in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "http://courses.example.test/courses/1",
            "https://courses.example.test.evil.test/courses/1",
            "https://evil.test/courses/1",
            "https://user@courses.example.test/courses/1@evil.test",
            "canvas://",
            "",
            "not a url",
        ] {
            assert!(!is_allowed_ref(bad, ORIGIN), "{bad}");
        }
    }

    #[test]
    fn an_oversize_note_is_refused_rather_than_cut() {
        let big = "x".repeat(MAX_NOTE_BYTES + 1);
        assert_eq!(check(&big, &[], ORIGIN), Err(Reason::NoteTooLarge));
        let exact = "x".repeat(MAX_NOTE_BYTES);
        assert_eq!(check(&exact, &[], ORIGIN), Ok(()));
    }

    #[test]
    fn an_empty_note_and_a_bad_ref_are_refused() {
        assert_eq!(check("   \n ", &[], ORIGIN), Err(Reason::NoteRejected));
        assert_eq!(
            check("hello", &["javascript:alert(1)".to_owned()], ORIGIN),
            Err(Reason::SourceRefRejected)
        );
        assert_eq!(
            check(
                "hello",
                &vec!["canvas://a".to_owned(); MAX_SOURCE_REFS + 1],
                ORIGIN
            ),
            Err(Reason::NoteRejected)
        );
    }
}

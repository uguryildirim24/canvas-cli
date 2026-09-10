//! The on-disk shape shared by `record`, `sanitize`, and `bench`.
//!
//! One recorded API response is one JSON file. The envelope keeps only what a
//! replay needs: the request identity (method, path, query), the status, the
//! four headers the client reads, and the decoded body. Response bodies are
//! never stored raw for the cache (SPEC §15); a fixture is a test artifact and
//! is stored only after `sanitize` has run over it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The tracked fixture root. `record` refuses to write anywhere under it.
pub const TRACKED_FIXTURES: &str = "crates/canvas-api/tests/fixtures";

/// The redaction rule set `sanitize` applies. Bump when the rules change so a
/// set recorded under older rules is visible as such in its manifest.
pub const REDACTION_VERSION: u32 = 1;

/// Response headers a fixture keeps. Everything else is dropped: an unknown
/// header can carry a session cookie or a capability.
pub const KEPT_HEADERS: [&str; 4] = ["date", "link", "x-rate-limit-remaining", "x-request-cost"];

/// One recorded API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recorded {
    /// Uppercase HTTP method.
    pub method: String,
    /// Path with the origin removed, `/api/v1/...`.
    pub path: String,
    /// Query pairs in the order sent.
    #[serde(default)]
    pub query: Vec<(String, String)>,
    /// HTTP status of the response.
    pub status: u16,
    /// Allowlisted response headers, lowercase keys.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Decoded JSON body.
    pub body: serde_json::Value,
    /// Page number when the response came from a paginated walk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

impl Recorded {
    /// Deterministic file name: `<method>-<path-slug>[-page-N].json`.
    ///
    /// Query pairs that distinguish two calls to the same path (every pair
    /// except the `per_page` and `include[]` boilerplate) join the slug, so
    /// `courses?enrollment_state=completed` cannot overwrite the active list.
    /// A slug longer than [`SLUG_MAX`] is truncated and closed with a hash of
    /// the full request, which keeps distinct requests in distinct files.
    pub fn file_name(&self) -> String {
        let mut name = slug(self.path.trim_start_matches("/api/v1/"));
        for (key, value) in &self.query {
            if key == "per_page" || key.starts_with("include[") {
                continue;
            }
            let _ = write!(name, "-{}-{}", slug(key), slug(value));
        }
        if name.len() > SLUG_MAX {
            let digest = Sha256::digest(self.request_key().as_bytes());
            name.truncate(SLUG_MAX - 9);
            let _ = write!(name, "-{:x}", Digest36(&digest[..4]));
        }
        let method = self.method.to_ascii_lowercase();
        match self.page {
            Some(page) if page > 1 => format!("{method}-{name}-page-{page}.json"),
            _ => format!("{method}-{name}.json"),
        }
    }

    /// Stable identity of the request, used for hashing and for replay lookup.
    pub fn request_key(&self) -> String {
        let mut key = format!("{} {}", self.method, self.path);
        let mut pairs: Vec<&(String, String)> = self.query.iter().collect();
        pairs.sort();
        for (name, value) in pairs {
            let _ = write!(key, "&{name}={value}");
        }
        key
    }

    /// The endpoint without its query, for the manifest listing.
    pub fn endpoint(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

const SLUG_MAX: usize = 80;

struct Digest36<'a>(&'a [u8]);

impl std::fmt::LowerHex for Digest36<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Lowercase a path into `a-z0-9_` runs joined by single dashes.
fn slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "root".to_owned()
    } else {
        out
    }
}

/// What a set records about itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Directory name of the set.
    pub set: String,
    /// When the underlying responses were recorded (or generated).
    pub recorded_at: String,
    /// Every endpoint in the set, sorted and deduplicated.
    pub endpoints: Vec<String>,
    /// Which redaction rules produced this set.
    pub redaction_version: u32,
    /// True when `bench` generated the set instead of recording it.
    #[serde(default)]
    pub synthetic: bool,
    /// Pseudonym mappings, so a second `sanitize` pass is a no-op.
    #[serde(default)]
    pub pseudonyms: Pseudonyms,
}

/// The pseudonyms a sanitized set uses.
///
/// Only the issued side is stored. A manifest that also held the real values
/// it replaced would put the identifying data straight back into the tracked
/// fixture, which is the opposite of what sanitizing is for. The issued lists
/// are enough for the one job they have: a value already in them is already a
/// pseudonym, so a second pass leaves it alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pseudonyms {
    /// Every pseudonym ID the set uses, sorted.
    #[serde(default)]
    pub issued_ids: Vec<i64>,
    /// Every pseudonym string the set uses, sorted.
    #[serde(default)]
    pub issued_strings: Vec<String>,
}

/// The name of the manifest inside a set.
pub const MANIFEST: &str = "MANIFEST.json";

/// Read every `*.json` response in a set, sorted by file name.
pub fn load_set(dir: &Path) -> Result<Vec<(String, Recorded)>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    let mut out = Vec::new();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if name == MANIFEST || path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let recorded: Recorded =
            serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        out.push((name, recorded));
    }
    if out.is_empty() {
        bail!("{} holds no recorded responses", dir.display());
    }
    Ok(out)
}

/// Read a set's manifest when it has one.
pub fn load_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let path = dir.join(MANIFEST);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&text)?))
}

/// Write one response, pretty-printed with a trailing newline so the tracked
/// fixtures stay reviewable in a diff.
pub fn write_recorded(dir: &Path, recorded: &Recorded) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(recorded.file_name());
    let mut text = serde_json::to_string_pretty(recorded)?;
    text.push('\n');
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// Write a set manifest.
pub fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut text = serde_json::to_string_pretty(manifest)?;
    text.push('\n');
    std::fs::write(dir.join(MANIFEST), text)?;
    Ok(())
}

/// True when `dir` is inside the tracked fixture root.
///
/// Only `sanitize` may write there (SPEC §15: fixtures pass through
/// `xtask sanitize`). The check compares canonical paths where it can, and
/// falls back to a textual suffix match for a directory that does not exist
/// yet, which is the normal case for a new set.
pub fn is_tracked_fixture_dir(dir: &Path) -> bool {
    let tracked = Path::new(TRACKED_FIXTURES);
    let canonical_root = std::fs::canonicalize(tracked).ok();
    let mut probe = dir.to_path_buf();
    loop {
        if let (Some(root), Ok(here)) = (canonical_root.as_ref(), std::fs::canonicalize(&probe))
            && here.starts_with(root)
        {
            return true;
        }
        if !probe.pop() {
            break;
        }
    }
    // Textual fallback: the target does not exist yet.
    let text = dir.to_string_lossy().replace('\\', "/");
    text.contains("canvas-api/tests/fixtures")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, query: &[(&str, &str)]) -> Recorded {
        Recorded {
            method: "GET".into(),
            path: path.into(),
            query: query
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::Value::Null,
            page: None,
        }
    }

    #[test]
    fn names_come_from_method_path_and_distinguishing_query() {
        assert_eq!(
            rec("/api/v1/users/self", &[]).file_name(),
            "get-users-self.json"
        );
        assert_eq!(
            rec(
                "/api/v1/courses/12/assignment_groups",
                &[("per_page", "100")]
            )
            .file_name(),
            "get-courses-12-assignment_groups.json"
        );
        // `per_page` and `include[]` are boilerplate; the rest separates files.
        let active = rec(
            "/api/v1/courses",
            &[("enrollment_state", "active"), ("include[]", "term")],
        );
        let done = rec("/api/v1/courses", &[("enrollment_state", "completed")]);
        assert_eq!(
            active.file_name(),
            "get-courses-enrollment_state-active.json"
        );
        assert_ne!(active.file_name(), done.file_name());
    }

    #[test]
    fn pages_after_the_first_carry_a_page_suffix() {
        let mut page = rec("/api/v1/courses", &[]);
        page.page = Some(1);
        assert_eq!(page.file_name(), "get-courses.json");
        page.page = Some(3);
        assert_eq!(page.file_name(), "get-courses-page-3.json");
    }

    #[test]
    fn a_long_query_truncates_but_stays_distinct() {
        let long = |v: &str| {
            rec(
                "/api/v1/planner/items",
                &[("start_date", v), ("end_date", "2026-10-01T00:00:00Z")],
            )
        };
        let a = long("2026-09-01T00:00:00Z");
        let b = long("2026-09-02T00:00:00Z");
        assert!(a.file_name().len() <= 80 + ".json".len());
        assert_ne!(a.file_name(), b.file_name());
    }

    #[test]
    fn the_tracked_fixture_root_is_recognized() {
        assert!(is_tracked_fixture_dir(Path::new(
            "crates/canvas-api/tests/fixtures/bench-5"
        )));
        assert!(is_tracked_fixture_dir(Path::new(TRACKED_FIXTURES)));
        assert!(!is_tracked_fixture_dir(Path::new("/tmp/recording")));
    }
}

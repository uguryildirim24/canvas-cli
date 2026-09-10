//! `cargo xtask sanitize --in DIR --out crates/canvas-api/tests/fixtures/<set>`.
//!
//! Two rules apply to a recorded set before it may be tracked (SPEC §11, §15):
//!
//! 1. **Redaction.** Every value under a key on the §11 list is dropped, in
//!    bodies, in query pairs, and in headers. The value is never inspected and
//!    never partially kept: guessing where an untrusted nested value ends can
//!    leak a capability.
//! 2. **Pseudonymization.** Identifying values are replaced by deterministic
//!    stand-ins with a stable mapping inside one set, so cross-references
//!    between files still resolve and a fixture still exercises real code
//!    paths.
//!
//! A second pass over sanitized output is a no-op. Two mechanisms give that:
//! the output manifest lists the pseudonyms the set uses, and a value in that
//! list maps to itself; and every pseudonym has a recognizable shape which is
//! its own fixed point even without a manifest. The manifest lists only the
//! issued pseudonyms, never the real values they replaced.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::fixture::{
    KEPT_HEADERS, Manifest, Pseudonyms, REDACTION_VERSION, Recorded, load_manifest, load_set,
    write_manifest, write_recorded,
};

/// Pseudonym IDs start here. The band is far above the IDs a Canvas instance
/// issues, so a value in it is already a pseudonym and passes through, which
/// is what makes a second pass idempotent even without a manifest.
pub const ID_BASE: i64 = 900_000_000;

/// The host every URL is rewritten to.
const HOST: &str = "canvas.example.edu";

/// Keys whose value is dropped outright (SPEC §11 redaction list).
pub fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "authorization"
            | "access_token"
            | "upload_params"
            | "signature"
            | "policy"
            | "expires"
            | "verifier"
            | "sig"
            | "token"
    ) || key.starts_with("x-amz-")
}

/// Keys that hold a person's or a thing's name.
fn is_name_key(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "short_name"
            | "sortable_name"
            | "display_name"
            | "full_name"
            | "user_name"
            | "author_name"
            | "title"
            | "course_code"
            | "friendly_name"
    )
}

/// Keys that hold free text: the body is replaced, the structure is kept.
fn is_text_key(key: &str) -> bool {
    matches!(
        key,
        "description"
            | "message"
            | "syllabus_body"
            | "body"
            | "summary"
            | "comment"
            | "public_description"
            | "text_comment"
    )
}

/// Keys that hold an identifier of a person outside Canvas.
fn is_login_key(key: &str) -> bool {
    matches!(
        key,
        "login_id" | "sis_user_id" | "sis_course_id" | "sis_section_id" | "integration_id"
    )
}

/// Keys that hold an ID. Canvas sends these as numbers or as strings.
fn is_id_key(key: &str) -> bool {
    key == "id" || key.ends_with("_id") || key.ends_with("_ids")
}

/// Keys that hold a URL.
fn is_url_key(key: &str) -> bool {
    key == "url" || key.ends_with("_url") || key == "href"
}

/// The mapping state for one set.
pub struct Sanitizer {
    ids: BTreeMap<String, i64>,
    strings: BTreeMap<String, String>,
    /// Every pseudonym this set uses, including the ones a previous pass
    /// issued. A value in here is its own mapping.
    issued_ids: BTreeSet<i64>,
    issued_strings: BTreeSet<String>,
    next_id: i64,
    next_string: u32,
}

impl Sanitizer {
    /// Start from the mappings a previous pass recorded, if any.
    pub fn new(previous: Option<&Pseudonyms>) -> Self {
        let mut issued_ids = BTreeSet::new();
        let mut issued_strings = BTreeSet::new();
        if let Some(previous) = previous {
            issued_ids.extend(previous.issued_ids.iter().copied());
            issued_strings.extend(previous.issued_strings.iter().cloned());
        }
        let next_id = issued_ids
            .iter()
            .copied()
            .max()
            .map_or(ID_BASE + 1, |m| m + 1);
        let next_string = u32::try_from(issued_strings.len()).unwrap_or(0) + 1;
        Self {
            ids: BTreeMap::new(),
            strings: BTreeMap::new(),
            issued_ids,
            issued_strings,
            next_id,
            next_string,
        }
    }

    /// The pseudonyms to store in the output manifest.
    pub fn pseudonyms(&self) -> Pseudonyms {
        Pseudonyms {
            issued_ids: self.issued_ids.iter().copied().collect(),
            issued_strings: self.issued_strings.iter().cloned().collect(),
        }
    }

    /// Map one ID. A value already in the pseudonym band maps to itself.
    fn id(&mut self, real: &str) -> i64 {
        if let Some(mapped) = self.ids.get(real) {
            return *mapped;
        }
        let parsed = real.parse::<i64>().ok();
        let assigned = match parsed {
            Some(parsed) if parsed >= ID_BASE || self.issued_ids.contains(&parsed) => {
                self.next_id = self.next_id.max(parsed + 1);
                parsed
            }
            _ => {
                let next = self.next_id;
                self.next_id += 1;
                next
            }
        };
        // The real value keys the run-local map only; it never reaches disk.
        self.ids.insert(real.to_owned(), assigned);
        self.issued_ids.insert(assigned);
        assigned
    }

    /// Map one identifying string under a pseudonym shape.
    fn string(&mut self, real: &str, shape: Shape) -> String {
        if let Some(mapped) = self.strings.get(real) {
            return mapped.clone();
        }
        let assigned = if shape.is_pseudonym(real) || self.issued_strings.contains(real) {
            real.to_owned()
        } else {
            let n = self.next_string;
            self.next_string += 1;
            shape.render(n, real)
        };
        self.strings.insert(real.to_owned(), assigned.clone());
        self.issued_strings.insert(assigned.clone());
        assigned
    }
}

/// The pseudonym forms, each its own fixed point.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Name,
    Email,
    Login,
    FileName,
    /// An identifier Canvas does not send as a number: an LTI ID, a UUID, an
    /// anonymous submission ID. It names a person or a record as surely as a
    /// numeric ID does, and no shape rule can tell one from a free string.
    Opaque,
}

impl Shape {
    fn is_pseudonym(self, value: &str) -> bool {
        match self {
            Self::Name => value
                .strip_prefix("Name ")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
            Self::Email => value
                .strip_prefix("user")
                .and_then(|rest| rest.strip_suffix("@example.invalid"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
            Self::Login => value
                .strip_prefix("user")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
            Self::FileName => {
                let stem = value.split('.').next().unwrap_or_default();
                stem.strip_prefix("file-")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            }
            Self::Opaque => value
                .strip_prefix("opaque-")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        }
    }

    fn render(self, n: u32, real: &str) -> String {
        match self {
            Self::Name => format!("Name {n}"),
            Self::Email => format!("user{n}@example.invalid"),
            Self::Login => format!("user{n}"),
            // A test that classifies by extension needs the extension kept.
            Self::FileName => match real.rsplit_once('.') {
                Some((_, ext)) if !ext.is_empty() && ext.len() <= 8 => format!("file-{n}.{ext}"),
                _ => format!("file-{n}"),
            },
            Self::Opaque => format!("opaque-{n}"),
        }
    }
}

/// Replace free text with a placeholder of the same length that keeps the
/// Markdown and HTML structure: punctuation, element names, and list markers
/// survive, letters become `x` and digits become `0`. Applying it twice
/// changes nothing, because `x` and `0` are already their own replacement.
///
/// Only the element *name* of an HTML tag survives, never its attributes. An
/// attribute value carries a URL, an e-mail address, or a name as readily as
/// the text around it does (`<a href="mailto:…">`, `<img src="…/users/77/…">`),
/// and none of those may reach a fixture (SPEC §15).
pub fn placeholder(input: &str) -> String {
    /// One character outside a tag name: letters to `x`, digits to `0`,
    /// everything else kept so the structure reads the same.
    fn masked(ch: char, first_of_word: &mut bool) -> char {
        if ch.is_alphabetic() {
            let out = if *first_of_word && ch.is_uppercase() {
                'X'
            } else {
                'x'
            };
            *first_of_word = false;
            out
        } else if ch.is_numeric() {
            *first_of_word = false;
            '0'
        } else {
            *first_of_word = true;
            ch
        }
    }

    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut in_tag_name = false;
    let mut first_of_word = true;
    for ch in input.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                in_tag_name = false;
                out.push(ch);
                first_of_word = true;
                continue;
            }
            // The name runs from `<` to the first character that cannot be
            // part of one; from there on the tag is attributes.
            if in_tag_name && (ch.is_ascii_alphanumeric() || ch == '/' || ch == '-') {
                out.push(ch);
                continue;
            }
            in_tag_name = false;
            out.push(masked(ch, &mut first_of_word));
            continue;
        }
        if ch == '<' {
            in_tag = true;
            in_tag_name = true;
            out.push(ch);
            continue;
        }
        out.push(masked(ch, &mut first_of_word));
    }
    out
}

/// Strip every capability from a value, pseudonymizing nothing.
///
/// Sanitizing needs a whole set and a stable mapping, so it can only run once
/// a recording is complete. Keeping a token or a signed storage URL off the
/// disk cannot wait that long (SPEC §15), so `record` runs this over every
/// body before it writes one.
pub fn redact_capabilities(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (name, child) in map {
                let child = if is_secret_key(name) {
                    Value::Null
                } else {
                    redact_capabilities(child)
                };
                out.insert(name.clone(), child);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_capabilities).collect()),
        Value::String(text) => Value::String(strip_capability_params(text)),
        other => other.clone(),
    }
}

/// Drop the §11 query parameters, and any userinfo, from one URL.
///
/// A string that is not an absolute `http(s)` URL, and a URL that carries
/// nothing on the list, come back byte for byte unchanged: re-encoding a URL
/// that needed no change would rewrite escapes a fixture is meant to preserve.
pub fn strip_capability_params(text: &str) -> String {
    let Ok(mut url) = url::Url::parse(text) else {
        return text.to_owned();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return text.to_owned();
    }
    let total = url.query_pairs().count();
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_secret_key(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let credentials = !url.username().is_empty() || url.password().is_some();
    if kept.len() == total && !credentials {
        return text.to_owned();
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    set_query_pairs(&mut url, kept);
    url.to_string()
}

/// Replace a URL's query with `pairs`, dropping it entirely when empty.
fn set_query_pairs(url: &mut url::Url, pairs: Vec<(String, String)>) {
    if pairs.is_empty() {
        url.set_query(None);
        return;
    }
    let mut query = url.query_pairs_mut();
    query.clear();
    for (key, value) in pairs {
        query.append_pair(&key, &value);
    }
    drop(query);
}

/// Rewrite a URL: drop every capability-bearing query pair, move it to the
/// pseudonym host, and map the IDs in its path.
fn sanitize_url(raw: &str, state: &mut Sanitizer) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        // A relative URL still carries IDs in its path.
        return map_path_ids(raw, state);
    };
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_secret_key(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let path = map_path_ids(url.path(), state);
    let _ = url.set_host(Some(HOST));
    let _ = url.set_port(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    if url.scheme() == "http" {
        let _ = url.set_scheme("https");
    }
    url.set_path(&path);
    url.set_fragment(None);
    set_query_pairs(&mut url, kept);
    url.to_string()
}

/// Map every numeric segment of a path through the ID mapping.
fn map_path_ids(path: &str, state: &mut Sanitizer) -> String {
    path.split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit()) {
                state.id(segment).to_string()
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Map one ID value, keeping the JSON type Canvas used.
///
/// A string that is not all digits is still an identifier — `lti_user_id`,
/// `anonymous_id`, a UUID — and used to pass through untouched. It goes
/// through its own pseudonym shape instead. An object under an ID key is not
/// an ID at all, so it goes back through the ordinary walk rather than being
/// copied whole.
fn sanitize_id(value: &Value, state: &mut Sanitizer) -> Value {
    match value {
        Value::Number(n) => Value::from(state.id(&n.to_string())),
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            Value::String(state.id(s).to_string())
        }
        Value::String(s) if !s.is_empty() => Value::String(state.string(s, Shape::Opaque)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| sanitize_id(item, state)).collect())
        }
        Value::Object(_) => sanitize_value(None, value, state),
        other => other.clone(),
    }
}

/// Sanitize one JSON value. `key` is the name the value arrived under.
fn sanitize_value(key: Option<&str>, value: &Value, state: &mut Sanitizer) -> Value {
    if let Some(key) = key {
        if is_secret_key(key) {
            return Value::Null;
        }
        let lower = key.to_ascii_lowercase();
        // An ID key wins over the shape rules: `course_id` is a number even
        // when Canvas sends it as a string. A login or SIS key also ends in
        // `_id` but names a person, so it keeps its own rule.
        if is_id_key(&lower) && !is_url_key(&lower) && !is_login_key(&lower) {
            return sanitize_id(value, state);
        }
        match value {
            Value::String(s) if !s.is_empty() => {
                if is_url_key(&lower) {
                    return Value::String(sanitize_url(s, state));
                }
                if lower == "filename" || (lower == "display_name" && looks_like_file(s)) {
                    return Value::String(state.string(s, Shape::FileName));
                }
                if lower == "email" || lower == "primary_email" || looks_like_email(s) {
                    return Value::String(state.string(s, Shape::Email));
                }
                if is_login_key(&lower) {
                    return Value::String(state.string(s, Shape::Login));
                }
                if is_name_key(&lower) {
                    return Value::String(state.string(s, Shape::Name));
                }
                if is_text_key(&lower) {
                    return Value::String(placeholder(s));
                }
            }
            _ => {}
        }
    }
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (name, child) in map {
                out.insert(name.clone(), sanitize_value(Some(name), child, state));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| sanitize_value(key, item, state))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn looks_like_email(value: &str) -> bool {
    let mut parts = value.splitn(2, '@');
    matches!((parts.next(), parts.next()), (Some(local), Some(domain))
        if !local.is_empty() && domain.contains('.') && !domain.contains(' '))
}

fn looks_like_file(value: &str) -> bool {
    value
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && (1..=8).contains(&ext.len()))
}

/// Sanitize one recorded response in place.
fn sanitize_recorded(recorded: &Recorded, state: &mut Sanitizer) -> Recorded {
    let path = map_path_ids(&recorded.path, state);
    let query = recorded
        .query
        .iter()
        .filter(|(k, _)| !is_secret_key(k))
        .map(|(k, v)| {
            let value = if is_id_key(&k.to_ascii_lowercase())
                && !v.is_empty()
                && v.bytes().all(|b| b.is_ascii_digit())
            {
                state.id(v).to_string()
            } else {
                v.clone()
            };
            (k.clone(), value)
        })
        .collect();
    let headers = recorded
        .headers
        .iter()
        .filter(|(name, _)| {
            let lower = name.to_ascii_lowercase();
            KEPT_HEADERS.contains(&lower.as_str()) && !is_secret_key(&lower)
        })
        .map(|(name, value)| {
            let lower = name.to_ascii_lowercase();
            let value = if lower == "link" {
                sanitize_link(value, state)
            } else {
                value.clone()
            };
            (lower, value)
        })
        .collect();
    Recorded {
        method: recorded.method.clone(),
        path,
        query,
        status: recorded.status,
        headers,
        body: sanitize_value(None, &recorded.body, state),
        page: recorded.page,
    }
}

/// A `Link` header is a list of `<url>; rel="x"` parts; each URL is rewritten.
fn sanitize_link(value: &str, state: &mut Sanitizer) -> String {
    value
        .split(',')
        .map(|part| {
            let part = part.trim();
            match (part.find('<'), part.find('>')) {
                (Some(start), Some(end)) if end > start => {
                    let url = sanitize_url(&part[start + 1..end], state);
                    format!("<{url}>{}", &part[end + 1..])
                }
                _ => part.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Run the sanitize task.
pub fn run(input: &Path, output: &Path) -> Result<Vec<String>> {
    if input == output {
        bail!("--in and --out must differ; sanitize never edits a set in place");
    }
    let entries = load_set(input)?;
    let previous = load_manifest(input)?;
    let mut state = Sanitizer::new(previous.as_ref().map(|m| &m.pseudonyms));

    let mut sanitized = Vec::with_capacity(entries.len());
    for (_, recorded) in &entries {
        sanitized.push(sanitize_recorded(recorded, &mut state));
    }

    std::fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
    // A rerun into a populated directory must not leave stale files behind.
    for entry in std::fs::read_dir(output)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            std::fs::remove_file(path)?;
        }
    }

    let mut names = Vec::with_capacity(sanitized.len());
    let mut endpoints = BTreeSet::new();
    for recorded in &sanitized {
        endpoints.insert(recorded.endpoint());
        let path = write_recorded(output, recorded)?;
        names.push(
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned(),
        );
    }

    let manifest = Manifest {
        set: output
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("set")
            .to_owned(),
        recorded_at: previous.as_ref().map_or_else(
            || jiff::Timestamp::now().to_string(),
            |m| m.recorded_at.clone(),
        ),
        endpoints: endpoints.into_iter().collect(),
        redaction_version: REDACTION_VERSION,
        synthetic: previous.as_ref().is_some_and(|m| m.synthetic),
        pseudonyms: state.pseudonyms(),
    };
    write_manifest(output, &manifest)?;
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> Sanitizer {
        Sanitizer::new(None)
    }

    #[test]
    fn every_secret_key_loses_its_value() {
        let mut s = state();
        let body = json!({
            "Authorization": "Bearer abc",
            "access_token": "abc",
            "upload_params": {"key": "v", "x-amz-credential": "c"},
            "verifier": "v1",
            "nested": {"sig": "s", "Policy": "p", "Expires": "1", "token": "t"},
            "X-Amz-Signature": "deadbeef"
        });
        let out = sanitize_value(None, &body, &mut s);
        let text = out.to_string();
        for leak in ["Bearer", "abc", "deadbeef", "v1", "\"s\"", "\"p\"", "\"t\""] {
            assert!(!text.contains(leak), "{leak} survived in {text}");
        }
        assert_eq!(out["upload_params"], Value::Null);
        assert_eq!(out["nested"]["sig"], Value::Null);
    }

    #[test]
    fn an_id_that_is_not_a_number_is_still_pseudonymized() {
        let mut s = state();
        let out = sanitize_value(
            None,
            &json!({
                "id": 4321,
                "lti_user_id": "5c9d8f1a2b3c4d5e6f708192a3b4c5d6",
                "anonymous_id": "z1B9",
                // Not an ID at all; the ordinary walk still has to reach it.
                "custom_id": {"name": "Ada Lovelace"}
            }),
            &mut s,
        );
        let text = out.to_string();
        for leak in ["5c9d8f1a", "z1B9", "Ada Lovelace"] {
            assert!(!text.contains(leak), "{leak} survived in {text}");
        }
        // Keys are visited in sorted order: anonymous_id, custom_id, id,
        // lti_user_id.
        assert_eq!(out["anonymous_id"], json!("opaque-1"));
        assert_eq!(out["custom_id"]["name"], json!("Name 2"));
        assert_eq!(out["id"], json!(ID_BASE + 1));
        assert_eq!(out["lti_user_id"], json!("opaque-3"));
        // Stable inside the set, and its own fixed point on a second pass.
        let again = sanitize_value(None, &out, &mut s);
        assert_eq!(again, out);
    }

    #[test]
    fn ids_map_stably_and_keep_their_json_type() {
        let mut s = state();
        let a = sanitize_value(None, &json!({"id": 4321, "course_id": "4321"}), &mut s);
        let b = sanitize_value(None, &json!({"course_id": 4321}), &mut s);
        assert_eq!(a["id"], json!(ID_BASE + 1));
        // Same real ID, same pseudonym, and the string stayed a string.
        assert_eq!(a["course_id"], json!((ID_BASE + 1).to_string()));
        assert_eq!(b["course_id"], json!(ID_BASE + 1));
        // A different ID gets a different pseudonym.
        let c = sanitize_value(None, &json!({"id": 9}), &mut s);
        assert_eq!(c["id"], json!(ID_BASE + 2));
    }

    #[test]
    fn names_emails_logins_and_urls_become_pseudonyms() {
        let mut s = state();
        let out = sanitize_value(
            None,
            &json!({
                "name": "Ada Lovelace",
                "sortable_name": "Lovelace, Ada",
                "login_id": "alovelace",
                "primary_email": "ada@lasell.edu",
                "avatar_url": "https://canvas.real.edu/images/thumbnails/9/abc?token=t",
                "url": "https://files.real.edu/courses/7/files/33/download?verifier=cap&download=1"
            }),
            &mut s,
        );
        // Keys are visited in sorted order: avatar_url, login_id, name,
        // primary_email, sortable_name, url.
        assert_eq!(out["login_id"], json!("user1"));
        assert_eq!(out["name"], json!("Name 2"));
        assert_eq!(out["primary_email"], json!("user3@example.invalid"));
        assert_eq!(out["sortable_name"], json!("Name 4"));
        let avatar = out["avatar_url"].as_str().unwrap();
        assert!(
            avatar.starts_with("https://canvas.example.edu/"),
            "{avatar}"
        );
        assert!(!avatar.contains("token"));
        let file = out["url"].as_str().unwrap();
        assert!(!file.contains("verifier"), "{file}");
        assert!(file.contains("download=1"), "{file}");
        // The course and file IDs inside the URL went through the ID mapping.
        assert!(!file.contains("/courses/7/"), "{file}");
        assert!(!file.contains("/files/33"), "{file}");
        assert_eq!(
            file,
            format!(
                "https://canvas.example.edu/courses/{}/files/{}/download?download=1",
                ID_BASE + 2,
                ID_BASE + 3
            )
        );
    }

    #[test]
    fn free_text_keeps_its_structure_and_its_length() {
        let input = "## Week 3\n\nRead *chapter 4* and <b>submit</b> by Friday.";
        let out = placeholder(input);
        assert_eq!(out.chars().count(), input.chars().count());
        assert!(out.starts_with("## Xxxx 0\n\nXxxx *xxxxxxx 0*"), "{out}");
        assert!(out.contains("<b>"), "{out}");
        assert!(out.ends_with("</b> xx Xxxxxx."), "{out}");
        // Idempotent.
        assert_eq!(placeholder(&out), out);
    }

    #[test]
    fn free_text_keeps_element_names_but_never_attributes() {
        let input = "<p>Ask <a href=\"mailto:ada@lasell.edu\" title=\"Ada\">Ada</a> \
                     or see <img src=\"https://canvas.real.edu/users/77/avatar.png\"/>.</p>";
        let out = placeholder(input);
        assert_eq!(out.chars().count(), input.chars().count());
        // The structure a test relies on survives.
        for kept in ["<p>", "<a ", "</a>", "<img ", "/>", "</p>"] {
            assert!(out.contains(kept), "{kept} lost from {out}");
        }
        // Nothing identifying does.
        for leak in [
            "mailto",
            "ada",
            "lasell",
            "Ada",
            "canvas.real.edu",
            "avatar",
            "77",
        ] {
            assert!(!out.contains(leak), "{leak} survived in {out}");
        }
        assert_eq!(placeholder(&out), out);
    }

    #[test]
    fn a_markdown_link_target_does_not_survive_free_text() {
        let out = placeholder("See [the syllabus](https://canvas.real.edu/courses/77).");
        assert!(!out.contains("canvas.real.edu"), "{out}");
        assert!(!out.contains("77"), "{out}");
        // The Markdown link structure is still there.
        assert!(
            out.contains('[') && out.contains("](") && out.contains(')'),
            "{out}"
        );
    }

    #[test]
    fn a_second_pass_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw");
        let once = dir.path().join("once");
        let twice = dir.path().join("twice");
        let recorded = Recorded {
            method: "GET".into(),
            path: "/api/v1/courses/4321/files".into(),
            query: vec![("per_page".into(), "100".into())],
            status: 200,
            headers: BTreeMap::from([(
                "link".into(),
                "<https://canvas.real.edu/api/v1/courses/4321/files?page=2>; rel=\"next\"".into(),
            )]),
            body: json!([{
                "id": 33, "course_id": 4321, "display_name": "Syllabus.pdf",
                "url": "https://files.real.edu/files/33?verifier=cap",
                "user": {"id": 7, "name": "Ada Lovelace", "login_id": "alovelace"},
                "description": "Read chapter 4."
            }]),
            page: None,
        };
        write_recorded(&raw, &recorded).unwrap();

        let first = run(&raw, &once).unwrap();
        let second = run(&once, &twice).unwrap();
        assert_eq!(first, second);
        for name in &first {
            let a = std::fs::read_to_string(once.join(name)).unwrap();
            let b = std::fs::read_to_string(twice.join(name)).unwrap();
            assert_eq!(a, b, "{name} changed on the second pass");
        }
        let manifest_a = std::fs::read_to_string(once.join(crate::fixture::MANIFEST)).unwrap();
        let manifest_b = std::fs::read_to_string(twice.join(crate::fixture::MANIFEST)).unwrap();
        // Only the set name differs; the mappings and the record date carry over.
        let mut a: Manifest = serde_json::from_str(&manifest_a).unwrap();
        let b: Manifest = serde_json::from_str(&manifest_b).unwrap();
        a.set = b.set.clone();
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
    }

    #[test]
    fn the_manifest_lists_the_set_date_endpoints_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw");
        let out = dir.path().join("fixtures").join("demo");
        for path in ["/api/v1/users/self", "/api/v1/courses"] {
            write_recorded(
                &raw,
                &Recorded {
                    method: "GET".into(),
                    path: path.into(),
                    query: vec![],
                    status: 200,
                    headers: BTreeMap::new(),
                    body: json!({"id": 1}),
                    page: None,
                },
            )
            .unwrap();
        }
        run(&raw, &out).unwrap();
        let manifest = load_manifest(&out).unwrap().unwrap();
        assert_eq!(manifest.set, "demo");
        assert_eq!(manifest.redaction_version, REDACTION_VERSION);
        assert_eq!(
            manifest.endpoints,
            vec!["GET /api/v1/courses", "GET /api/v1/users/self"]
        );
        assert!(manifest.recorded_at.parse::<jiff::Timestamp>().is_ok());
    }
}

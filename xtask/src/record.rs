//! `cargo xtask record --host ORIGIN --out DIR [--course ID ...]`.
//!
//! Walks the Appendix B endpoint list for the signed-in identity and stores
//! each response as a [`Recorded`] envelope.
//!
//! **Recording from a real account is not approved yet** (SPEC §19 item 5).
//! The tool exists so the approval, when it comes, needs no new code; its
//! tests prove it against `wiremock`.
//!
//! Three rules hold whatever the server sends:
//!
//! - The token never reaches the disk. The request headers are never written,
//!   and the four kept response headers cannot carry it.
//! - A URL that carries a capability (`verifier`, an S3 signature) is never
//!   followed and never stored: `record` walks `/api/v1` only, and every body,
//!   header, and query pair goes through
//!   [`redact_capabilities`](crate::sanitize::redact_capabilities) before it
//!   reaches the disk. Pseudonymizing waits for `sanitize`, which needs the
//!   whole set; dropping a capability cannot wait that long.
//! - The tracked fixture directory is refused. Only `xtask sanitize` writes
//!   there (SPEC §15).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use canvas_api::{ApiRequest, Client, Secret};
use serde_json::Value;

use crate::fixture::{KEPT_HEADERS, Recorded, is_tracked_fixture_dir, write_recorded};
use crate::sanitize::{is_secret_key, redact_capabilities, strip_capability_params};

/// How many pages one endpoint may contribute, so a large account cannot turn
/// a recording into an unbounded crawl.
const MAX_PAGES: u32 = 10;

/// Arguments of the record task.
pub struct Args {
    pub host: String,
    pub out: PathBuf,
    pub courses: Vec<i64>,
}

/// The Appendix B endpoints that need no course ID.
fn account_endpoints() -> Vec<(String, bool)> {
    let (start, end) = planner_window();
    vec![
        ("/api/v1/users/self".to_owned(), false),
        (
            "/api/v1/courses?enrollment_type=student&enrollment_state=active&include[]=term\
             &include[]=total_scores&include[]=current_grading_period_scores&include[]=favorites\
             &per_page=100"
                .to_owned(),
            true,
        ),
        (
            "/api/v1/courses?enrollment_type=student&enrollment_state=completed&include[]=term\
             &include[]=total_scores&per_page=100"
                .to_owned(),
            true,
        ),
        (
            format!("/api/v1/planner/items?start_date={start}&end_date={end}&per_page=100"),
            true,
        ),
        (
            "/api/v1/users/self/missing_submissions?include[]=planner_overrides&include[]=course\
             &per_page=100"
                .to_owned(),
            true,
        ),
        (
            "/api/v1/users/self/enrollments?type[]=StudentEnrollment&state[]=active\
             &state[]=completed&per_page=100"
                .to_owned(),
            true,
        ),
    ]
}

/// The Appendix B endpoints scoped to one course.
fn course_endpoints(id: i64) -> Vec<(String, bool)> {
    vec![
        (
            format!(
                "/api/v1/courses/{id}?include[]=term&include[]=syllabus_body&include[]=teachers\
                 &include[]=total_scores&include[]=current_grading_period_scores"
            ),
            false,
        ),
        (
            format!("/api/v1/courses/{id}/assignments?include[]=submission&per_page=100"),
            true,
        ),
        (
            format!(
                "/api/v1/courses/{id}/assignment_groups?include[]=assignments\
                 &include[]=submission&override_assignment_dates=true&per_page=100"
            ),
            true,
        ),
        (
            format!("/api/v1/courses/{id}/grading_periods?per_page=100"),
            true,
        ),
        (format!("/api/v1/courses/{id}/folders?per_page=100"), true),
        (format!("/api/v1/courses/{id}/files?per_page=100"), true),
        (
            format!(
                "/api/v1/courses/{id}/modules?include[]=items&include[]=content_details\
                 &per_page=100"
            ),
            true,
        ),
    ]
}

/// A two-week window around today, the same shape `todo` asks for.
fn planner_window() -> (String, String) {
    let now = jiff::Timestamp::now();
    let day = jiff::SignedDuration::from_hours(24);
    let start = (now - day * 7).to_string();
    let end = (now + day * 14).to_string();
    (start, end)
}

/// Run the record task.
pub async fn run(args: &Args) -> Result<Vec<PathBuf>> {
    if is_tracked_fixture_dir(&args.out) {
        bail!(
            "refusing to record into the tracked fixture directory {}; \
             record into a scratch directory, then `cargo xtask sanitize --in <scratch> --out {}/<set>`",
            args.out.display(),
            crate::fixture::TRACKED_FIXTURES
        );
    }
    let origin: url::Url = args
        .host
        .parse()
        .with_context(|| format!("parse --host {}", args.host))?;
    let token = resolve_token(&args.host)?;
    let agent = format!(
        "{}/{} (xtask record)",
        canvas_cli::dist::BIN_NAME,
        canvas_cli::dist::VERSION
    );
    let client = Client::new(origin.clone(), Secret::new(token), &agent)?;

    let mut plan = account_endpoints();
    for id in &args.courses {
        plan.extend(course_endpoints(*id));
    }

    let mut written = Vec::new();
    for (path, paginated) in plan {
        match record_endpoint(&client, &origin, &path, paginated, &args.out).await {
            Ok(paths) => written.extend(paths),
            // One denied endpoint (a course without files, say) must not end
            // the walk; the set records what the account can actually see.
            Err(e) => eprintln!("skipped {path}: {e:#}"),
        }
    }
    if written.is_empty() {
        bail!("recorded nothing; check --host and the token");
    }
    Ok(written)
}

/// Record one endpoint and, when it is a collection, its `Link` pages.
async fn record_endpoint(
    client: &Client,
    origin: &url::Url,
    path: &str,
    paginated: bool,
    out: &Path,
) -> Result<Vec<PathBuf>> {
    let mut url = origin.join(path)?;
    let mut written = Vec::new();
    for page in 1..=MAX_PAGES {
        let request = ApiRequest::new(reqwest::Method::GET, url.clone());
        let (body, headers) = client
            .send_api_with_headers::<Value>(request)
            .await
            .with_context(|| format!("GET {url}"))?;
        // The next page is taken from the response as it arrived; only the
        // stored copy is redacted.
        let next = headers
            .get("link")
            .and_then(|value| value.to_str().ok())
            .and_then(next_link);
        let recorded = Recorded {
            method: "GET".to_owned(),
            path: url.path().to_owned(),
            query: url
                .query_pairs()
                .filter(|(k, _)| !is_secret_key(k))
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            status: 200,
            headers: kept_headers(&headers),
            body: redact_capabilities(&body),
            page: paginated.then_some(page),
        };
        written.push(write_recorded(out, &recorded)?);

        match next {
            Some(next) if paginated => {
                let next: url::Url = next.parse()?;
                // Same-origin only; the client enforces this too (SPEC §11).
                if next.origin() != origin.origin() {
                    bail!("cross-origin next link");
                }
                url = next;
            }
            _ => break,
        }
    }
    Ok(written)
}

/// Keep only the four headers the client reads, lowercase.
///
/// `Link` is the one kept header that carries a URL, so it is the one that can
/// carry a capability; every URL in it is stripped before it is stored.
fn kept_headers(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in KEPT_HEADERS {
        if let Some(value) = headers.get(name)
            && let Ok(text) = value.to_str()
        {
            let text = if name == "link" {
                strip_link_capabilities(text)
            } else {
                text.to_owned()
            };
            out.insert(name.to_owned(), text);
        }
    }
    out
}

/// Strip the capability parameters from every URL in a `Link` header.
fn strip_link_capabilities(header: &str) -> String {
    header
        .split(',')
        .map(|part| {
            let part = part.trim();
            match (part.find('<'), part.find('>')) {
                (Some(start), Some(end)) if end > start => {
                    let url = strip_capability_params(&part[start + 1..end]);
                    format!("<{url}>{}", &part[end + 1..])
                }
                _ => part.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `rel="next"` URL of a `Link` header.
fn next_link(header: &str) -> Option<String> {
    for part in header.split(',') {
        let part = part.trim();
        // A part this does not understand is skipped, not fatal: a `rel="next"`
        // later in the same header is still the page to follow.
        let Some((url, rest)) = part.strip_prefix('<').and_then(|p| p.split_once('>')) else {
            continue;
        };
        if rest.contains("rel=\"next\"") || rest.contains("rel=next") {
            return Some(url.to_owned());
        }
    }
    None
}

/// The token, from `CANVAS_TOKEN` or from the credential store.
///
/// The store is read through the `canvas` binary's own `auth token` command
/// rather than through a second copy of the keyring and file-store logic:
/// one implementation of a secret read is easier to keep correct than two.
fn resolve_token(host: &str) -> Result<String> {
    if let Ok(token) = std::env::var("CANVAS_TOKEN")
        && !token.is_empty()
    {
        return Ok(token);
    }
    let output = std::process::Command::new("canvas")
        .args(["auth", "token", "--reveal"])
        .env("CANVAS_HOST", host)
        .output()
        .context("no CANVAS_TOKEN, and `canvas auth token --reveal` did not run")?;
    if !output.status.success() {
        bail!(
            "no CANVAS_TOKEN, and `canvas auth token --reveal` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let token = String::from_utf8(output.stdout)?.trim().to_owned();
    if token.is_empty() {
        bail!("the credential store returned an empty token");
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_link_is_found_and_other_relations_are_not() {
        let header = "<https://c.test/api/v1/courses?page=1>; rel=\"current\", \
                      <https://c.test/api/v1/courses?page=2>; rel=\"next\", \
                      <https://c.test/api/v1/courses?page=9>; rel=\"last\"";
        assert_eq!(
            next_link(header).as_deref(),
            Some("https://c.test/api/v1/courses?page=2")
        );
        assert_eq!(
            next_link("<https://c.test/api/v1/courses?page=9>; rel=\"last\""),
            None
        );
    }

    #[test]
    fn the_plan_covers_the_appendix_b_endpoints() {
        let account: Vec<String> = account_endpoints().into_iter().map(|(p, _)| p).collect();
        for needle in [
            "/api/v1/users/self",
            "enrollment_state=active",
            "planner/items",
            "missing_submissions",
            "users/self/enrollments",
        ] {
            assert!(
                account.iter().any(|p| p.contains(needle)),
                "{needle} missing"
            );
        }
        let course: Vec<String> = course_endpoints(7).into_iter().map(|(p, _)| p).collect();
        for needle in [
            "assignment_groups",
            "grading_periods",
            "/folders",
            "/files",
            "/modules",
            "/assignments",
        ] {
            assert!(
                course.iter().any(|p| p.contains(needle)),
                "{needle} missing"
            );
        }
    }
}

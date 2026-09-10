//! `canvas pages`, `canvas page`, and `canvas syllabus` (class C, M8-a).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::markdown::{BodyRefs, ResolvedRefs};
use canvas_core::resolve::canvas_url;
use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{
    PageDetailDataset, PagesDataset, RefreshOutcome, listing_denial_status, refresh_page,
    refresh_pages,
};
use comfy_table::Row;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{
    RefreshFail, cached_outcome, cached_outcome_with_error, outcome_freshness,
};
use super::emit::{base_envelope, emit, emit_error, session_error};
use crate::output::{
    EmbeddedJson, ExternalLinkJson, FileRefJson, FilesListingJson, Outcome, PageDetailJson,
    PageResult, PageSummaryJson, PagesResult, PartialScope, SCHEMA_PAGE, SCHEMA_PAGES,
    SCHEMA_SYLLABUS, SyllabusResult, apply_two_space_padding, new_table, now_timestamp,
};
use crate::session::{Session, ttl_pages};

/// Run `canvas pages <course> [--unpublished]`.
///
/// The listing is one dataset for the whole course; `--unpublished` widens
/// what is shown, and never changes what is fetched.
pub async fn run_list(globals: &Globals, course: String, unpublished: bool) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let outcome = match ensure_pages(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let mut rows = match session
        .open
        .store
        .call(move |conns| load_pages(conns, resolved.id))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return local_error(globals, &session, &e),
    };
    if !unpublished {
        rows.retain(|row| row.published != Some(false));
    }

    let denial = outcome.error.as_deref().and_then(listing_denial_status);
    let result = PagesResult {
        course_id: resolved.id.to_string(),
        listing: FilesListingJson {
            available: denial.is_none(),
            http_status: denial,
        },
        pages: rows,
    };
    let mut envelope = base_envelope(SCHEMA_PAGES, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale pages cache".into());
    }
    if let Some(status) = denial {
        let message = format!("Pages listing unavailable (HTTP {status})");
        envelope.partial.push(PartialScope {
            scope: format!("pages:course:{}", resolved.id),
            http_status: Some(status),
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    emit(globals.json, &envelope, || {
        print_page_table(&envelope.result.pages)
    })
}

/// Run `canvas page <course> <url-slug|id|URL>`.
pub async fn run_show(globals: &Globals, course: String, page: String) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let operand = match page_operand(&page, &session.identity.origin, resolved.id) {
        Ok(operand) => operand,
        Err(message) => {
            return emit_error(
                globals.json,
                "resolution",
                message,
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    let outcome = match ensure_page(globals, &session, resolved.id, &operand).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let scope = outcome.freshness.scope.clone();
    let row = match session
        .open
        .store
        .call(move |conns| load_page(conns, &scope))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                globals.json,
                "resolution",
                &format!("page {page} not found in course {}", resolved.id),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return local_error(globals, &session, &e),
    };

    let rich = canvas_core::markdown::rich_text_opt(row.body.as_deref())
        .await
        .unwrap_or_default();
    let refs = rich.refs.resolve(&session.identity.origin);
    let detail = PageDetailJson {
        id: row.id.to_string(),
        course_id: row
            .course_id
            .map_or_else(|| resolved.id.to_string(), |id| id.to_string()),
        title: row.title.clone(),
        url: row.url.clone(),
        updated_at: row.updated_at.clone(),
        published: json_bool(&row.data, "published"),
        front_page: json_bool(&row.data, "front_page"),
        locked_for_user: json_bool(&row.data, "locked_for_user"),
        html_url: json_string(&row.data, "html_url"),
        body_markdown: rich.markdown.clone(),
        truncated: rich.refs.truncated,
        embedded: embedded_json(&refs),
        files: files_json(&refs),
        external_links: external_json(&refs),
    };

    let mut envelope = base_envelope(SCHEMA_PAGE, &session, PageResult { page: detail });
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope.warnings.push("served stale page cache".into());
    }
    truncation_partial(
        &mut envelope,
        rich.refs.truncated,
        &format!("page:{}", row.id),
    );

    emit(globals.json, &envelope, || {
        print_page(&envelope.result.page)
    })
}

/// Run `canvas syllabus <course>`.
///
/// No new request: `course` already carries the syllabus body.
pub async fn run_syllabus(globals: &Globals, course: String) -> ExitCode {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(globals.json, e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };
    let detail = match super::course::ensure_detail(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(globals, &session, e),
    };
    freshness.push(outcome_freshness(&detail));

    let row = match session
        .open
        .store
        .call(move |conns| super::course_load::load_course_by_id(conns, resolved.id))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                globals.json,
                "resolution",
                &format!("course {} not found", resolved.id),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return local_error(globals, &session, &e),
    };

    // The cache keeps the Markdown and its reference projection, never the
    // source HTML; a cache written before M8-a has no projection, and then the
    // reference lists are empty instead of wrong.
    let stored: BodyRefs = row.syllabus_refs();
    let refs = stored.resolve(&session.identity.origin);

    let result = SyllabusResult {
        course_id: resolved.id.to_string(),
        syllabus_markdown: row.syllabus_markdown(),
        truncated: stored.truncated,
        embedded: embedded_json(&refs),
        files: files_json(&refs),
        external_links: external_json(&refs),
        updated_at: row.updated_at(),
    };
    let mut envelope = base_envelope(SCHEMA_SYLLABUS, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if detail.freshness.stale {
        envelope.warnings.push("served stale course cache".into());
    }
    truncation_partial(
        &mut envelope,
        stored.truncated,
        &format!("syllabus:course:{}", resolved.id),
    );

    emit(globals.json, &envelope, || print_syllabus(&envelope.result))
}

/// A body cut at the size bound is a partial answer, never a complete one.
pub(crate) fn truncation_partial<T>(
    envelope: &mut crate::output::Envelope<T>,
    truncated: bool,
    scope: &str,
) {
    if !truncated {
        return;
    }
    let message = format!(
        "body truncated at {} bytes; the text is not complete",
        canvas_core::markdown::BODY_LIMIT
    );
    envelope.partial.push(PartialScope {
        scope: scope.to_owned(),
        http_status: None,
        message: message.clone(),
    });
    envelope.warnings.push(message);
    envelope.outcome = Outcome::Partial;
    envelope.exit = 12;
}

pub(crate) fn embedded_json(refs: &ResolvedRefs) -> Vec<EmbeddedJson> {
    refs.embedded
        .iter()
        .map(|e| EmbeddedJson {
            kind: e.kind.clone(),
            src_origin: e.src_origin.clone(),
            reported: "unavailable".to_owned(),
        })
        .collect()
}

pub(crate) fn files_json(refs: &ResolvedRefs) -> Vec<FileRefJson> {
    refs.files
        .iter()
        .map(|f| FileRefJson {
            file_id: f.file_id.clone(),
            name: f.name.clone(),
            url: f.url.clone(),
        })
        .collect()
}

pub(crate) fn external_json(refs: &ResolvedRefs) -> Vec<ExternalLinkJson> {
    refs.external_links
        .iter()
        .map(|l| ExternalLinkJson { url: l.url.clone() })
        .collect()
}

/// The operand Canvas addresses the page by: a slug, an id, or a Canvas URL.
fn page_operand(raw: &str, origin: &str, course_id: i64) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("page operand must not be empty");
    }
    match canvas_url(trimmed, origin) {
        Err(_) => Err("url origin does not match the active identity"),
        Ok(Some(url)) => {
            let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
            let Some(window) = parts
                .windows(4)
                .find(|p| p[0] == "courses" && p[2] == "pages")
            else {
                return Err("url is not a Canvas course page");
            };
            if window[1].parse::<i64>() != Ok(course_id) {
                return Err("url names a different course than the course operand");
            }
            Ok(window[3].to_owned())
        }
        Ok(None) => Ok(trimmed.to_owned()),
    }
}

async fn ensure_pages(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_pages();
    let ds = PagesDataset::new(course_id, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome_with_error(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_pages(
        client,
        &session.open.store,
        course_id,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

async fn ensure_page(
    globals: &Globals,
    session: &Session,
    course_id: i64,
    operand: &str,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_pages();
    let ds = PageDetailDataset::new(course_id, operand, ttl);
    let lookup = session
        .open
        .store
        .call(move |conns| lookup_dataset(conns, &ds, now, None))
        .await
        .map_err(RefreshFail::Db)?;
    if let Some(outcome) = cached_outcome(lookup, globals.fresh, globals.offline)? {
        return Ok(outcome);
    }
    session
        .validate_network_token()
        .await
        .map_err(RefreshFail::Sync)?;
    let client = session.client.as_ref().ok_or(RefreshFail::NeedAuth)?;
    refresh_page(
        client,
        &session.open.store,
        course_id,
        operand,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

/// One cached page row.
pub(crate) struct PageRow {
    pub id: i64,
    pub course_id: Option<i64>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub updated_at: Option<String>,
    pub data: Value,
}

const PAGE_SELECT: &str =
    "SELECT id, course_id, url, title, body, updated_at, data_json FROM pages";

fn read_page(r: &rusqlite::Row<'_>) -> Result<PageRow, rusqlite::Error> {
    let raw: String = r.get(6)?;
    Ok(PageRow {
        id: r.get(0)?,
        course_id: r.get(1)?,
        url: r.get(2)?,
        title: r.get(3)?,
        body: r.get(4)?,
        updated_at: r.get(5)?,
        data: serde_json::from_str(&raw).unwrap_or(Value::Null),
    })
}

fn load_pages(conns: &StoreConns, course_id: i64) -> Result<Vec<PageSummaryJson>, DbError> {
    let scope = format!("course:{course_id}");
    let sql = format!(
        "{PAGE_SELECT} INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = pages.id
         WHERE m.dataset = 'pages' AND m.scope = ?1 AND m.entity_kind = 'page'
         ORDER BY m.position, pages.id"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    let rows = stmt
        .query_map([scope], read_page)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|row| PageSummaryJson {
            id: row.id.to_string(),
            title: row.title,
            url: row.url,
            updated_at: row.updated_at,
            published: json_bool(&row.data, "published"),
            front_page: json_bool(&row.data, "front_page"),
        })
        .collect())
}

/// The page the `page:<course>:<operand>` scope covers.
fn load_page(conns: &StoreConns, scope: &str) -> Result<Option<PageRow>, DbError> {
    let sql = format!(
        "{PAGE_SELECT} INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = pages.id
         WHERE m.dataset = 'page' AND m.scope = ?1 AND m.entity_kind = 'page'
         ORDER BY m.position, pages.id LIMIT 1"
    );
    Ok(conns
        .cache
        .query_row(&sql, params![scope], read_page)
        .optional()?)
}

fn local_error(globals: &Globals, session: &Session, err: &DbError) -> ExitCode {
    emit_error(
        globals.json,
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

pub(crate) fn json_string(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub(crate) fn json_bool(data: &Value, key: &str) -> Option<bool> {
    match data.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|v| v != 0),
        _ => None,
    }
}

pub(crate) fn json_u64(data: &Value, key: &str) -> Option<u64> {
    match data.get(key)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub(crate) fn json_f64(data: &Value, key: &str) -> Option<f64> {
    match data.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn print_page_table(pages: &[PageSummaryJson]) -> io::Result<()> {
    if pages.is_empty() {
        return writeln!(io::stdout(), "no pages");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec!["ID", "TITLE", "SLUG", "UPDATED", "STATE"]));
    for page in pages {
        table.add_row(Row::from(vec![
            page.id.clone(),
            page.title.clone().unwrap_or_default(),
            page.url.clone().unwrap_or_default(),
            page.updated_at.clone().unwrap_or_default(),
            match page.published {
                Some(true) => "published",
                Some(false) => "unpublished",
                None => "—",
            }
            .to_owned(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}")
}

fn print_page(page: &PageDetailJson) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "{}", page.title.clone().unwrap_or_default())?;
    if let Some(url) = page.html_url.as_deref() {
        writeln!(out, "{url}")?;
    }
    if let Some(updated) = page.updated_at.as_deref() {
        writeln!(out, "updated {updated}")?;
    }
    if let Some(body) = page.body_markdown.as_deref() {
        writeln!(out)?;
        writeln!(out, "{body}")?;
    }
    if page.truncated {
        writeln!(out, "\n[body truncated; not complete]")?;
    }
    print_refs(&mut out, &page.embedded, &page.files, &page.external_links)
}

fn print_syllabus(result: &SyllabusResult) -> io::Result<()> {
    let mut out = io::stdout();
    match result.syllabus_markdown.as_deref() {
        Some(body) => writeln!(out, "{body}")?,
        None => writeln!(out, "no syllabus")?,
    }
    if result.truncated {
        writeln!(out, "\n[body truncated; not complete]")?;
    }
    print_refs(
        &mut out,
        &result.embedded,
        &result.files,
        &result.external_links,
    )
}

pub(crate) fn print_refs(
    out: &mut impl Write,
    embedded: &[EmbeddedJson],
    files: &[FileRefJson],
    external: &[ExternalLinkJson],
) -> io::Result<()> {
    for item in embedded {
        let origin = item.src_origin.as_deref().unwrap_or("same origin");
        writeln!(out, "embedded {} ({origin}): unavailable", item.kind)?;
    }
    for file in files {
        writeln!(
            out,
            "file {} {}",
            file.file_id,
            file.name.clone().unwrap_or_default()
        )?;
    }
    for link in external {
        writeln!(out, "external {}", link.url)?;
    }
    Ok(())
}

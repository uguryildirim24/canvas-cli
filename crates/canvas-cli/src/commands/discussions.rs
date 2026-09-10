//! `canvas discussions` and `canvas discussion` (class C, M8-a).

use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::markdown::RichText;
use canvas_core::resolve::canvas_url;
use canvas_core::store::{DbError, StoreConns, lookup_dataset};
use canvas_core::sync::{
    DiscussionDataset, DiscussionsDataset, RefreshOutcome, listing_denial_status,
    refresh_discussion, refresh_discussions,
};
use comfy_table::Row;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::Globals;
use super::course::{refresh_fail, resolve_with_refresh};
use super::course_load::{
    RefreshFail, cached_outcome, cached_outcome_with_error, outcome_freshness,
};
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
use super::pages::{
    embedded_json, external_json, files_json, json_bool, json_f64, json_string, json_u64,
    print_refs, truncation_partial,
};
use crate::output::{
    DiscussionDetailJson, DiscussionReplyJson, DiscussionResult, DiscussionSummaryJson,
    DiscussionsResult, FilesListingJson, GroupTopicChildJson, Outcome, PartialScope,
    RepliesCoverageJson, SCHEMA_DISCUSSION, SCHEMA_DISCUSSIONS, apply_two_space_padding, new_table,
    now_timestamp,
};
use crate::session::{Session, ttl_discussions};

/// Replies shown per `--page`.
const REPLY_PAGE: usize = 100;

/// Run `canvas discussions` for the CLI: one envelope, one exit code.
pub async fn run_list(globals: &Globals, course: String, unread: bool) -> ExitCode {
    handle_list(globals, course, unread)
        .await
        .emit(globals.json)
}

/// Run `canvas discussions <course> [--unread]`.
///
/// The pinned request is `only_announcements=false`, so this listing holds
/// discussion topics and never an announcement (SPEC §19 item 27). The human
/// output says where announcements live instead.
pub async fn handle_list(globals: &Globals, course: String, unread: bool) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };

    let outcome = match ensure_discussions(globals, &session, resolved.id).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let rows = match session
        .open
        .store
        .call(move |conns| load_topics(conns, resolved.id))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return local_error(&session, &e),
    };
    let mut items: Vec<DiscussionSummaryJson> = rows.iter().map(TopicRow::summary).collect();
    if unread {
        // Filtered locally on the stored state; nothing is marked read.
        items.retain(|item| {
            item.read_state.as_deref() == Some("unread") || item.unread_count.unwrap_or(0) > 0
        });
    }

    let denial = outcome.error.as_deref().and_then(listing_denial_status);
    let result = DiscussionsResult {
        course_id: resolved.id.to_string(),
        listing: FilesListingJson {
            available: denial.is_none(),
            http_status: denial,
        },
        discussions: items,
    };
    let mut envelope = base_envelope(SCHEMA_DISCUSSIONS, &session, result);
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale discussions cache".into());
    }
    if let Some(status) = denial {
        let message = format!("Discussions listing unavailable (HTTP {status})");
        envelope.partial.push(PartialScope {
            scope: format!("discussions:course:{}", resolved.id),
            http_status: Some(status),
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    Handled::new(envelope, move |envelope| {
        print_topic_table(&envelope.result.discussions)
    })
}

/// Run `canvas discussion` for the CLI: one envelope, one exit code.
pub async fn run_show(
    globals: &Globals,
    course: String,
    discussion: String,
    replies: bool,
    page: Option<u32>,
) -> ExitCode {
    handle_show(globals, course, discussion, replies, page)
        .await
        .emit(globals.json)
}

/// Run `canvas discussion <course> <id|URL> [--replies] [--page N]`.
pub async fn handle_show(
    globals: &Globals,
    course: String,
    discussion: String,
    replies: bool,
    page: Option<u32>,
) -> Handled {
    let session = match globals.open_session() {
        Ok(s) => s,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    if page.is_some() && !replies {
        return emit_error(
            "usage",
            "--page needs --replies",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    if page == Some(0) {
        return emit_error(
            "usage",
            "--page counts from 1",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    let (resolved, mut freshness, _) = match resolve_with_refresh(globals, &session, &course).await
    {
        Ok(v) => v,
        Err(code) => return code,
    };
    let topic_id = match topic_id_of(&discussion, &session.identity.origin, resolved.id) {
        Ok(id) => id,
        Err(message) => {
            return emit_error(
                "resolution",
                message,
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };

    let outcome = match ensure_discussion(globals, &session, resolved.id, topic_id, replies).await {
        Ok(o) => o,
        Err(e) => return refresh_fail(&session, e),
    };
    freshness.push(outcome_freshness(&outcome));

    let row = match session
        .open
        .store
        .call(move |conns| load_topic(conns, topic_id))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return emit_error(
                "resolution",
                &format!("discussion {topic_id} not found"),
                6,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
        Err(e) => return local_error(&session, &e),
    };
    let entries = if replies {
        match session
            .open
            .store
            .call(move |conns| load_entries(conns, topic_id))
            .await
        {
            Ok(rows) => rows,
            Err(e) => return local_error(&session, &e),
        }
    } else {
        Vec::new()
    };

    let origin = session.identity.origin.clone();
    let rich = rich_or_default(row.message.as_deref()).await;
    let refs = rich.refs.resolve(&origin);
    let coverage = coverage_of(&row, replies);
    let window = page.unwrap_or(1) as usize;
    let start = (window - 1) * REPLY_PAGE;
    // A page past the end is an empty list, not an error (SPEC §19 item 28).
    // `replies_total` is what tells the two apart, so it counts the whole
    // covered set rather than the window. Without `--replies` nothing was
    // read, and an unmade count is `null`, not 0: a topic with replies must
    // never report the document a topic without them reports.
    let replies_total = replies.then(|| u32::try_from(entries.len()).unwrap_or(u32::MAX));
    let mut reply_json = Vec::new();
    for entry in entries.iter().skip(start).take(REPLY_PAGE) {
        let body = rich_or_default(entry.message.as_deref()).await;
        reply_json.push(DiscussionReplyJson {
            id: entry.id.to_string(),
            parent_id: entry.parent_id.map(|id| id.to_string()),
            user_id: entry.user_id.map(|id| id.to_string()),
            user_name: json_string(&entry.data, "user_name"),
            created_at: entry.created_at.clone(),
            message_markdown: body.markdown.clone(),
            truncated: body.refs.truncated,
            read_state: json_string(&entry.data, "read_state"),
            replies_count: json_u64(&entry.data, "replies_count").unwrap_or(0),
        });
    }

    let detail = DiscussionDetailJson {
        id: row.id.to_string(),
        course_id: row.course_id.map(|id| id.to_string()),
        title: row.title.clone(),
        posted_at: row.posted_at.clone(),
        last_reply_at: json_string(&row.data, "last_reply_at"),
        author: json_string(&row.data, "author"),
        discussion_type: json_string(&row.data, "discussion_type"),
        read_state: json_string(&row.data, "read_state"),
        unread_count: json_u64(&row.data, "unread_count"),
        reply_count: json_u64(&row.data, "discussion_subentry_count"),
        locked: json_bool(&row.data, "locked"),
        pinned: json_bool(&row.data, "pinned"),
        is_announcement: json_bool(&row.data, "is_announcement"),
        require_initial_post: json_bool(&row.data, "require_initial_post"),
        assignment_id: json_string(&row.data, "assignment_id"),
        points_possible: json_f64(&row.data, "points_possible"),
        group_category_id: json_string(&row.data, "group_category_id"),
        group_topic_children: group_children(&row.data),
        html_url: json_string(&row.data, "html_url"),
        message_markdown: rich.markdown.clone(),
        truncated: rich.refs.truncated,
        embedded: embedded_json(&refs),
        files: files_json(&refs),
        external_links: external_json(&refs),
        replies: reply_json,
        replies_page: u32::try_from(window).unwrap_or(u32::MAX),
        replies_total,
        replies_coverage: coverage.clone(),
    };

    // §10: a reply this CLI started and did not resolve makes the thread
    // uncertain, whatever the cache says.
    let pending = super::inbox::pending_operations(
        &session,
        canvas_core::store::PendingTarget::Topic(topic_id),
    )
    .await;
    let mut envelope = base_envelope(
        SCHEMA_DISCUSSION,
        &session,
        DiscussionResult {
            discussion: detail,
            pending: !pending.is_empty(),
            pending_journals: pending,
        },
    );
    envelope.freshness = freshness;
    envelope.requests = session.requests();
    if outcome.freshness.stale {
        envelope
            .warnings
            .push("served stale discussion cache".into());
    }
    truncation_partial(
        &mut envelope,
        rich.refs.truncated,
        &format!("discussion:{topic_id}"),
    );
    // A reply body is bounded like every other body, so a cut one is a partial
    // answer too; the topic message is not the only text this command returns.
    let cut = envelope
        .result
        .discussion
        .replies
        .iter()
        .filter(|reply| reply.truncated)
        .count();
    if cut > 0 {
        let message = format!(
            "{cut} reply bodies truncated at {} bytes; the text is not complete",
            canvas_core::markdown::BODY_LIMIT
        );
        envelope.partial.push(PartialScope {
            scope: format!("discussion_entries:topic:{topic_id}"),
            http_status: None,
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    // The gate is a refusal, not a partial answer: the thread is not readable
    // until this user posts, so `--replies` cannot be served at all.
    if replies && coverage.blocked.as_deref() == Some("initial_post_required") {
        return emit_error(
            "refused",
            "initial_post_required: post to this discussion before you can read its replies",
            8,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    if replies && !coverage.complete {
        let message =
            format!("reply set for discussion {topic_id} is not complete; a page did not load");
        envelope.partial.push(PartialScope {
            scope: format!("discussion_entries:topic:{topic_id}"),
            http_status: None,
            message: message.clone(),
        });
        envelope.warnings.push(message);
        envelope.outcome = Outcome::Partial;
        envelope.exit = 12;
    }

    Handled::new(envelope, move |envelope| {
        print_topic(&envelope.result.discussion)
    })
}

async fn rich_or_default(html: Option<&str>) -> RichText {
    canvas_core::markdown::rich_text_opt(html)
        .await
        .unwrap_or_default()
}

/// Coverage as the cache recorded it, never widened by this read.
fn coverage_of(row: &TopicRow, replies: bool) -> RepliesCoverageJson {
    if !replies {
        return RepliesCoverageJson {
            pages_fetched: 0,
            complete: false,
            blocked: Some("not_requested".to_owned()),
        };
    }
    RepliesCoverageJson {
        pages_fetched: u32::try_from(json_u64(&row.data, "replies_pages_fetched").unwrap_or(0))
            .unwrap_or(u32::MAX),
        complete: json_bool(&row.data, "replies_complete").unwrap_or(false),
        blocked: json_string(&row.data, "replies_blocked"),
    }
}

fn group_children(data: &Value) -> Vec<GroupTopicChildJson> {
    let raw = data.get("group_topic_children").and_then(|v| match v {
        Value::String(s) => serde_json::from_str::<Value>(s).ok(),
        other => Some(other.clone()),
    });
    raw.as_ref()
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| GroupTopicChildJson {
            id: id_string(c.get("id")),
            group_id: id_string(c.get("group_id")),
        })
        .collect()
}

fn id_string(raw: Option<&Value>) -> Option<String> {
    raw.and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}

/// The topic id: a bare number, or a Canvas discussion URL for this course.
pub(super) fn topic_id_of(raw: &str, origin: &str, course_id: i64) -> Result<i64, &'static str> {
    let trimmed = raw.trim();
    if let Ok(id) = trimmed.parse::<i64>() {
        if id > 0 {
            return Ok(id);
        }
        return Err("discussion id must be a positive number");
    }
    match canvas_url(trimmed, origin) {
        Err(_) => Err("url origin does not match the active identity"),
        Ok(None) => Err("discussion needs an id or a Canvas discussion URL"),
        Ok(Some(url)) => {
            let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
            let Some(window) = parts
                .windows(4)
                .find(|p| p[0] == "courses" && p[2] == "discussion_topics")
            else {
                return Err("url is not a Canvas discussion");
            };
            if window[1].parse::<i64>() != Ok(course_id) {
                return Err("url names a different course than the course operand");
            }
            window[3]
                .parse::<i64>()
                .map_err(|_| "url has no discussion id")
        }
    }
}

async fn ensure_discussions(
    globals: &Globals,
    session: &Session,
    course_id: i64,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_discussions();
    let ds = DiscussionsDataset::new(course_id, ttl);
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
    refresh_discussions(
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

async fn ensure_discussion(
    globals: &Globals,
    session: &Session,
    course_id: i64,
    topic_id: i64,
    replies: bool,
) -> Result<RefreshOutcome, RefreshFail> {
    let now = now_timestamp();
    let ttl = ttl_discussions();
    let ds = DiscussionDataset::new(course_id, topic_id, replies, ttl);
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
    refresh_discussion(
        client,
        &session.open.store,
        course_id,
        topic_id,
        replies,
        ttl,
        now,
        globals.fresh,
        false,
    )
    .await
    .map_err(RefreshFail::Sync)
}

/// One cached discussion topic row.
pub(crate) struct TopicRow {
    pub id: i64,
    pub course_id: Option<i64>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub posted_at: Option<String>,
    pub data: Value,
}

impl TopicRow {
    fn summary(&self) -> DiscussionSummaryJson {
        DiscussionSummaryJson {
            id: self.id.to_string(),
            course_id: self.course_id.map(|id| id.to_string()),
            title: self.title.clone(),
            posted_at: self.posted_at.clone(),
            last_reply_at: json_string(&self.data, "last_reply_at"),
            author: json_string(&self.data, "author"),
            read_state: json_string(&self.data, "read_state"),
            unread_count: json_u64(&self.data, "unread_count"),
            reply_count: json_u64(&self.data, "discussion_subentry_count"),
            locked: json_bool(&self.data, "locked"),
            pinned: json_bool(&self.data, "pinned"),
            is_announcement: json_bool(&self.data, "is_announcement"),
            require_initial_post: json_bool(&self.data, "require_initial_post"),
            assignment_id: json_string(&self.data, "assignment_id"),
            points_possible: json_f64(&self.data, "points_possible"),
            group_category_id: json_string(&self.data, "group_category_id"),
            html_url: json_string(&self.data, "html_url"),
        }
    }
}

/// One cached reply row.
pub(crate) struct EntryRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub user_id: Option<i64>,
    pub message: Option<String>,
    pub created_at: Option<String>,
    pub data: Value,
}

const TOPIC_SELECT: &str = "SELECT id, course_id, title, message, posted_at, data_json
     FROM discussion_topics";

fn read_topic(r: &rusqlite::Row<'_>) -> Result<TopicRow, rusqlite::Error> {
    let raw: String = r.get(5)?;
    Ok(TopicRow {
        id: r.get(0)?,
        course_id: r.get(1)?,
        title: r.get(2)?,
        message: r.get(3)?,
        posted_at: r.get(4)?,
        data: serde_json::from_str(&raw).unwrap_or(Value::Null),
    })
}

fn load_topics(conns: &StoreConns, course_id: i64) -> Result<Vec<TopicRow>, DbError> {
    let scope = format!("course:{course_id}");
    let sql = format!(
        "{TOPIC_SELECT}
         INNER JOIN membership m ON CAST(m.entity_id AS INTEGER) = discussion_topics.id
         WHERE m.dataset = 'discussions' AND m.scope = ?1
           AND m.entity_kind = 'discussion_topic'
         ORDER BY m.position, discussion_topics.id"
    );
    let mut stmt = conns.cache.prepare(&sql)?;
    Ok(stmt
        .query_map([scope], read_topic)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn load_topic(conns: &StoreConns, id: i64) -> Result<Option<TopicRow>, DbError> {
    let sql = format!("{TOPIC_SELECT} WHERE id = ?1");
    Ok(conns.cache.query_row(&sql, [id], read_topic).optional()?)
}

fn load_entries(conns: &StoreConns, topic_id: i64) -> Result<Vec<EntryRow>, DbError> {
    let scope = format!("topic:{topic_id}");
    let mut stmt = conns.cache.prepare(
        "SELECT e.id, e.parent_id, e.user_id, e.message, e.created_at, e.data_json
         FROM membership m
         JOIN discussion_entries e ON e.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'discussion_entries' AND m.scope = ?1
           AND m.entity_kind = 'discussion_entry'
         ORDER BY m.position, e.id",
    )?;
    Ok(stmt
        .query_map(params![scope], |r| {
            let raw: String = r.get(5)?;
            Ok(EntryRow {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                user_id: r.get(2)?,
                message: r.get(3)?,
                created_at: r.get(4)?,
                data: serde_json::from_str(&raw).unwrap_or(Value::Null),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn local_error(session: &Session, err: &DbError) -> Handled {
    emit_error(
        "local",
        &err.to_string(),
        13,
        session.profile.clone(),
        Some(session.identity_ref()),
    )
}

/// Announcements are a different listing, so the table says so rather than
/// letting a reader take an empty or short list for the whole course feed.
const ANNOUNCEMENTS_POINTER: &str = "announcements are listed by `canvas announcements`";

fn print_topic_table(items: &[DiscussionSummaryJson]) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(io::stdout(), "no discussions\n{ANNOUNCEMENTS_POINTER}");
    }
    let mut table = new_table();
    table.set_header(Row::from(vec![
        "ID",
        "TITLE",
        "LAST REPLY",
        "REPLIES",
        "UNREAD",
        "STATE",
    ]));
    for item in items {
        table.add_row(Row::from(vec![
            item.id.clone(),
            item.title.clone().unwrap_or_default(),
            item.last_reply_at.clone().unwrap_or_default(),
            item.reply_count.map(|n| n.to_string()).unwrap_or_default(),
            item.unread_count.map(|n| n.to_string()).unwrap_or_default(),
            item.read_state.clone().unwrap_or_default(),
        ]));
    }
    apply_two_space_padding(&mut table);
    writeln!(io::stdout(), "{table}\n{ANNOUNCEMENTS_POINTER}")
}

fn print_topic(topic: &DiscussionDetailJson) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "{}", topic.title.clone().unwrap_or_default())?;
    if let Some(author) = topic.author.as_deref() {
        writeln!(out, "by {author}")?;
    }
    if let Some(posted) = topic.posted_at.as_deref() {
        writeln!(out, "posted {posted}")?;
    }
    if let Some(points) = topic.points_possible {
        writeln!(out, "graded, {points} points")?;
    }
    if !topic.group_topic_children.is_empty() {
        writeln!(
            out,
            "group discussion, {} child topics",
            topic.group_topic_children.len()
        )?;
    }
    if let Some(url) = topic.html_url.as_deref() {
        writeln!(out, "{url}")?;
    }
    if let Some(body) = topic.message_markdown.as_deref() {
        writeln!(out)?;
        writeln!(out, "{body}")?;
    }
    if topic.truncated {
        writeln!(out, "\n[body truncated; not complete]")?;
    }
    print_refs(
        &mut out,
        &topic.embedded,
        &topic.files,
        &topic.external_links,
    )?;
    for reply in &topic.replies {
        writeln!(out)?;
        writeln!(
            out,
            "— {} {}",
            reply.user_name.clone().unwrap_or_default(),
            reply.created_at.clone().unwrap_or_default()
        )?;
        if let Some(body) = reply.message_markdown.as_deref() {
            writeln!(out, "{body}")?;
        }
    }
    // "page 1 of 3" would read as one page of three; the total counts
    // replies, not pages. The line names both units instead.
    let Some(total) = topic.replies_total else {
        return writeln!(out, "\nreplies: not read; --replies asks for the thread");
    };
    writeln!(
        out,
        "\nreplies: showing page {}; {total} replies in the covered set, \
         {} pages fetched, complete={}",
        topic.replies_page, topic.replies_coverage.pages_fetched, topic.replies_coverage.complete
    )
}

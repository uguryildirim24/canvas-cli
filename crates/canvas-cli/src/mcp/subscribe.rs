//! `subscriptions/listen` over the event log (REPORT §3.6, §3.2).
//!
//! One identity keeps one event log. `watch` streams it, `notify` derives
//! alerts from it, and a subscribed MCP host reads the same rows: every event
//! that names a dataset scope invalidates the resources that answer from that
//! scope, and the server sends one `notifications/resources/updated` per
//! resource. Nothing is invented here. A URI is invalidated only when a row in
//! the log says its scope changed, and a dataset REPORT §3.6 names no kind for
//! emits nothing at all.
//!
//! The cursor rules are the ones `watch --since` follows. The position is
//! durable per consumer, so a reconnecting host resumes where it stopped; a
//! cursor this log can still replay is replayed at least once, in cursor
//! order; and a cursor that expired or belongs to another identity generation
//! asks the host to resync instead of pretending the gap did not happen. A
//! resync invalidates every subscribed resource once: the host re-reads what
//! it holds and follows the log from its high water mark.
//!
//! A host opens the subscription after it discovers the server. The SDK
//! answers the first request of a connection inline, before its service loop
//! starts, so a connection that opens a subscription as its very first
//! request would get no other answer while the stream is open.

use std::time::Duration;

use canvas_core::events::{
    CursorCheck, EventRecord, check_cursor, consumer_cursor, high_water, read_after,
    reset_consumer_cursor, set_consumer_cursor,
};
use canvas_core::store::DbError;
use rmcp::model::{ErrorData, RequestMetaObject};
use rmcp::service::SubscriptionContext;
use rusqlite::Connection;

use crate::commands::Globals;
use crate::mcp::resources::{Binding, Target};

/// The `_meta` key a host uses to name the cursor it resumes from.
///
/// It is optional: without it the durable consumer position is used, which is
/// what a host that simply reconnects wants.
pub const META_CURSOR: &str = "dev.canvas-cli/cursor";

/// How often an established subscription re-reads the event log.
const POLL: Duration = Duration::from_secs(2);

/// How many rows one read takes.
const BATCH: usize = 500;

/// Where a subscription starts reading, and whether the host must resync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Replay from this cursor, then follow the log.
    Replay(i64),
    /// The cursor cannot be replayed: invalidate everything subscribed, then
    /// follow the log from this position.
    Resync(i64),
}

/// Classify the cursor a subscription resumes from (§3.2 replay rules).
pub fn start(state: &Connection, since: i64, generation: &str) -> Result<Start, DbError> {
    match check_cursor(state, since, generation)? {
        CursorCheck::Replay => Ok(Start::Replay(since)),
        // The gap is reported once, and the stream continues from the log's
        // own high water mark: everything before it is what the host re-reads.
        CursorCheck::Resync => Ok(Start::Resync(high_water(state)?)),
    }
}

/// Whether an event on this log can ever invalidate the target.
///
/// `context/<consumer-handle>` is a name this server reads, but no event names
/// it, and it addresses another consumer's routing. REPORT §3.2 keeps that out
/// of resource subscriptions — "no implicit sharing from resource
/// subscriptions" — so it is not subscribable: a host that held one would be
/// told on a resync that a consumer context it does not own changed, which
/// nothing observed.
fn subscribable(target: &Target) -> bool {
    match target {
        Target::Todo | Target::Receipts | Target::CourseAssignments(_) => true,
        Target::Context(_) => false,
    }
}

/// The resource URIs of a requested filter this instance can invalidate.
///
/// A foreign identity key, another generation, an unknown path, and a name no
/// event can reach all address nothing here, so they are dropped instead of
/// refused: the host keeps the part of its subscription that can actually be
/// served.
#[must_use]
pub fn served(binding: &Binding, requested: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|uri| binding.target(uri).as_ref().is_some_and(subscribable))
        .cloned()
        .collect()
}

/// Which resources of this binding one event invalidates.
///
/// REPORT §3.6 leaves the mapping open, and the rule for an undefined case is
/// to claim less: a dataset is mapped only to the resources that read it.
#[must_use]
pub fn invalidated(binding: &Binding, record: &EventRecord) -> Vec<String> {
    // Both halves of the name are part of every URI, so a row another
    // identity generation wrote addresses nothing this instance serves.
    if record.identity_key != binding.key || record.generation != binding.generation {
        return Vec::new();
    }
    let mut uris = Vec::new();
    match record.dataset.as_str() {
        // One course's assignment membership: that course's resource, and the
        // todo window, which renders the assignment rows the planner names.
        "assignments" => {
            if let Some(course) = record.scope.strip_prefix("course:") {
                uris.push(binding.uri(&format!("course/{course}/assignments")));
            }
            uris.push(binding.uri("todo"));
        }
        // The missing membership is one of the datasets `todo.list` reads.
        "missing" => uris.push(binding.uri("todo")),
        // A journal transition changes the local receipts, and only those:
        // nothing about the deadline window was observed on Canvas.
        "submission_journal" => uris.push(binding.uri("receipts")),
        // `announcements` has an event kind but no resource, and no other
        // dataset writes events, so nothing else is invalidated.
        _ => {}
    }
    uris
}

/// The URIs one batch invalidates: subscribed, deduplicated, in cursor order.
///
/// A batch invalidates a resource once. The host re-reads it after the
/// notification, so a second notification for the same URI in the same batch
/// would ask for a read it is already making.
#[must_use]
pub fn updates(binding: &Binding, subscribed: &[String], records: &[EventRecord]) -> Vec<String> {
    let mut uris: Vec<String> = Vec::new();
    for record in records {
        for uri in invalidated(binding, record) {
            if subscribed.contains(&uri) && !uris.contains(&uri) {
                uris.push(uri);
            }
        }
    }
    uris
}

/// The cursor a host names in `_meta`, when it names one.
fn requested_cursor(meta: &RequestMetaObject) -> Option<i64> {
    let value = meta.0.0.get(META_CURSOR)?;
    // §7 writes every id as a decimal string; a JSON number is accepted too,
    // because a host that has just read `cursor` back may not have quoted it.
    value
        .as_str()
        .and_then(|raw| raw.parse::<i64>().ok())
        .or_else(|| value.as_i64())
        .filter(|cursor| *cursor >= 0)
}

/// Serve one established subscription until the host cancels it.
///
/// The acknowledgment is already sent when this starts, so a failure here ends
/// the listen request rather than the connection: every other request, and
/// every other subscription, keeps working.
pub async fn listen(
    globals: &Globals,
    binding: &Binding,
    consumer: &str,
    context: SubscriptionContext,
) -> Result<(), ErrorData> {
    let Some(subscribed) = context.accepted().resource_subscriptions.clone() else {
        // A list-changed subscription stays quiet, as it did before: one
        // instance serves one generation, so its resource list never changes.
        context.cancelled().await;
        return Ok(());
    };

    // The log is local, so the subscription needs no token and no network. The
    // session is held for the life of the stream, which makes a subscribed
    // host a resident consumer: `identity remove` reports busy (§3.4).
    let session = globals.open_local_session().map_err(|error| {
        ErrorData::internal_error(format!("cannot open the identity store: {error}"), None)
    })?;
    let store = &session.open.store;
    let consumer = consumer.to_owned();

    let stored = {
        let consumer = consumer.clone();
        store
            .call(move |conns| consumer_cursor(&conns.state, &consumer))
            .await
            .map_err(db_error)?
    };
    let named = requested_cursor(&context.request_context().meta);
    let since = named.unwrap_or(stored);
    let generation = binding.generation.clone();
    let begin = store
        .call(move |conns| start(&conns.state, since, &generation))
        .await
        .map_err(db_error)?;

    let mut cursor = match begin {
        Start::Replay(cursor) => cursor,
        Start::Resync(cursor) => {
            for uri in &subscribed {
                if !send(&context, uri).await {
                    return Ok(());
                }
            }
            // A stored position the log cannot replay is not a position, so it
            // is replaced at once and the same gap is never reported to the
            // same consumer twice. A position the host named in `_meta` is the
            // host's own, and it never replaces the stored one: that one is
            // still replayable, and it still names rows this consumer has not
            // been told about. `notify` draws the same line at `--since`.
            if named.is_none() {
                let who = consumer.clone();
                store
                    .call(move |conns| reset_consumer_cursor(&mut conns.state, &who, cursor))
                    .await
                    .map_err(db_error)?;
            }
            cursor
        }
    };

    loop {
        loop {
            let batch = store
                .call(move |conns| read_after(&conns.state, cursor, BATCH))
                .await
                .map_err(db_error)?;
            let Some(last) = batch.last() else { break };
            let next = last.cursor;
            for uri in updates(binding, &subscribed, &batch) {
                if !send(&context, &uri).await {
                    return Ok(());
                }
            }
            // The position moves only after the notifications are sent, so a
            // stream that ends mid-batch replays it instead of dropping it.
            cursor = next;
            record_cursor(store, &consumer, cursor).await?;
        }
        tokio::select! {
            biased;
            () = context.cancelled() => break,
            () = tokio::time::sleep(poll()) => {}
        }
    }
    Ok(())
}

/// Send one `notifications/resources/updated`, or report the stream is gone.
async fn send(context: &SubscriptionContext, uri: &str) -> bool {
    match context.sink().notify_resource_updated(uri.to_owned()).await {
        Ok(()) => true,
        // A closed subscription is the normal end of a stream, and a filter
        // refusal cannot happen for a URI the accepted filter names. Either
        // way there is nothing left to send.
        Err(error) => {
            tracing::debug!(%error, uri, "cannot notify a subscribed resource");
            false
        }
    }
}

/// Move the durable consumer position forward.
async fn record_cursor(
    store: &canvas_core::store::Store,
    consumer: &str,
    cursor: i64,
) -> Result<(), ErrorData> {
    let consumer = consumer.to_owned();
    store
        .call(move |conns| set_consumer_cursor(&mut conns.state, &consumer, cursor))
        .await
        .map_err(db_error)
}

fn db_error(error: DbError) -> ErrorData {
    ErrorData::internal_error(format!("the event log is unreadable: {error}"), None)
}

/// How long a subscription waits between reads, shortened by tests.
fn poll() -> Duration {
    if cfg!(debug_assertions)
        && let Ok(raw) = std::env::var("CANVAS_TEST_MCP_POLL_MS")
        && let Ok(millis) = raw.parse::<u64>()
    {
        return Duration::from_millis(millis);
    }
    POLL
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn binding() -> Binding {
        Binding::new(
            "canvas.test-7-abcd1234",
            "11111111-1111-4111-8111-111111111111",
        )
    }

    fn record(dataset: &str, scope: &str) -> EventRecord {
        let binding = binding();
        EventRecord {
            cursor: 1,
            observation_id: format!("{dataset}:{scope}:1:2026-09-09T17:05:12Z"),
            kind: "assignment.changed".to_owned(),
            observed_at: "2026-09-09T17:05:12Z".to_owned(),
            identity_key: binding.key,
            generation: binding.generation,
            dataset: dataset.to_owned(),
            scope: scope.to_owned(),
            entity_key: Some("500".to_owned()),
            before: Value::Null,
            after: Value::Null,
        }
    }

    #[test]
    fn an_event_invalidates_only_the_resources_that_read_its_scope() {
        let binding = binding();
        assert_eq!(
            invalidated(&binding, &record("assignments", "course:1")),
            vec![binding.uri("course/1/assignments"), binding.uri("todo"),]
        );
        assert_eq!(
            invalidated(&binding, &record("missing", "self")),
            vec![binding.uri("todo")]
        );
        assert_eq!(
            invalidated(&binding, &record("submission_journal", "assignment:500")),
            vec![binding.uri("receipts")]
        );
        // `announcements` has a kind and no resource; `courses`, `planner`,
        // and the grade datasets have neither.
        for dataset in ["announcements", "courses", "planner", "enrollment_grades"] {
            assert!(
                invalidated(&binding, &record(dataset, "courses")).is_empty(),
                "{dataset}"
            );
        }
    }

    #[test]
    fn a_row_from_another_identity_generation_invalidates_nothing() {
        let binding = binding();
        let mut record = record("assignments", "course:1");
        record.generation = "22222222-2222-4222-8222-222222222222".to_owned();
        assert!(invalidated(&binding, &record).is_empty());
        let mut record = record.clone();
        record.generation = binding.generation.clone();
        record.identity_key = "other.test-9-99999999".to_owned();
        assert!(invalidated(&binding, &record).is_empty());
    }

    #[test]
    fn a_batch_notifies_each_subscribed_resource_once_and_no_other() {
        let binding = binding();
        let subscribed = vec![binding.uri("todo"), binding.uri("course/1/assignments")];
        let batch = vec![
            record("assignments", "course:1"),
            record("assignments", "course:1"),
            record("missing", "self"),
            // Course 2 is not subscribed, and neither is `receipts`.
            record("assignments", "course:2"),
            record("submission_journal", "assignment:500"),
        ];
        assert_eq!(
            updates(&binding, &subscribed, &batch),
            vec![binding.uri("course/1/assignments"), binding.uri("todo"),]
        );
    }

    #[test]
    fn only_uris_this_instance_serves_are_accepted() {
        let binding = binding();
        let other = Binding::new(&binding.key, "22222222-2222-4222-8222-222222222222");
        let foreign = Binding::new("other.test-9-99999999", &binding.generation);
        let requested = vec![
            binding.uri("todo"),
            binding.uri("receipts"),
            binding.uri("course/1/assignments"),
            // Not subscribable: an unknown path, another generation, another
            // identity, something that is not even this scheme, and a
            // consumer context, which no event names and which belongs to
            // another consumer's routing (REPORT §3.2).
            binding.uri("auth/token"),
            other.uri("todo"),
            foreign.uri("todo"),
            "file:///etc/passwd".to_owned(),
            binding.uri("context/consumer-1"),
        ];
        assert_eq!(
            served(&binding, &requested),
            vec![
                binding.uri("todo"),
                binding.uri("receipts"),
                binding.uri("course/1/assignments"),
            ]
        );
    }

    #[test]
    fn a_host_can_name_the_cursor_it_resumes_from() {
        let mut meta = RequestMetaObject::new();
        assert_eq!(requested_cursor(&meta), None);
        meta.0.0.insert(META_CURSOR.to_owned(), Value::from("42"));
        assert_eq!(requested_cursor(&meta), Some(42));
        // A number is accepted, and nothing else is.
        meta.0.0.insert(META_CURSOR.to_owned(), Value::from(7));
        assert_eq!(requested_cursor(&meta), Some(7));
        for bad in [Value::from(-1), Value::from("soon"), Value::Null] {
            meta.0.0.insert(META_CURSOR.to_owned(), bad);
            assert_eq!(requested_cursor(&meta), None);
        }
    }
}

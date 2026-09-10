//! `operation status` and `operation reconcile` (M8-b).
//!
//! Both read the thread again. Neither ever resends anything.
//!
//! - **status** enriches. It records what the readback saw and, when the
//!   readback shows the object the acceptance named, moves attribution from
//!   `accepted` to `observed`. The state never changes.
//! - **reconcile** decides. On an `outcome_unknown` journal it either finds a
//!   message whose digest matches and moves the journal to `matched` with
//!   attribution `unproven`, or it finds nothing — and then, and only with
//!   `--assume-not-posted` and after thirty minutes, records that nothing was
//!   posted.
//!
//! Both take the owner lock and nothing else. A journal a live owner still
//! holds is left alone.

use canvas_api::Client;
use jiff::Timestamp;
use serde_json::Value;

use crate::journal::{OwnerLock, OwnerStatus};
use crate::store::Store;

use super::OperationError;
use super::execute::hex_sha256;
use super::ops;
use super::prepare::{entries_of, entry_replies_of, get, json_id};
use super::record::{
    Attribution, OpState, OperationReadback, OperationRow, OperationTarget, ServerMatch,
};

/// What one status or reconcile produced.
#[derive(Debug)]
pub struct Reconciled {
    /// The journal as it stands afterwards.
    pub row: Box<OperationRow>,
    /// The readback that ran, when one did.
    pub readback: Option<OperationReadback>,
    /// What the readback let this process conclude.
    pub verdict: Verdict,
    /// The owner probe at the time of the read.
    pub owner: OwnerStatus,
    /// A caveat worth printing beside the result.
    pub warning: Option<String>,
}

/// What a readback concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The readback found the object the acceptance named.
    Observed,
    /// The readback found a message whose digest matches, with no id link.
    Matched,
    /// The readback found nothing of this operation.
    NotFound,
    /// No readback ran: the journal needs none, or its owner is live.
    NotRead,
    /// A person asserted that nothing was posted.
    AssumedNotPosted,
}

impl Verdict {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Matched => "matched",
            Self::NotFound => "not_found",
            Self::NotRead => "not_read",
            Self::AssumedNotPosted => "assumed_not_posted",
        }
    }
}

/// Read one operation journal back, without changing its state.
pub async fn status(
    client: &Client,
    store: &Store,
    identity_dir: &std::path::Path,
    journal_id: &str,
) -> Result<Reconciled, OperationError> {
    run(
        client,
        store,
        identity_dir,
        journal_id,
        false,
        false,
        Timestamp::now(),
    )
    .await
}

/// Resolve one operation journal against the thread as it stands now.
pub async fn reconcile(
    client: &Client,
    store: &Store,
    identity_dir: &std::path::Path,
    journal_id: &str,
    assume_not_posted: bool,
    now: Timestamp,
) -> Result<Reconciled, OperationError> {
    run(
        client,
        store,
        identity_dir,
        journal_id,
        true,
        assume_not_posted,
        now,
    )
    .await
}

async fn run(
    client: &Client,
    store: &Store,
    identity_dir: &std::path::Path,
    journal_id: &str,
    may_transition: bool,
    assume_not_posted: bool,
    now: Timestamp,
) -> Result<Reconciled, OperationError> {
    let row = ops::require(store, journal_id)?;
    let owner_status = ops::owner_status_for(identity_dir, journal_id, row.state)?;

    // A live owner is still running this operation. Reading the thread would
    // be honest, but changing the journal under it would not.
    let Some(owner) = OwnerLock::try_acquire(identity_dir, journal_id)? else {
        return Ok(Reconciled {
            row: Box::new(row),
            readback: None,
            verdict: Verdict::NotRead,
            owner: OwnerStatus::Live,
            warning: Some("another process owns this operation right now".to_owned()),
        });
    };

    // The owner was absent: give the row the state its interruption implies
    // before anything is concluded from a readback.
    ops::recover_owned(store, &owner, journal_id)?;
    let row = ops::require(store, journal_id)?;

    let readback = read_thread(client, &row).await?;
    let found = match_of(&row, &readback);

    // Enrichment is the same for both commands: the journal records what was
    // seen, and an acceptance whose object is now visible becomes `observed`.
    let attribution = ops::enrich_readback(store, &owner, journal_id, &readback)?;
    let mut verdict = if attribution == Attribution::Observed {
        Verdict::Observed
    } else if found.is_some() {
        Verdict::Matched
    } else {
        Verdict::NotFound
    };
    let mut warning = None;

    if may_transition && row.state == OpState::OutcomeUnknown {
        if let Some(candidate) = found.clone() {
            // A digest match with no id link is exactly `unproven`, and the
            // state that carries it is `matched`.
            ops::commit_matched(store, &owner, journal_id, &candidate, &readback)?;
            persist_identity(store, journal_id)?;
            verdict = Verdict::Matched;
        } else if assume_not_posted {
            // A readback that did not cover the whole thread cannot prove
            // absence, so it cannot support the assertion that nothing was
            // posted either (`docs/writes-v2.md` choice 8). The exposed case
            // is an `inbox_send` Canvas never named a conversation for: there
            // is no thread to read at all, and asserting "never sent" there
            // invites a resend that would be a second message.
            if readback.complete {
                match ops::assume_not_posted(store, &owner, journal_id, false, now) {
                    Ok(()) => verdict = Verdict::AssumedNotPosted,
                    Err(OperationError::StateConflict) => {
                        warning = Some(
                            "nothing can be assumed yet: an operation must be thirty minutes \
                             old and unseen before it is recorded as never posted"
                                .to_owned(),
                        );
                    }
                    Err(other) => return Err(other),
                }
            } else {
                warning = Some(
                    "nothing can be assumed: the readback did not cover the thread, so it is \
                     no evidence that the write is absent"
                        .to_owned(),
                );
            }
        } else {
            warning = Some(
                "the outcome is still unknown; nothing was resent, and \
                 --assume-not-posted records that nothing was posted"
                    .to_owned(),
            );
        }
    }
    if !readback.complete {
        warning.get_or_insert_with(|| {
            "the readback did not cover the whole thread, so a missing object is not proof"
                .to_owned()
        });
    }

    Ok(Reconciled {
        row: Box::new(ops::require(store, journal_id)?),
        readback: Some(readback),
        verdict,
        owner: owner_status,
        warning,
    })
}

fn persist_identity(store: &Store, journal_id: &str) -> Result<(), OperationError> {
    let row = ops::require(store, journal_id)?;
    let Some(mut receipt) = row.receipt else {
        return Ok(());
    };
    ops::attach_identity(store, &mut receipt)?;
    let json = serde_json::to_string(&receipt)?;
    let jid = journal_id.to_owned();
    store.call_blocking(move |conns| {
        conns.state.execute(
            "UPDATE operation_journal SET receipt_record_json = ?1 WHERE journal_id = ?2",
            rusqlite::params![json, jid],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Read the thread this operation writes to.
///
/// A top-level discussion reply is paged through the topic's entries; a
/// **threaded** reply is paged through its parent entry's replies, because
/// that is where Canvas puts it and the topic's entry listing is top-level
/// only. A conversation is one `GET` with `auto_mark_as_read=false`, so
/// reading never marks anything read. A new conversation is read back through
/// the conversation Canvas said it created, and when Canvas never said, there
/// is nothing to read.
pub async fn readback(
    client: &Client,
    row: &OperationRow,
) -> Result<OperationReadback, OperationError> {
    read_thread(client, row).await
}

async fn read_thread(
    client: &Client,
    row: &OperationRow,
) -> Result<OperationReadback, OperationError> {
    let read_at = Timestamp::now().to_string();
    let empty = |complete: bool| OperationReadback {
        read_at: read_at.clone(),
        complete,
        ..OperationReadback::default()
    };
    let objects: Vec<Value> = match &row.intended.target {
        // `--to` posted to `…/entries/:eid/replies`, and Canvas does not list
        // a nested reply among the topic's top-level entries. Reading the
        // wrong route would report every threaded reply as absent, which is
        // exactly the evidence `--assume-not-posted` must never be given.
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id: Some(entry_id),
        } => entry_replies_of(client, *course_id, *topic_id, *entry_id).await?,
        OperationTarget::DiscussionReply {
            course_id,
            topic_id,
            parent_entry_id: None,
        } => entries_of(client, *course_id, *topic_id).await?,
        OperationTarget::InboxReply { conversation_id } => {
            messages_of(client, *conversation_id).await?
        }
        OperationTarget::InboxSend { .. } => {
            let Some(conversation_id) = row
                .response
                .as_ref()
                .and_then(|r| r.conversation_id.as_deref())
                .and_then(|id| id.parse::<i64>().ok())
            else {
                // Canvas never named a conversation, so there is no thread to
                // read. Nothing is concluded from that.
                return Ok(empty(false));
            };
            messages_of(client, conversation_id).await?
        }
    };
    let scanned = u32::try_from(objects.len()).unwrap_or(u32::MAX);
    let mut readback = OperationReadback {
        read_at,
        scanned,
        complete: true,
        ..OperationReadback::default()
    };

    // Prefer the object the acceptance named. Fall back to a message that
    // carries the digest this operation sent, which is the weaker evidence.
    let wanted_id = row.response.as_ref().and_then(|r| r.id.clone());
    let digest = sent_digest(row);
    let found = objects
        .iter()
        .find(|object| wanted_id.is_some() && json_id(object.get("id")) == wanted_id)
        .or_else(|| {
            objects
                .iter()
                .rev()
                .find(|object| body_digest(object).as_deref() == Some(digest.as_str()))
        });
    if let Some(object) = found {
        readback.id = json_id(object.get("id"));
        readback.created_at = object
            .get("created_at")
            .and_then(Value::as_str)
            .map(str::to_owned);
        readback.created_at_local = super::receipt::local(readback.created_at.as_deref());
        readback.user_id = json_id(object.get("user_id").or_else(|| object.get("author_id")));
        readback.body_sha256 = body_digest(object);
        readback.attachment_ids = object
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| json_id(a.get("id")))
            .collect();
    }
    Ok(readback)
}

/// The digest of what this operation sent, as Canvas would echo it.
fn sent_digest(row: &OperationRow) -> String {
    hex_sha256(row.intended.body.outbound_bytes.as_bytes())
}

fn body_digest(object: &Value) -> Option<String> {
    object
        .get("message")
        .or_else(|| object.get("body"))
        .and_then(Value::as_str)
        .map(|text| hex_sha256(text.as_bytes()))
}

/// A digest-only candidate, when the readback found one and no id links it.
fn match_of(row: &OperationRow, readback: &OperationReadback) -> Option<ServerMatch> {
    let id = readback.id.clone()?;
    let digest = readback.body_sha256.clone()?;
    if digest != sent_digest(row) {
        return None;
    }
    Some(ServerMatch {
        id,
        created_at: readback.created_at.clone(),
        created_at_local: readback.created_at_local.clone(),
        user_id: readback.user_id.clone(),
        body_sha256: digest,
    })
}

async fn messages_of(client: &Client, conversation_id: i64) -> Result<Vec<Value>, OperationError> {
    let conversation = get(
        client,
        &format!("/api/v1/conversations/{conversation_id}?auto_mark_as_read=false"),
    )
    .await?;
    Ok(conversation
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

//! Plan and approval-handle storage.
//!
//! Every state change is one `BEGIN IMMEDIATE` transaction with an
//! expected-state guard, the same discipline SPEC §12.2 sets for the journal:
//! zero rows affected means another actor moved the plan first.

use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

use crate::events::EventKind;
use crate::store::{DbError, Store};
use crate::submit::InputKind;

use super::record::{Approval, ApprovalChannel, Observations, PlanRow, PlanState};
use super::{HandleRefusal, PlanError};

/// How long a prepared plan may wait for approval (REPORT §3.5).
pub const EXPIRY: jiff::SignedDuration = jiff::SignedDuration::from_mins(15);

/// The columns [`load`] reads, in order.
const COLUMNS: &str = "plan_id, identity_key, identity_generation, consumer, course_id,
     assignment_id, kind, payload_json, file_paths_json, input_sha256, sent_sha256,
     baseline_attempt, baseline_submission_id, observations_json, plan_sha256, state,
     created_at, expires_at, approval_json, journal_id, invalidated_reason";

fn row_from(row: &rusqlite::Row<'_>) -> Result<PlanRow, rusqlite::Error> {
    let kind: String = row.get(6)?;
    let payload: String = row.get(7)?;
    let paths: String = row.get(8)?;
    let observations: String = row.get(13)?;
    let state: String = row.get(15)?;
    let approval: Option<String> = row.get(18)?;
    let decode = |what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(DbError::Message(format!("unreadable plan {what}"))),
        )
    };
    Ok(PlanRow {
        plan_id: row.get(0)?,
        identity_key: row.get(1)?,
        identity_generation: row.get(2)?,
        consumer: row.get(3)?,
        course_id: row.get(4)?,
        assignment_id: row.get(5)?,
        kind: InputKind::parse_plan_name(&kind).ok_or_else(|| decode("kind"))?,
        payload: serde_json::from_str(&payload).map_err(|_| decode("payload"))?,
        file_paths: serde_json::from_str(&paths).map_err(|_| decode("file paths"))?,
        input_sha256: row.get(9)?,
        sent_sha256: row.get(10)?,
        baseline_attempt: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
        baseline_submission_id: row.get(12)?,
        observations: serde_json::from_str(&observations).map_err(|_| decode("observations"))?,
        plan_sha256: row.get(14)?,
        state: PlanState::parse(&state).ok_or_else(|| decode("state"))?,
        created_at: row.get(16)?,
        expires_at: row.get(17)?,
        approval: approval
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(|_| decode("approval"))?,
        journal_id: row.get(19)?,
        invalidated_reason: row.get(20)?,
    })
}

/// What [`insert`] needs. The digest is computed from the assembled row.
pub struct NewPlan {
    pub identity_key: String,
    pub identity_generation: String,
    pub consumer: Option<String>,
    pub course_id: i64,
    pub assignment_id: i64,
    pub kind: InputKind,
    pub payload: crate::journal::IntendedPayload,
    pub file_paths: Vec<String>,
    pub baseline_attempt: i64,
    pub baseline_submission_id: Option<i64>,
    pub observations: Observations,
}

/// Freeze a plan into the `prepared` state and return the stored row.
pub fn insert(store: &Store, new: NewPlan, now: Timestamp) -> Result<PlanRow, PlanError> {
    let text = new.payload.text.as_ref();
    let mut row = PlanRow {
        plan_id: Uuid::new_v4().to_string(),
        identity_key: new.identity_key,
        identity_generation: new.identity_generation,
        consumer: new.consumer,
        course_id: new.course_id,
        assignment_id: new.assignment_id,
        kind: new.kind,
        input_sha256: text.map(|t| t.input_sha256.clone()),
        sent_sha256: text.map(|t| t.sent_sha256.clone()),
        payload: new.payload,
        file_paths: new.file_paths,
        baseline_attempt: new.baseline_attempt,
        baseline_submission_id: new.baseline_submission_id,
        observations: new.observations,
        plan_sha256: String::new(),
        state: PlanState::Prepared,
        created_at: now.to_string(),
        expires_at: (now + EXPIRY).to_string(),
        approval: None,
        journal_id: None,
        invalidated_reason: None,
    };
    row.plan_sha256 = row.digest();

    let stored = row.clone();
    let payload_json = serde_json::to_string(&stored.payload)?;
    let paths_json = serde_json::to_string(&stored.file_paths)?;
    let observations_json = serde_json::to_string(&stored.observations)?;
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key: String =
            tx.query_row("SELECT value FROM identity WHERE key = 'key'", [], |r| {
                r.get(0)
            })?;
        if key != stored.identity_key {
            return Err(DbError::Message("identity mismatch".into()));
        }
        tx.execute(
            "INSERT INTO plans (
                plan_id, identity_key, identity_generation, consumer, course_id,
                assignment_id, kind, payload_json, file_paths_json, input_sha256,
                sent_sha256, baseline_attempt, baseline_submission_id, observations_json,
                plan_sha256, state, created_at, expires_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'prepared',?16,?17)",
            params![
                stored.plan_id,
                stored.identity_key,
                stored.identity_generation,
                stored.consumer,
                stored.course_id,
                stored.assignment_id,
                stored.kind.plan_name(),
                payload_json,
                paths_json,
                stored.input_sha256,
                stored.sent_sha256,
                stored.baseline_attempt,
                stored.baseline_submission_id,
                observations_json,
                stored.plan_sha256,
                stored.created_at,
                stored.expires_at,
            ],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(row)
}

/// Read one plan.
pub fn load(store: &Store, plan_id: &str) -> Result<Option<PlanRow>, PlanError> {
    let plan_id = plan_id.to_owned();
    let row = store.call_blocking(move |conns| {
        Ok(conns
            .state
            .query_row(
                &format!("SELECT {COLUMNS} FROM plans WHERE plan_id = ?1"),
                [&plan_id],
                row_from,
            )
            .optional()?)
    })?;
    Ok(row)
}

/// Read one plan or fail with [`PlanError::NotFound`].
pub fn require(store: &Store, plan_id: &str) -> Result<PlanRow, PlanError> {
    load(store, plan_id)?.ok_or(PlanError::NotFound)
}

/// The current identity generation recorded in the state database.
pub fn identity_generation(store: &Store) -> Result<String, PlanError> {
    let value = store.call_blocking(|conns| {
        Ok(conns
            .state
            .query_row(
                "SELECT value FROM identity WHERE key = 'generation'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    })?;
    value.ok_or_else(|| PlanError::Store(DbError::Message("no identity generation".into())))
}

/// Issue a random approval handle bound to one plan, consumer, and expiry.
///
/// The handle is a version-4 UUID: a 128-bit value carrying 122 bits from the
/// operating system's random source. It is the server-issued secret REPORT
/// §3.5 requires; echoing a plan digest is not a substitute for holding it.
pub fn issue_handle(
    store: &Store,
    plan_id: &str,
    consumer: Option<&str>,
) -> Result<String, PlanError> {
    let plan = require(store, plan_id)?;
    if plan.state != PlanState::Prepared {
        return Err(PlanError::Refused {
            reason: refusal_for(plan.state),
            message: format!("plan is {}", plan.state),
        });
    }
    let handle = Uuid::new_v4().simple().to_string();
    let stored = handle.clone();
    let plan_id = plan_id.to_owned();
    let consumer = consumer.map(str::to_owned);
    let expires_at = plan.expires_at.clone();
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO approval_handles (handle, plan_id, consumer, expires_at)
             VALUES (?1,?2,?3,?4)",
            params![stored, plan_id, consumer, expires_at],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(handle)
}

/// Record a human decision against a plan.
///
/// Everything is validated inside one immediate transaction, so two callers
/// racing the same handle cannot both succeed: the first marks it used and the
/// second finds `used_at` set.
pub fn approve(
    store: &Store,
    plan_id: &str,
    handle: &str,
    channel: ApprovalChannel,
    consumer: Option<&str>,
    now: Timestamp,
) -> Result<PlanRow, PlanError> {
    let plan = require(store, plan_id)?;
    guard_admission(&plan, now)?;
    if plan.state != PlanState::Prepared {
        return Err(PlanError::Refused {
            reason: refusal_for(plan.state),
            message: format!("plan is {}", plan.state),
        });
    }
    // The stored digest must still describe the stored plan, and the plan must
    // still belong to the identity generation it was frozen under.
    if plan.digest() != plan.plan_sha256 {
        return Err(PlanError::Handle(HandleRefusal::DigestMismatch));
    }
    if identity_generation(store)? != plan.identity_generation {
        return Err(PlanError::Invalidated {
            reason: "identity generation changed".into(),
        });
    }

    let approval = Approval {
        channel,
        at: now.to_string(),
        consumer: consumer.map(str::to_owned),
        plan_sha256: plan.plan_sha256.clone(),
    };
    let approval_json = serde_json::to_string(&approval)?;
    let handle = handle.to_owned();
    let plan_key = plan_id.to_owned();
    let expected_consumer = consumer.map(str::to_owned);
    let now_text = now.to_string();

    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(String, Option<String>, String, Option<String>)> = tx
            .query_row(
                "SELECT plan_id, consumer, expires_at, used_at
                 FROM approval_handles WHERE handle = ?1",
                [&handle],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((handle_plan, handle_consumer, handle_expires, used_at)) = found else {
            return Err(handle_refusal(HandleRefusal::Unknown));
        };
        if handle_plan != plan_key {
            return Err(handle_refusal(HandleRefusal::OtherPlan));
        }
        if used_at.is_some() {
            return Err(handle_refusal(HandleRefusal::AlreadyUsed));
        }
        if handle_consumer != expected_consumer {
            return Err(handle_refusal(HandleRefusal::WrongConsumer));
        }
        if handle_expires
            .parse::<Timestamp>()
            .is_ok_and(|deadline| now >= deadline)
        {
            return Err(handle_refusal(HandleRefusal::Expired));
        }
        let used = tx.execute(
            "UPDATE approval_handles SET used_at = ?1 WHERE handle = ?2 AND used_at IS NULL",
            params![now_text, handle],
        )?;
        if used != 1 {
            return Err(handle_refusal(HandleRefusal::AlreadyUsed));
        }
        let moved = tx.execute(
            "UPDATE plans SET state = 'approved', approval_json = ?1
             WHERE plan_id = ?2 AND state = 'prepared'",
            params![approval_json, plan_key],
        )?;
        if moved != 1 {
            return Err(DbError::Message("plan moved".into()));
        }
        record_plan_decision(&tx, EventKind::PlanApproved, &plan_key)?;
        tx.commit()?;
        Ok(())
    })?;
    require(store, plan_id)
}

/// Invalidate a plan and every handle issued for it.
pub fn invalidate(store: &Store, plan_id: &str, reason: &str) -> Result<PlanRow, PlanError> {
    invalidate_recording(store, plan_id, reason, None)
}

/// Invalidate a plan, optionally recording the person's decision as an event.
///
/// The event commits with the state change, so a decision the log names is a
/// decision the plan row already carries. It records the plan id and nothing
/// else: what was in the plan stays in the plan (REPORT §3.5).
///
/// `None` is the invalidation `execute` performs when a meaningful fact
/// changed. That is not a decision, so it produces no decision event.
fn invalidate_recording(
    store: &Store,
    plan_id: &str,
    reason: &str,
    event: Option<EventKind>,
) -> Result<PlanRow, PlanError> {
    let plan_key = plan_id.to_owned();
    let reason = reason.to_owned();
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // An executed plan is history; it is never rewritten.
        let moved = tx.execute(
            "UPDATE plans SET state = 'invalidated', invalidated_reason = ?1
             WHERE plan_id = ?2 AND state IN ('prepared','approved','expired')",
            params![reason, plan_key],
        )?;
        tx.execute(
            "UPDATE approval_handles SET used_at = COALESCE(used_at, ?1)
             WHERE plan_id = ?2",
            params![jiff::Timestamp::now().to_string(), plan_key],
        )?;
        if let Some(kind) = event
            && moved == 1
        {
            record_plan_decision(&tx, kind, &plan_key)?;
        }
        tx.commit()?;
        Ok(())
    })?;
    require(store, plan_id)
}

/// A human declined this plan.
///
/// Declining and cancelling both invalidate the plan and spend every handle
/// issued for it, so neither the plan nor a handle can be replayed.
pub fn decline(store: &Store, plan_id: &str) -> Result<PlanRow, PlanError> {
    invalidate_recording(store, plan_id, "declined", Some(EventKind::PlanDeclined))
}

/// The requester withdrew this plan.
pub fn cancel(store: &Store, plan_id: &str) -> Result<PlanRow, PlanError> {
    invalidate_recording(store, plan_id, "cancelled", Some(EventKind::PlanCancelled))
}

/// Append one `plan.approved|declined|cancelled` event inside a transaction.
///
/// The payload is the plan id and the decision, and nothing else: no target,
/// no digest, no bytes. The panel shows the person what they decided; the log
/// records only that a decision happened (REPORT §3.5).
fn record_plan_decision(
    tx: &rusqlite::Transaction<'_>,
    kind: EventKind,
    plan_id: &str,
) -> Result<(), DbError> {
    let who = crate::events::identity(tx)?;
    let assignment: Option<i64> = tx
        .query_row(
            "SELECT assignment_id FROM plans WHERE plan_id = ?1",
            [plan_id],
            |r| r.get(0),
        )
        .optional()?;
    let scope = assignment.map_or_else(|| "plan".to_owned(), |id| format!("assignment:{id}"));
    let decision = kind.as_str().trim_start_matches("plan.").to_owned();
    crate::events::insert_decision(
        tx,
        &who,
        &format!("plan:{plan_id}:{decision}"),
        &jiff::Timestamp::now().to_string(),
        "plans",
        &scope,
        kind,
        plan_id,
        &decision,
    )
}

/// Every prepared plan that still has an unspent handle, oldest first.
///
/// This is what the companion's panel shows: a plan a person can still decide
/// on, with the handle that was issued for it. A plan with no handle is not
/// waiting on a person; nobody has asked yet.
pub fn awaiting_decision(store: &Store, now: Timestamp) -> Result<Vec<Awaiting>, PlanError> {
    let now_text = now.to_string();
    let rows: Vec<(String, String, Option<String>)> = store.call_blocking(move |conns| {
        let mut stmt = conns.state.prepare(
            "SELECT h.handle, h.plan_id, h.consumer
             FROM approval_handles h JOIN plans p ON p.plan_id = h.plan_id
             WHERE h.used_at IS NULL AND p.state = 'prepared' AND h.expires_at > ?1
             ORDER BY p.created_at ASC, h.handle ASC",
        )?;
        Ok(stmt
            .query_map([&now_text], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?)
    })?;
    let mut out = Vec::new();
    for (handle, plan_id, consumer) in rows {
        if let Some(plan) = load(store, &plan_id)? {
            out.push(Awaiting {
                handle,
                consumer,
                plan,
            });
        }
    }
    Ok(out)
}

/// One plan waiting for a decision, with the handle issued for it.
#[derive(Debug, Clone)]
pub struct Awaiting {
    /// The server-issued handle. A decision must carry it back.
    pub handle: String,
    /// The consumer the **handle** was issued to, which is what `approve`
    /// checks. It is not always the consumer that prepared the plan: the
    /// handle is issued when somebody asks for a decision.
    pub consumer: Option<String>,
    /// The frozen plan.
    pub plan: PlanRow,
}

/// Mark a plan expired. Only a plan still waiting can expire.
pub fn expire(store: &Store, plan_id: &str) -> Result<PlanRow, PlanError> {
    let plan_key = plan_id.to_owned();
    store.call_blocking(move |conns| {
        let tx = conns
            .state
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE plans SET state = 'expired'
             WHERE plan_id = ?1 AND state IN ('prepared','approved')",
            [&plan_key],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    require(store, plan_id)
}

/// The refusal reason a non-admissible state maps to (REPORT §3.2).
pub fn refusal_for(state: PlanState) -> &'static str {
    match state {
        PlanState::Expired => "expired",
        PlanState::Invalidated => "invalidated",
        _ => "approval_required",
    }
}

/// Refuse a plan that can no longer be admitted.
///
/// Expiry is evaluated here, at admission, and nowhere else: a status read of
/// an executed plan must never turn it into an expired one (REPORT §3.5).
pub fn guard_admission(plan: &PlanRow, now: Timestamp) -> Result<(), PlanError> {
    match plan.state {
        PlanState::Expired => {
            return Err(PlanError::Refused {
                reason: "expired",
                message: "plan expired before it was executed".into(),
            });
        }
        PlanState::Invalidated => {
            return Err(PlanError::Refused {
                reason: "invalidated",
                message: plan
                    .invalidated_reason
                    .clone()
                    .unwrap_or_else(|| "plan invalidated".into()),
            });
        }
        _ => {}
    }
    if plan.is_expired(now) {
        return Err(PlanError::Refused {
            reason: "expired",
            message: format!("plan expired at {}", plan.expires_at),
        });
    }
    Ok(())
}

/// Encode a handle refusal so it survives the store's error channel.
fn handle_refusal(refusal: HandleRefusal) -> DbError {
    DbError::Message(format!("{HANDLE_PREFIX}{}", refusal.as_str()))
}

const HANDLE_PREFIX: &str = "approval-handle:";

/// Recover a handle refusal from a store error.
pub fn handle_refusal_from(error: &DbError) -> Option<HandleRefusal> {
    let DbError::Message(text) = error else {
        return None;
    };
    HandleRefusal::parse(text.strip_prefix(HANDLE_PREFIX)?)
}

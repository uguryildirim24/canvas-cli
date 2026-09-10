//! The shared `governor` row in `state.sqlite` (SPEC §11, REPORT §3.6).
//!
//! The row is read before admission and written after each response. Every
//! write is one `BEGIN IMMEDIATE` transaction around a pure merge closure
//! supplied by `canvas-api`, so no network wait ever happens inside it.

use std::sync::{Arc, Mutex};

use canvas_api::{GovernorSnapshot, GovernorState};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// `governor` row access for one identity.
pub struct SharedGovernorState {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SharedGovernorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedGovernorState")
            .finish_non_exhaustive()
    }
}

impl SharedGovernorState {
    /// Wrap a `state.sqlite` connection.
    #[must_use]
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

fn read(conn: &Connection) -> rusqlite::Result<Option<GovernorSnapshot>> {
    conn.query_row(
        "SELECT estimate, watermark, cooldown_until, refill, updated_at
         FROM governor WHERE id = 1",
        [],
        |r| {
            Ok(GovernorSnapshot {
                estimate: r.get(0)?,
                watermark: r.get::<_, i64>(1)?.try_into().unwrap_or(0),
                cooldown_until: r.get(2)?,
                refill: r.get(3)?,
                updated_at: r.get(4)?,
            })
        },
    )
    .optional()
}

impl GovernorState for SharedGovernorState {
    fn load(&self) -> Option<GovernorSnapshot> {
        let conn = self.conn.lock().ok()?;
        match read(&conn) {
            Ok(row) => row,
            Err(error) => {
                tracing::debug!(%error, "cannot read the shared governor row");
                None
            }
        }
    }

    fn update(&self, merge: &mut dyn FnMut(Option<GovernorSnapshot>) -> Option<GovernorSnapshot>) {
        let Ok(mut conn) = self.conn.lock() else {
            return;
        };
        let result = (|| -> rusqlite::Result<()> {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let stored = read(&tx)?;
            let Some(next) = merge(stored) else {
                return Ok(());
            };
            tx.execute(
                "INSERT INTO governor (id, estimate, watermark, cooldown_until, refill, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    estimate = excluded.estimate,
                    watermark = excluded.watermark,
                    cooldown_until = excluded.cooldown_until,
                    refill = excluded.refill,
                    updated_at = excluded.updated_at",
                params![
                    next.estimate,
                    i64::try_from(next.watermark).unwrap_or(i64::MAX),
                    next.cooldown_until,
                    next.refill,
                    next.updated_at,
                ],
            )?;
            tx.commit()
        })();
        if let Err(error) = result {
            // A governor row this process cannot write only costs sharing; the
            // §11 rules still hold locally, so the request is not failed here.
            tracing::debug!(%error, "cannot update the shared governor row");
        }
    }
}

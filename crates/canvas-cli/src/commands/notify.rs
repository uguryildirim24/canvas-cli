//! `canvas notify`: desktop alerts derived from the event log (REPORT §3.6).
//!
//! Notify has one data source: the `events` table item 4 writes. It never
//! reads Canvas, never refreshes a dataset, and never invents a change. It
//! reads the events after its cursor, groups them by event kind group, and
//! posts one notification per group. The position is durable in
//! `consumer_cursor`, so a second run posts nothing the first one posted;
//! `--since` overrides the stored position for one run.
//!
//! **Backend.** `--stdout` is the only backend. `notify-rust` is not used: its
//! macOS path goes through `mac-notification-sys`, which is Objective-C FFI,
//! so it adds unsafe code to a workspace that has none. Without `--stdout` the
//! command says the desktop backend is unavailable and writes the same lines
//! to stdout, so it never claims a notification it did not post.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process::ExitCode;

use canvas_core::events::{
    CursorCheck, EventKind, EventRecord, check_cursor, consumer_cursor, read_after,
    set_consumer_cursor,
};

use super::Globals;
use super::emit::{emit_error, session_error};

/// The consumer name this command keeps its cursor under.
const CONSUMER: &str = "notify";
/// How many events one read takes.
const BATCH: usize = 500;

/// Run `canvas notify`.
pub async fn run(globals: &Globals, since: Option<String>, stdout: bool) -> ExitCode {
    let since = match since.as_deref().map(parse_cursor) {
        None => None,
        Some(Ok(cursor)) => Some(cursor),
        Some(Err(message)) => {
            return emit_error(false, "usage", &message, 2, globals.profile.clone(), None);
        }
    };

    // Notify reads the local log, so it needs no token and no network.
    let session = match globals.open_local_session() {
        Ok(s) => s,
        Err(e) => return session_error(false, e, globals.profile.clone()),
    };
    let generation = session.identity.generation.to_string();

    let start = match since {
        Some(cursor) => cursor,
        None => match session
            .open
            .store
            .call_blocking(|conns| consumer_cursor(&conns.state, CONSUMER))
        {
            Ok(cursor) => cursor,
            Err(e) => return local_error(globals, &e.to_string()),
        },
    };

    // The §3.2 rule is the same for every consumer: a cursor this log cannot
    // replay asks for a resync and closes normally.
    let check = {
        let generation = generation.clone();
        session
            .open
            .store
            .call_blocking(move |conns| check_cursor(&conns.state, start, &generation))
    };
    match check {
        Ok(CursorCheck::Resync) => {
            let mut out = io::stdout().lock();
            let _ = writeln!(
                out,
                "resync_required: cursor {start} can no longer be replayed"
            );
            return ExitCode::SUCCESS;
        }
        Ok(CursorCheck::Replay) => {}
        Err(e) => return local_error(globals, &e.to_string()),
    }

    let mut cursor = start;
    let mut groups: BTreeMap<&'static str, Group> = BTreeMap::new();
    loop {
        let batch = match session
            .open
            .store
            .call_blocking(move |conns| read_after(&conns.state, cursor, BATCH))
        {
            Ok(batch) => batch,
            Err(e) => return local_error(globals, &e.to_string()),
        };
        if batch.is_empty() {
            break;
        }
        for record in &batch {
            cursor = record.cursor;
            let Some(kind) = EventKind::parse(&record.kind) else {
                continue;
            };
            groups.entry(kind.group()).or_default().add(record);
        }
    }

    if !stdout {
        let _ = writeln!(
            io::stderr(),
            "warning: no desktop notification backend is available; writing to stdout"
        );
    }
    let mut out = io::stdout().lock();
    for (group, entry) in &groups {
        if writeln!(out, "{}", entry.line(group)).is_err() {
            return ExitCode::from(1);
        }
    }
    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    drop(out);

    // The position moves only after the notifications are written, so a failed
    // run repeats them instead of dropping them.
    if cursor > start
        && let Err(e) = session
            .open
            .store
            .call_blocking(move |conns| set_consumer_cursor(&conns.state, CONSUMER, cursor))
    {
        return local_error(globals, &e.to_string());
    }
    ExitCode::SUCCESS
}

/// One notification: how many events, and the newest cursor in the group.
#[derive(Default)]
struct Group {
    count: u64,
    cursor: i64,
    kinds: BTreeMap<String, u64>,
}

impl Group {
    fn add(&mut self, record: &EventRecord) {
        self.count += 1;
        self.cursor = self.cursor.max(record.cursor);
        *self.kinds.entry(record.kind.clone()).or_default() += 1;
    }

    fn line(&self, group: &str) -> String {
        let detail = self
            .kinds
            .iter()
            .map(|(kind, count)| format!("{kind} x{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{group}: {} event(s) through cursor {} ({detail})",
            self.count, self.cursor
        )
    }
}

fn parse_cursor(raw: &str) -> Result<i64, String> {
    raw.parse::<i64>()
        .ok()
        .filter(|cursor| *cursor >= 0)
        .ok_or_else(|| format!("--since expects a decimal cursor, not {raw:?}"))
}

fn local_error(globals: &Globals, message: &str) -> ExitCode {
    emit_error(false, "local", message, 13, globals.profile.clone(), None)
}

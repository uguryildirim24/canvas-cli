//! Schema migrations.
//!
//! Each migration is a numbered batch. A database records how far it has come
//! in `PRAGMA user_version`, and the opener passes that number in, so a batch
//! runs once and only on a database that has not seen it.
//!
//! | # | Database | Package | Adds |
//! |---|---|---|---|
//! | `0001_initial` | both | M1-a | the v1 cache and state schema |
//! | `0002_plans` | state | M6-a | `plans`, `approval_handles`, the journal plan link |
//! | `0002_reads` | cache | M8-a | `pages`, `discussion_topics`, `discussion_entries`, `conversations` |

use rusqlite::Connection;

use super::db::DbError;

/// Current cache.sqlite schema version.
pub const CACHE_USER_VERSION: i32 = 2;
/// Current state.sqlite schema version.
pub const STATE_USER_VERSION: i32 = 2;

/// Apply cache migrations from `from` up to [`CACHE_USER_VERSION`].
pub fn migrate_cache(conn: &Connection, from: i32) -> Result<(), DbError> {
    if from < 1 {
        conn.execute_batch(CACHE_0001)?;
    }
    if from < 2 {
        conn.execute_batch(CACHE_0002)?;
    }
    Ok(())
}

/// Apply state migrations from `from` up to [`STATE_USER_VERSION`].
pub fn migrate_state(conn: &Connection, from: i32) -> Result<(), DbError> {
    if from < 1 {
        conn.execute_batch(STATE_0001)?;
    }
    if from < 2 {
        conn.execute_batch(STATE_0002)?;
    }
    Ok(())
}

/// Richer read entities: pages, discussions, and the inbox (M8-a).
///
/// Every entity is keyed by its Canvas id, as the v1 tables are. A page keeps
/// its `url` slug in a column as well, because a course addresses its pages by
/// slug and the coverage scope names whichever form the caller asked for.
const CACHE_0002: &str = r"
CREATE TABLE pages (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    url TEXT,
    title TEXT,
    body TEXT,
    updated_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE INDEX pages_course_url ON pages(course_id, url);

CREATE TABLE discussion_topics (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    title TEXT,
    message TEXT,
    posted_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE discussion_entries (
    id INTEGER PRIMARY KEY NOT NULL,
    topic_id INTEGER,
    parent_id INTEGER,
    user_id INTEGER,
    message TEXT,
    created_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE INDEX discussion_entries_topic ON discussion_entries(topic_id);

CREATE TABLE conversations (
    id INTEGER PRIMARY KEY NOT NULL,
    subject TEXT,
    workflow_state TEXT,
    last_message_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE conversation_unread (
    id INTEGER PRIMARY KEY NOT NULL,
    unread_count INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);
";

/// Cache entity and coverage tables (SPEC §10).
const CACHE_0001: &str = r"
CREATE TABLE courses (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT,
    course_code TEXT,
    workflow_state TEXT,
    term_id INTEGER,
    html_url TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE terms (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT,
    start_at TEXT,
    end_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE assignment_groups (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    name TEXT,
    position INTEGER,
    group_weight REAL,
    rules_json TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE assignments (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    name TEXT,
    due_at TEXT,
    unlock_at TEXT,
    lock_at TEXT,
    points_possible REAL,
    html_url TEXT,
    description TEXT,
    submission_types TEXT,
    allowed_extensions TEXT,
    allowed_attempts INTEGER,
    rubric_json TEXT,
    can_submit INTEGER,
    submitted INTEGER,
    graded INTEGER,
    score REAL,
    late INTEGER,
    missing INTEGER,
    excused INTEGER,
    workflow_state TEXT,
    attempt INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE submissions (
    id INTEGER PRIMARY KEY NOT NULL,
    assignment_id INTEGER,
    user_id INTEGER,
    attempt INTEGER,
    score REAL,
    grade TEXT,
    submitted_at TEXT,
    workflow_state TEXT,
    late INTEGER,
    missing INTEGER,
    excused INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE modules (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    name TEXT,
    position INTEGER,
    items_count INTEGER,
    items_complete INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE module_items (
    id INTEGER PRIMARY KEY NOT NULL,
    module_id INTEGER,
    course_id INTEGER,
    title TEXT,
    position INTEGER,
    content_id INTEGER,
    type TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE folders (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    name TEXT,
    full_name TEXT,
    parent_folder_id INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE files (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    folder_id INTEGER,
    display_name TEXT,
    filename TEXT,
    size INTEGER,
    content_type TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE announcements (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    title TEXT,
    message TEXT,
    posted_at TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE calendar_events (
    id INTEGER PRIMARY KEY NOT NULL,
    title TEXT,
    start_at TEXT,
    end_at TEXT,
    context_code TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE planner_items (
    id TEXT PRIMARY KEY NOT NULL,
    plannable_id INTEGER,
    plannable_type TEXT,
    course_id INTEGER,
    title TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE enrollment_grades (
    enrollment_id INTEGER NOT NULL,
    period TEXT NOT NULL,
    course_id INTEGER,
    current_score REAL,
    final_score REAL,
    current_grade TEXT,
    final_grade TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT,
    PRIMARY KEY (enrollment_id, period)
);

CREATE TABLE course_totals (
    course_id INTEGER NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('all', 'current')),
    current_score REAL,
    final_score REAL,
    current_grade TEXT,
    final_grade TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT,
    PRIMARY KEY (course_id, mode)
);

CREATE TABLE grading_periods (
    id INTEGER PRIMARY KEY NOT NULL,
    course_id INTEGER,
    title TEXT,
    start_date TEXT,
    end_date TEXT,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

-- Test dataset entity table (FakeEntity).
CREATE TABLE fake_entities (
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT,
    due_at TEXT,
    description TEXT,
    score REAL,
    can_submit INTEGER,
    data_json TEXT NOT NULL DEFAULT '{}',
    observed_at_core TEXT,
    observed_at_detail TEXT,
    observed_at_status TEXT
);

CREATE TABLE field_obs (
    entity_kind TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    field TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    PRIMARY KEY (entity_kind, entity_key, field)
);

CREATE TABLE membership (
    dataset TEXT NOT NULL,
    scope TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (dataset, scope, entity_kind, entity_id)
);

CREATE TABLE fetch_log (
    dataset TEXT NOT NULL,
    scope TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    complete INTEGER NOT NULL DEFAULT 0,
    count INTEGER NOT NULL DEFAULT 0,
    stale INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    epoch_seen INTEGER NOT NULL DEFAULT 0,
    contexts TEXT,
    window_start TEXT,
    window_end TEXT,
    PRIMARY KEY (dataset, scope)
);
";

/// Durable state tables (SPEC §10, §12.2, §12.3).
const STATE_0001: &str = r"
CREATE TABLE scope_epoch (
    scope TEXT PRIMARY KEY NOT NULL,
    epoch INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE credential (
    identity_key TEXT PRIMARY KEY NOT NULL,
    active_source TEXT NOT NULL DEFAULT 'none' CHECK (active_source IN ('keyring', 'file', 'none')),
    token_sha256 TEXT,
    validated_at TEXT,
    cleanup_keyring INTEGER NOT NULL DEFAULT 0,
    cleanup_file INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE submission_journal (
    journal_id TEXT PRIMARY KEY NOT NULL,
    identity_key TEXT NOT NULL,
    course_id INTEGER NOT NULL,
    assignment_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned', 'uploading', 'uploaded', 'posting',
        'submitted', 'matched', 'upload_incomplete', 'uploaded_not_submitted', 'outcome_unknown', 'refused')),
    intended_payload_json TEXT NOT NULL DEFAULT '{}',
    baseline_attempt INTEGER,
    baseline_submission_id INTEGER,
    created_at TEXT NOT NULL,
    planned_at TEXT,
    uploading_at TEXT,
    uploaded_at TEXT,
    posting_at TEXT,
    posting_started_at TEXT,
    submitted_at TEXT,
    matched_at TEXT,
    terminal_at TEXT,
    upload_incomplete_at TEXT,
    uploaded_not_submitted_at TEXT,
    outcome_unknown_at TEXT,
    refused_at TEXT,
    uploaded_file_ids_json TEXT NOT NULL DEFAULT '[]',
    post_status INTEGER,
    response_kind TEXT CHECK (response_kind IN ('canvas-error', 'other', 'none')),
    not_submitted_evidence TEXT CHECK (not_submitted_evidence IN ('never_sent', 'assumed')),
    response_record_json TEXT,
    readback_record_json TEXT,
    server_match_json TEXT,
    receipt_record_json TEXT,
    acknowledged_at TEXT,
    error_text TEXT
);

CREATE UNIQUE INDEX submission_journal_one_active
ON submission_journal(assignment_id)
WHERE state IN ('planned','uploading','uploaded','posting');

CREATE TABLE alias (
    name TEXT PRIMARY KEY NOT NULL,
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE identity (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE destinations (
    dest_id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL,
    root_fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL
);
";

/// Operation plans and the approval record (M6-a; REPORT §3.5).
///
/// `plan_sha256` covers the canonical plan document, so an approval names
/// exact bytes. The journal link is unique: one plan can admit at most one
/// journal, whatever else races. Legacy journal rows keep `plan_id` and
/// `approval_json` null, which Appendix D's nullable convention exposes as
/// `null` rather than invented approval evidence.
const STATE_0002: &str = r"
CREATE TABLE plans (
    plan_id TEXT PRIMARY KEY NOT NULL,
    identity_key TEXT NOT NULL,
    identity_generation TEXT NOT NULL,
    consumer TEXT,
    course_id INTEGER NOT NULL,
    assignment_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    file_paths_json TEXT NOT NULL DEFAULT '[]',
    input_sha256 TEXT,
    sent_sha256 TEXT,
    baseline_attempt INTEGER,
    baseline_submission_id INTEGER,
    observations_json TEXT NOT NULL,
    plan_sha256 TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('prepared','approved','executed','expired','invalidated')),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    approval_json TEXT,
    journal_id TEXT,
    invalidated_reason TEXT
);

CREATE INDEX plans_assignment ON plans(assignment_id, state);

CREATE TABLE approval_handles (
    handle TEXT PRIMARY KEY NOT NULL,
    plan_id TEXT NOT NULL REFERENCES plans(plan_id),
    consumer TEXT,
    expires_at TEXT NOT NULL,
    used_at TEXT
);

CREATE INDEX approval_handles_plan ON approval_handles(plan_id);

ALTER TABLE submission_journal ADD COLUMN plan_id TEXT;
ALTER TABLE submission_journal ADD COLUMN approval_json TEXT;

CREATE UNIQUE INDEX submission_journal_plan
ON submission_journal(plan_id)
WHERE plan_id IS NOT NULL;
";

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a database at exactly the schema version `version` describes.
    fn at_version(version: i32) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(STATE_0001).unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        conn
    }

    #[test]
    fn a_v1_state_database_gains_the_plan_tables_and_keeps_its_journals() {
        let conn = at_version(1);
        conn.execute(
            "INSERT INTO submission_journal
                (journal_id, identity_key, course_id, assignment_id, kind, state, created_at)
             VALUES ('legacy','k',1,2,'online_text_entry','submitted','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // The batch runs once, from the version the database has reached.
        migrate_state(&conn, 1).unwrap();

        // A journal written before plans exposes null, never invented evidence.
        let (plan_id, approval): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT plan_id, approval_json FROM submission_journal WHERE journal_id = 'legacy'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(plan_id, None);
        assert_eq!(approval, None);

        // The new tables and the one-journal-per-plan guard are present.
        for (kind, name) in [
            ("table", "plans"),
            ("table", "approval_handles"),
            ("index", "submission_journal_plan"),
        ] {
            let found: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                    rusqlite::params![kind, name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "{kind} {name} is missing");
        }
    }

    #[test]
    fn the_journal_plan_link_admits_one_journal_per_plan() {
        let conn = at_version(1);
        migrate_state(&conn, 1).unwrap();
        let insert = |journal_id: &str, plan: Option<&str>| {
            conn.execute(
                "INSERT INTO submission_journal
                    (journal_id, identity_key, course_id, assignment_id, kind, state, created_at, plan_id)
                 VALUES (?1,'k',1,2,'online_text_entry','submitted','2026-01-01T00:00:00Z',?2)",
                rusqlite::params![journal_id, plan],
            )
        };
        insert("a", Some("plan-1")).unwrap();
        insert("b", Some("plan-1")).unwrap_err();
        // Null plan ids stay distinct, so legacy rows never collide.
        insert("c", None).unwrap();
        insert("d", None).unwrap();
    }

    #[test]
    fn a_database_already_at_the_current_version_runs_no_batch() {
        // Re-running `0001` would fail on the tables it already created, so a
        // successful call is the proof that the gating works.
        let conn = at_version(1);
        migrate_state(&conn, 1).unwrap();
        migrate_state(&conn, STATE_USER_VERSION).unwrap();

        let cache = Connection::open_in_memory().unwrap();
        cache.execute_batch(CACHE_0001).unwrap();
        migrate_cache(&cache, CACHE_USER_VERSION).unwrap();
    }
}

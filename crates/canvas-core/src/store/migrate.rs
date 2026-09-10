//! Schema migrations. Owner of the migration list for M1-a: `0001_initial`.

use rusqlite::Connection;

use super::db::DbError;

/// Current cache.sqlite schema version.
pub const CACHE_USER_VERSION: i32 = 1;
/// Current state.sqlite schema version.
pub const STATE_USER_VERSION: i32 = 1;

/// Apply cache migrations up to [`CACHE_USER_VERSION`].
pub fn migrate_cache(conn: &Connection) -> Result<(), DbError> {
    // 0001_initial
    conn.execute_batch(CACHE_0001)?;
    Ok(())
}

/// Apply state migrations up to [`STATE_USER_VERSION`].
pub fn migrate_state(conn: &Connection) -> Result<(), DbError> {
    // 0001_initial
    conn.execute_batch(STATE_0001)?;
    Ok(())
}

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

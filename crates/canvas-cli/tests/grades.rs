//! M4-a `grades` CLI tests (SPEC §12.4, §16 rows 2-3).

use std::fs;
use std::process::Command as StdCommand;

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::Value;

fn bin() -> StdCommand {
    StdCommand::new(env!("CARGO_BIN_EXE_canvas"))
}

/// Three active courses: SYN-101 has grading periods, SYN-201 does not, and
/// SYN-301 is absent from the course list's totals (no `course_totals` row).
fn seed_courses(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO terms (id, name, start_at, end_at, data_json)
                 VALUES (7, 'Fall 2026', '2040-08-25T04:00:00Z', '2040-12-15T05:00:00Z', '{}')",
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO courses (id, name, course_code, html_url, term_id, data_json)
                 VALUES (
                    61001, 'Synthetic Computing', 'SYN-101',
                    'https://canvas.example.test/courses/61001', 7,
                    '{"enrollment_state":"active","is_favorite":true,"restricted":false,"has_grading_periods":1}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO courses (id, name, course_code, html_url, term_id, data_json)
                 VALUES (
                    61002, 'Synthetic Mathematics', 'SYN-201',
                    'https://canvas.example.test/courses/61002', 7,
                    '{"enrollment_state":"active","is_favorite":false,"restricted":false}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO courses (id, name, course_code, html_url, term_id, data_json)
                 VALUES (
                    61003, 'Synthetic Physics', 'SYN-301',
                    'https://canvas.example.test/courses/61003', 7,
                    '{"enrollment_state":"active","is_favorite":false,"restricted":false}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('courses', 'active', 'course', '61001', 0),
                        ('courses', 'active', 'course', '61002', 1),
                        ('courses', 'active', 'course', '61003', 2)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('courses', 'active', '2040-09-09T16:00:00Z', 1, 3, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

/// `course_totals` rows for both default modes, plus their observation clocks.
fn seed_totals(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                r#"INSERT INTO course_totals
                    (course_id, mode, current_score, current_grade, final_score, final_grade, data_json)
                 VALUES
                   (61001, 'all', 68.25, 'C+', 65.25, 'B', '{}'),
                   (61001, 'current', 73.25, 'C', 70.25, 'C',
                    '{"period_id":"5","period_title":"Synthetic Period One"}'),
                   (61002, 'all', NULL, NULL, NULL, NULL, '{}'),
                   (61002, 'current', NULL, NULL, NULL, NULL, '{}')"#,
                [],
            )?;
            for (key, field) in [
                ("61001|all", "current_score"),
                ("61001|all", "current_grade"),
                ("61001|all", "final_score"),
                ("61001|all", "final_grade"),
                ("61001|current", "current_score"),
                ("61001|current", "current_grade"),
                ("61001|current", "final_score"),
                ("61001|current", "final_grade"),
            ] {
                conns.cache.execute(
                    "INSERT INTO field_obs (entity_kind, entity_key, field, observed_at)
                     VALUES ('course_totals', ?1, ?2, '2040-09-09T16:00:00Z')",
                    rusqlite::params![key, field],
                )?;
            }
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('course_totals', 'course:61001', '2040-09-09T16:00:00Z', 1, 2, 0, 0),
                        ('course_totals', 'course:61002', '2040-09-09T16:00:00Z', 1, 2, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

/// `enrollment_grades` for the whole course and for period 5 only.
///
/// Courses 61001 and 61003 each hold two enrollments (a section change); only one
/// carries values, which is what the de-duplication rule must keep. Course 61003
/// has no `course_totals` row, so its overview total comes from that rule.
fn seed_enrollments(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO enrollment_grades
                    (enrollment_id, period, course_id, current_score, current_grade,
                     final_score, final_grade, data_json)
                 VALUES
                   (900, 'none', 61001, NULL, NULL, NULL, NULL, '{}'),
                   (901, 'none', 61001, 68.25, 'C+', 65.25, 'B', '{}'),
                   (902, 'none', 61002, NULL, NULL, NULL, NULL, '{}'),
                   (903, 'none', 61003, NULL, NULL, NULL, NULL, '{}'),
                   (904, 'none', 61003, 44.25, 'D', 40.25, 'F', '{}'),
                   (900, '5', 61001, 57.25, 'C+', 55.25, 'C', '{}'),
                   (901, '5', 61001, 57.25, 'C+', 55.25, 'C', '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('enrollment_grades', 'period:none', '2040-09-09T16:00:00Z', 1, 5, 0, 0),
                        ('enrollment_grades', 'period:5', '2040-09-09T16:00:00Z', 1, 2, 0, 0),
                        ('enrollment_grades', 'period:999', '2040-09-09T16:00:00Z', 1, 0, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn seed_periods(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                "INSERT INTO grading_periods (id, course_id, title, start_date, end_date, data_json)
                 VALUES (5, 61001, 'Synthetic Period One', '2040-09-01T00:00:00Z', '2040-10-31T00:00:00Z', '{}'),
                        (6, 61001, 'Synthetic Period Two', '2040-11-01T00:00:00Z', '2040-12-20T00:00:00Z', '{}')",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('grading_periods', 'course:61001', 'grading_period', '5', 0),
                        ('grading_periods', 'course:61001', 'grading_period', '6', 1)",
                [],
            )?;
            // Course 61002 has no grading periods; Canvas answers with an empty
            // list, which is complete coverage of zero rows.
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES ('grading_periods', 'course:61001', '2040-09-09T16:00:00Z', 1, 2, 0, 0),
                        ('grading_periods', 'course:61002', '2040-09-09T16:00:00Z', 1, 0, 0, 0),
                        ('grading_periods', 'course:61003', '2040-09-09T16:00:00Z', 1, 0, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

/// Two groups for period `none`; the second has no subtotal.
fn seed_groups(open: &OpenIdentity) {
    open.store
        .call_blocking(|conns| {
            conns.cache.execute(
                r#"INSERT INTO assignment_groups
                    (id, course_id, name, position, group_weight, rules_json, data_json)
                 VALUES (
                    20, 61001, 'Synthetic Exercises', 1, 40.0,
                    '{"drop_lowest":1,"drop_highest":null,"never_drop":["31"]}',
                    '{"assignments_by_period":{"none":{"observed_at":"2040-09-09T16:00:00Z","subtotal":{"score":13.25,"possible":50.0},"assignments":[
                        {"id":"32","name":"Synthetic Exercise Two","due_at":"2040-09-28T03:59:00Z","points_possible":25.0,"omit_from_final_grade":false,"score":null,"grade":null,"excused":null,"late":null,"missing":true,"posted_at":null,"workflow_state":"unsubmitted","submitted_at":null,"attempt":null},
                        {"id":"31","name":"Synthetic Exercise One","due_at":"2040-09-14T03:59:00Z","points_possible":25.0,"omit_from_final_grade":false,"score":13.25,"grade":"13.25","excused":false,"late":false,"missing":false,"posted_at":"2040-09-16T12:00:00Z","workflow_state":"graded","submitted_at":"2040-09-13T18:00:00Z","attempt":1}
                    ]}}}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                r#"INSERT INTO assignment_groups
                    (id, course_id, name, position, group_weight, rules_json, data_json)
                 VALUES (
                    21, 61001, 'Synthetic Exams', 2, 40.25, NULL,
                    '{"assignments_by_period":{"none":{"observed_at":"2040-09-09T16:00:00Z","assignments":[]}}}'
                 )"#,
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES ('assignment_groups', 'course:61001:period:none', 'assignment_group', '20', 0),
                        ('assignment_groups', 'course:61001:period:none', 'assignment_group', '21', 1)",
                [],
            )?;
            conns.cache.execute(
                "INSERT INTO fetch_log (dataset, scope, fetched_at, complete, count, stale, epoch_seen)
                 VALUES
                   ('assignment_groups', 'course:61001:period:none', '2040-09-09T16:00:00Z', 1, 2, 0, 0),
                   ('assignment_groups', 'course:61001:period:5', '2040-09-09T16:00:00Z', 1, 0, 0, 0),
                   ('assignment_groups', 'course:61002:period:none', '2040-09-09T16:00:00Z', 1, 0, 0, 0),
                   ('assignment_groups', 'course:61002:period:5', '2040-09-09T16:00:00Z', 1, 0, 0, 0),
                   ('assignment_groups', 'course:61003:period:none', '2040-09-09T16:00:00Z', 1, 0, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn prepare() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example.test", 62001, "2040-01-01T00:00:00Z");
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    seed_courses(&open);
    seed_totals(&open);
    seed_enrollments(&open);
    seed_periods(&open);
    seed_groups(&open);
    drop(open);
    (dir, key)
}

fn run(dir: &tempfile::TempDir, key: &str, args: &[&str]) -> (i32, String) {
    let output = bin()
        .env("CANVAS_DATA_ROOT", dir.path())
        .env("CANVAS_IDENTITY_KEY", key)
        .env("CANVAS_NOW", "2040-09-09T17:05:12Z")
        .env("COLUMNS", "100")
        .env("TZ", "America/New_York")
        .env_remove("CANVAS_TOKEN")
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn run_json(dir: &tempfile::TempDir, key: &str, args: &[&str]) -> (i32, Value) {
    let (code, stdout) = run(dir, key, args);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"));
    (code, value)
}

// --- period modes ---

#[test]
fn a_course_without_periods_defaults_to_all() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-201",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["result"]["period_mode"], "all");
    assert_eq!(v["result"]["courses"][0]["grades"]["period"]["mode"], "all");
}

#[test]
fn a_course_with_periods_defaults_to_current() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["result"]["period_mode"], "current");
    let grades = &v["result"]["courses"][0]["grades"];
    assert_eq!(grades["period"]["mode"], "current");
    assert_eq!(grades["period"]["id"], "5");
    assert_eq!(grades["period"]["title"], "Synthetic Period One");
    // The current-period total, not the whole-course one.
    assert_eq!(grades["current_score"], 73.25);
}

#[test]
fn explicit_all_reads_the_whole_course_total() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["result"]["period_mode"], "all");
    let grades = &v["result"]["courses"][0]["grades"];
    assert_eq!(grades["current_score"], 68.25);
    // A total from one mode is never labelled with another.
    assert!(grades["period"]["id"].is_null());
    assert!(grades["period"]["title"].is_null());
}

#[test]
fn an_explicit_valid_period_reads_that_periods_enrollments() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "5",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["result"]["period_mode"], "id");
    let grades = &v["result"]["courses"][0]["grades"];
    // Period 5's enrollment values, not the current-period or course totals.
    assert_eq!(grades["current_score"], 57.25);
    assert_eq!(grades["current_grade"], "C+");
    assert_eq!(grades["period"]["mode"], "id");
    assert_eq!(grades["period"]["id"], "5");
    assert_eq!(grades["period"]["title"], "Synthetic Period One");
}

#[test]
fn a_non_numeric_period_is_a_usage_error() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "--period",
            "midterm",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 2, "{v}");
    assert_eq!(v["schema"], "canvas-cli/error@1");
    assert_eq!(v["result"]["code"], "usage");
}

#[test]
fn a_period_absent_for_one_course_reports_unavailable() {
    let (dir, key) = prepare();
    // Period 5 belongs to SYN-101; SYN-201 has no row for it.
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-201",
            "--period",
            "5",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let course = &v["result"]["courses"][0];
    assert_eq!(
        course["unavailable_reason"],
        "course has no grading period 5"
    );
    // Unavailable stays null; it is never filled from another mode.
    assert!(course["grades"]["current_score"].is_null());
    assert!(course["grades"]["final_score"].is_null());
}

#[test]
fn a_period_the_selected_course_lacks_is_unavailable_not_an_error() {
    let (dir, key) = prepare();
    // 999 is not in SYN-101's period list, so the course reports unavailable.
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "999",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let course = &v["result"]["courses"][0];
    assert_eq!(
        course["unavailable_reason"],
        "course has no grading period 999"
    );
    assert!(course["grades"]["current_score"].is_null());
    // Nothing is fetched for a period the course does not have.
    let datasets: Vec<&str> = v["freshness"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["dataset"].as_str().unwrap())
        .collect();
    assert!(!datasets.contains(&"enrollment_grades"), "{datasets:?}");
}

#[test]
fn an_id_no_course_reports_grades_for_is_a_resolution_error() {
    let (dir, key) = prepare();
    // Canvas answers a bogus period with an empty list: complete, zero rows.
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "--period",
            "999",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 6, "{v}");
    assert_eq!(v["result"]["code"], "resolution");
}

// --- totals and de-duplication ---

#[test]
fn null_totals_stay_null_and_print_unavailable() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-201",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let grades = &v["result"]["courses"][0]["grades"];
    for field in [
        "current_score",
        "current_grade",
        "final_score",
        "final_grade",
    ] {
        assert!(
            grades[field].is_null(),
            "{field} should stay null: {grades}"
        );
    }

    let (code, human) = run(
        &dir,
        &key,
        &["grades", "SYN-201", "--offline", "--color", "never"],
    );
    assert_eq!(code, 0);
    assert!(human.contains("unavailable"), "human={human}");
}

#[test]
fn duplicate_enrollments_collapse_to_one_row_per_course() {
    let (dir, key) = prepare();
    // Courses 61001 and 61003 each have two `none` enrollments; only the second
    // carries values. Course 61003 has no `course_totals` row, so its total is
    // read straight from the de-duplicated enrollments.
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let courses = v["result"]["courses"].as_array().unwrap();
    assert_eq!(courses.len(), 3, "one row per course: {v}");
    let ids: Vec<&str> = courses
        .iter()
        .map(|c| c["course"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["61001", "61002", "61003"], "sorted by code");
    // The enrollment that carried values wins over the empty one.
    let phys = &courses[2]["grades"];
    assert_eq!(phys["current_score"], 44.25, "{v}");
    assert_eq!(phys["current_grade"], "D");
    assert_eq!(phys["final_score"], 40.25);
}

/// SPEC §12.4: totals from one period mode are never labelled with another.
/// The unqualified enrollments are whole-course values, so a course the course
/// list did not cover reports `unavailable` under `--period current`.
#[test]
fn an_uncovered_course_never_borrows_whole_course_totals_for_current() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-301",
            "--period",
            "current",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let course = &v["result"]["courses"][0];
    assert_eq!(course["grades"]["period"]["mode"], "current");
    for field in [
        "current_score",
        "current_grade",
        "final_score",
        "final_grade",
    ] {
        assert!(
            course["grades"][field].is_null(),
            "{field} must not carry the whole-course value: {course}"
        );
    }
    assert_eq!(
        course["unavailable_reason"],
        "no current grading period total"
    );

    // The same course under `all` does read those whole-course values.
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-301",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["result"]["courses"][0]["grades"]["current_score"], 44.25);
}

// --- course view ---

#[test]
fn the_course_view_carries_groups_periods_and_sorted_assignments() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    let view = &v["result"]["course"];
    let groups = view["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    // Groups sort by position.
    assert_eq!(groups[0]["name"], "Synthetic Exercises");
    assert_eq!(groups[0]["weight"], 40.0);
    assert_eq!(groups[0]["rules"]["drop_lowest"], 1);
    assert_eq!(groups[0]["rules"]["never_drop"][0], "31");
    // A subtotal appears only when the API supplied one.
    assert_eq!(groups[0]["subtotal"]["score"], 13.25);
    assert!(groups[1]["subtotal"].is_null());
    // Assignments sort by due_at then id, regardless of stored order.
    let names: Vec<&str> = groups[0]["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Synthetic Exercise One", "Synthetic Exercise Two"]);
    let status = &groups[0]["assignments"][1]["status"];
    assert_eq!(status["missing"], true);
    assert_eq!(status["pending"], false);
    // Periods carry which one contains CANVAS_NOW.
    let periods = view["periods"].as_array().unwrap();
    assert_eq!(periods.len(), 2);
    assert_eq!(periods[0]["is_current"], true);
    assert_eq!(periods[1]["is_current"], false);
}

#[test]
fn the_overview_has_no_course_view() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert!(v["result"]["course"].is_null());
    assert_eq!(v["result"]["courses"].as_array().unwrap().len(), 3);
}

// --- offline and freshness ---

#[test]
fn offline_with_coverage_serves_the_cache_without_requests() {
    let (dir, key) = prepare();
    let (code, v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["requests"]["api"], 0);
    let datasets: Vec<&str> = v["freshness"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["dataset"].as_str().unwrap())
        .collect();
    assert!(datasets.contains(&"courses"), "{datasets:?}");
    assert!(datasets.contains(&"enrollment_grades"), "{datasets:?}");
    // The default modes read `course_totals`, so it is declared too.
    assert!(datasets.contains(&"course_totals"), "{datasets:?}");
    // Offline coverage is served stale.
    for row in v["freshness"].as_array().unwrap() {
        assert_eq!(row["source"], "cache", "{row}");
    }
}

#[test]
fn offline_without_coverage_is_exit_7() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = IdentityDocument::new("https://canvas.example.test", 62001, "2040-01-01T00:00:00Z");
    let key = doc.key.to_string();
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    drop(OpenIdentity::open(&paths, &doc).unwrap());

    let (code, v) = run_json(
        &dir,
        &key,
        &["grades", "--offline", "--json", "--color", "never"],
    );
    assert_eq!(code, 7, "{v}");
    assert_eq!(v["result"]["code"], "offline");
}

#[test]
fn grades_without_an_identity_is_exit_3() {
    let empty = tempfile::TempDir::new().unwrap();
    let output = bin()
        .env("CANVAS_DATA_ROOT", empty.path())
        .env_remove("CANVAS_IDENTITY_KEY")
        .env_remove("CANVAS_TOKEN")
        .args(["grades", "--offline", "--json", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("canvas-cli/error@1"), "stdout={stdout}");
}

// --- snapshots ---

#[test]
fn grades_json_snapshot_from_fixture_cache() {
    let (dir, key) = prepare();
    let (code, mut v) = run_json(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "all",
            "--offline",
            "--json",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0, "{v}");
    v["identity"]["key"] = Value::String("KEY".into());
    insta::assert_json_snapshot!("grades_json_course_view", v);
}

#[test]
fn grades_human_snapshot_from_fixture_cache() {
    let (dir, key) = prepare();
    let (code, stdout) = run(
        &dir,
        &key,
        &[
            "grades",
            "SYN-101",
            "--period",
            "all",
            "--offline",
            "--color",
            "never",
        ],
    );
    assert_eq!(code, 0);
    insta::assert_snapshot!("grades_human_course_view", stdout);
}

#[test]
fn grades_overview_human_snapshot() {
    let (dir, key) = prepare();
    let (code, stdout) = run(
        &dir,
        &key,
        &["grades", "--period", "all", "--offline", "--color", "never"],
    );
    assert_eq!(code, 0);
    insta::assert_snapshot!("grades_human_overview", stdout);
}

//! The synthetic 5-course fixture set `bench` falls back to.
//!
//! SPEC §13 fixes the benchmark shape at five courses. Recording a real
//! account is not approved yet (SPEC §19 item 5), so `bench` generates a set
//! of the right shape instead. It is written once and then tracked, which
//! keeps a benchmark run reproducible and keeps `git status` clean.
//!
//! Every timestamp is generated relative to `BASE`, and `bench` shifts each
//! one by whole days when it serves the set. A tracked fixture with absolute
//! dates would otherwise drift out of the planner window and quietly shrink
//! the workload the benchmark measures.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use crate::fixture::{Manifest, REDACTION_VERSION, Recorded, write_manifest, write_recorded};

/// The instant every generated timestamp is relative to.
pub const BASE: &str = "2026-01-05T12:00:00Z";

/// Course IDs in the set.
pub const COURSES: [i64; 5] = [101, 102, 103, 104, 105];

/// The course whose files the benchmark downloads.
pub const DOWNLOAD_COURSE: i64 = 101;

/// Files of [`DOWNLOAD_COURSE`], and the size each one serves.
pub const DOWNLOAD_FILES: [(i64, u64); 4] = [
    (5011, 3_145_728),
    (5012, 2_097_152),
    (5013, 1_048_576),
    (5014, 4_194_304),
];

const CODES: [&str; 5] = ["BIO-110", "CHEM-101", "HIST-240", "MATH-201", "PHYS-150"];
const ASSIGNMENTS_PER_COURSE: i64 = 8;

fn base() -> jiff::Timestamp {
    BASE.parse().expect("valid base timestamp")
}

/// `BASE` shifted by whole days and hours.
fn at(days: i64, hours: i64) -> String {
    let shift = jiff::SignedDuration::from_hours(days * 24 + hours);
    (base() + shift).to_string()
}

fn get(path: String, body: Value) -> Recorded {
    Recorded {
        method: "GET".to_owned(),
        path,
        query: Vec::new(),
        status: 200,
        headers: BTreeMap::new(),
        body,
        page: None,
    }
}

fn with_query(mut recorded: Recorded, query: &[(&str, &str)]) -> Recorded {
    recorded.query = query
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    recorded
}

fn course(index: usize) -> Value {
    let id = COURSES[index];
    let n = i64::try_from(index).unwrap_or(0);
    let offset = f64::from(u32::try_from(index).unwrap_or(0));
    json!({
        "id": id,
        "course_code": CODES[index],
        "name": format!("Course {}", CODES[index]),
        "workflow_state": "available",
        "is_favorite": index < 3,
        "term": {"id": 900 + n, "name": "Spring 2026"},
        "enrollments": [{
            "type": "student",
            "enrollment_state": "active",
            "computed_current_score": 88.0 + offset,
            "computed_current_grade": "B+",
            "computed_final_score": 85.0 + offset,
            "computed_final_grade": "B",
            "current_period_computed_current_score": 90.0 + offset,
            "current_period_computed_current_grade": "A-",
            "current_grading_period_id": "700",
            "current_grading_period_title": "Term 1"
        }]
    })
}

fn assignment(course_id: i64, n: i64) -> Value {
    let id = course_id * 100 + n;
    // A spread of due dates around the planner window, some graded, some not.
    let due = at(n * 3 - 10, 4);
    json!({
        "id": id,
        "course_id": course_id,
        "name": format!("Assignment {n}"),
        "due_at": due,
        "points_possible": 20.0 + f64::from(u32::try_from(n).unwrap_or(0)),
        "submission_types": ["online_upload"],
        "published": true,
        "locked_for_user": false,
        "html_url": format!("/courses/{course_id}/assignments/{id}"),
        "submission": {
            "workflow_state": if n % 3 == 0 { "graded" } else { "unsubmitted" },
            "attempt": i64::from(n % 3 == 0),
            "score": if n % 3 == 0 { json!(18.0) } else { Value::Null },
            "submitted_at": if n % 3 == 0 { json!(at(n * 3 - 11, 2)) } else { Value::Null },
            "late": false,
            "missing": n % 7 == 0,
            "excused": false
        }
    })
}

fn planner_item(course_id: i64, n: i64) -> Value {
    let id = course_id * 100 + n;
    json!({
        "plannable_type": "assignment",
        "plannable_id": id,
        "course_id": course_id,
        "plannable_date": at(n * 2 - 4, 6),
        "plannable": {
            "id": id,
            "title": format!("Assignment {n}"),
            "due_at": at(n * 2 - 4, 6),
            "points_possible": 20.0
        },
        "submissions": {
            "submitted": n % 3 == 0,
            "graded": n % 3 == 0,
            "missing": false,
            "excused": false
        },
        "html_url": format!("/courses/{course_id}/assignments/{id}")
    })
}

/// Every response in the set.
#[allow(clippy::too_many_lines)]
pub fn responses() -> Vec<Recorded> {
    let mut out = Vec::new();

    out.push(get(
        "/api/v1/users/self".to_owned(),
        json!({
            "id": 1001, "name": "Name 1", "short_name": "Name 1",
            "sortable_name": "Name 1", "login_id": "user1"
        }),
    ));

    let courses: Vec<Value> = (0..COURSES.len()).map(course).collect();
    out.push(with_query(
        get("/api/v1/courses".to_owned(), json!(courses)),
        &[("enrollment_state", "active")],
    ));

    let enrollments: Vec<Value> = COURSES
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let n = i64::try_from(index).unwrap_or(0);
            let offset = f64::from(u32::try_from(index).unwrap_or(0));
            json!({
                "id": 800 + n,
                "course_id": id,
                "type": "StudentEnrollment",
                "enrollment_state": "active",
                "grades": {
                    "current_score": 88.0 + offset,
                    "current_grade": "B+",
                    "final_score": 85.0 + offset,
                    "final_grade": "B"
                }
            })
        })
        .collect();
    out.push(get(
        "/api/v1/users/self/enrollments".to_owned(),
        json!(enrollments),
    ));

    // The planner window and the missing list drive `todo`, the measured command.
    let mut planner = Vec::new();
    for course_id in COURSES {
        for n in 1..=6 {
            planner.push(planner_item(course_id, n));
        }
    }
    out.push(get("/api/v1/planner/items".to_owned(), json!(planner)));

    let missing: Vec<Value> = COURSES
        .iter()
        .map(|course_id| {
            json!({
                "id": course_id * 100 + 7,
                "course_id": course_id,
                "name": "Assignment 7",
                "due_at": at(-3, 4),
                "points_possible": 40.0,
                "submission_types": ["online_upload"],
                "html_url": format!("/courses/{course_id}/assignments/{}", course_id * 100 + 7)
            })
        })
        .collect();
    out.push(get(
        "/api/v1/users/self/missing_submissions".to_owned(),
        json!(missing),
    ));

    for course_id in COURSES {
        out.push(get(
            format!("/api/v1/courses/{course_id}/grading_periods"),
            json!({"grading_periods": [
                {"id": "700", "title": "Term 1",
                 "start_date": at(-30, 0), "end_date": at(30, 0)},
                {"id": "701", "title": "Term 2",
                 "start_date": at(31, 0), "end_date": at(90, 0)}
            ]}),
        ));

        let assignments: Vec<Value> = (1..=ASSIGNMENTS_PER_COURSE)
            .map(|n| assignment(course_id, n))
            .collect();
        out.push(get(
            format!("/api/v1/courses/{course_id}/assignments"),
            json!(assignments),
        ));

        out.push(get(
            format!("/api/v1/courses/{course_id}/folders"),
            json!([
                {"id": course_id * 10, "name": "course files",
                 "full_name": "course files", "parent_folder_id": Value::Null,
                 "files_count": 4, "folders_count": 1, "hidden": false,
                 "locked_for_user": false},
                {"id": course_id * 10 + 1, "name": "Readings",
                 "full_name": "course files/Readings",
                 "parent_folder_id": course_id * 10, "files_count": 2,
                 "folders_count": 0, "hidden": false, "locked_for_user": false}
            ]),
        ));

        let files: Vec<Value> = file_rows(course_id);
        out.push(get(
            format!("/api/v1/courses/{course_id}/files"),
            json!(files),
        ));

        out.push(get(
            format!("/api/v1/courses/{course_id}/modules"),
            json!([
                {"id": course_id * 20, "name": "Unit 1", "position": 1,
                 "state": "unlocked", "items_count": 2, "published": true,
                 "items": [
                    {"id": course_id * 20 + 1, "module_id": course_id * 20,
                     "title": "Reading", "type": "File",
                     "content_id": course_id * 100 + 11, "position": 1,
                     "content_details": {"locked_for_user": false}},
                    {"id": course_id * 20 + 2, "module_id": course_id * 20,
                     "title": "Assignment 1", "type": "Assignment",
                     "content_id": course_id * 100 + 1, "position": 2,
                     "content_details": {"locked_for_user": false}}
                 ]},
                {"id": course_id * 20 + 10, "name": "Unit 2", "position": 2,
                 "state": "unlocked", "items_count": 0, "published": true,
                 "items": []}
            ]),
        ));
    }

    // Per-file metadata for the course the benchmark downloads.
    for (id, size) in DOWNLOAD_FILES {
        out.push(get(format!("/api/v1/files/{id}"), file_row(id, size)));
    }
    out
}

fn file_rows(course_id: i64) -> Vec<Value> {
    if course_id == DOWNLOAD_COURSE {
        return DOWNLOAD_FILES
            .iter()
            .map(|(id, size)| file_row(*id, *size))
            .collect();
    }
    (1..=4)
        .map(|n| {
            file_row(
                course_id * 100 + 10 + n,
                4096 * u64::try_from(n).unwrap_or(1),
            )
        })
        .collect()
}

fn file_row(id: i64, size: u64) -> Value {
    json!({
        "id": id,
        "display_name": format!("file-{id}.pdf"),
        "filename": format!("file-{id}.pdf"),
        "content-type": "application/pdf",
        "size": size,
        "folder_id": DOWNLOAD_COURSE * 10,
        "hidden": false,
        "locked_for_user": false,
        "updated_at": at(-20, 0),
        // Relative: the model resolves it against the client origin (SPEC §11),
        // so the same fixture works against any mock address.
        "url": format!("{BLOB_PREFIX}{id}")
    })
}

/// Where a file body is served from. `bench` mounts this route with a
/// generated body; a fixture holds JSON only, never a binary blob.
pub const BLOB_PREFIX: &str = "/bench/blob/";

/// Write the set, with its manifest.
pub fn generate(dir: &Path) -> Result<usize> {
    let responses = responses();
    let mut endpoints: Vec<String> = responses.iter().map(Recorded::endpoint).collect();
    endpoints.sort();
    endpoints.dedup();
    for recorded in &responses {
        write_recorded(dir, recorded)?;
    }
    write_manifest(
        dir,
        &Manifest {
            set: dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("bench-5")
                .to_owned(),
            recorded_at: BASE.to_owned(),
            endpoints,
            redaction_version: REDACTION_VERSION,
            synthetic: true,
            pseudonyms: crate::fixture::Pseudonyms::default(),
        },
    )?;
    Ok(responses.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_has_five_courses_and_every_endpoint_the_benchmark_drives() {
        let responses = responses();
        let paths: Vec<&str> = responses.iter().map(|r| r.path.as_str()).collect();
        for course_id in COURSES {
            for suffix in [
                "grading_periods",
                "assignments",
                "folders",
                "files",
                "modules",
            ] {
                let want = format!("/api/v1/courses/{course_id}/{suffix}");
                assert!(paths.contains(&want.as_str()), "{want} missing");
            }
        }
        for want in [
            "/api/v1/users/self",
            "/api/v1/courses",
            "/api/v1/users/self/enrollments",
            "/api/v1/planner/items",
            "/api/v1/users/self/missing_submissions",
        ] {
            assert!(paths.contains(&want), "{want} missing");
        }
        for (id, _) in DOWNLOAD_FILES {
            let want = format!("/api/v1/files/{id}");
            assert!(paths.contains(&want.as_str()), "{want} missing");
        }
        // Five courses, and a planner list worth measuring.
        let courses = responses
            .iter()
            .find(|r| r.path == "/api/v1/courses")
            .unwrap();
        assert_eq!(courses.body.as_array().unwrap().len(), 5);
        let planner = responses
            .iter()
            .find(|r| r.path == "/api/v1/planner/items")
            .unwrap();
        assert_eq!(planner.body.as_array().unwrap().len(), 30);
    }

    #[test]
    fn file_urls_stay_relative_so_any_mock_address_works() {
        for recorded in responses() {
            let text = recorded.body.to_string();
            assert!(
                !text.contains("http://"),
                "{} has an absolute URL",
                recorded.path
            );
            assert!(
                !text.contains("https://"),
                "{} has an absolute URL",
                recorded.path
            );
        }
    }

    #[test]
    fn generating_twice_writes_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        generate(&a).unwrap();
        generate(&b).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&a)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(names.len() > 30);
        for name in names {
            // The manifest names its own directory, which differs by design.
            if name == crate::fixture::MANIFEST {
                continue;
            }
            assert_eq!(
                std::fs::read_to_string(a.join(&name)).unwrap(),
                std::fs::read_to_string(b.join(&name)).unwrap(),
                "{name} differs"
            );
        }
    }
}

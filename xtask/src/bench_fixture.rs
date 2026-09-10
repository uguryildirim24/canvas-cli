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

/// The user the set belongs to. `sync --full` asks for that user's calendar
/// context by id, so the two have to agree.
pub const USER_ID: i64 = 1001;

/// Course IDs in the set.
pub const COURSES: [i64; 5] = [101, 102, 103, 104, 105];

/// The course whose files the benchmark downloads.
pub const DOWNLOAD_COURSE: i64 = 101;

/// Files of [`DOWNLOAD_COURSE`], and the size each one serves.
/// Large enough that one download keeps the server streaming across a run.
pub const DOWNLOAD_FILES: [(i64, u64); 4] = [
    (5011, 4_194_304),
    (5012, 3_145_728),
    (5013, 2_097_152),
    (5014, 5_242_880),
];

const CODES: [&str; 5] = ["BIO-110", "CHEM-101", "HIST-240", "MATH-201", "PHYS-150"];

/// The host every link in the set points at.
///
/// Only the file `url` stays relative, because that one is fetched and must
/// resolve to the mock server (SPEC §11: a relative URL resolves against the
/// client origin). Every other link is decoration, and one of them must be
/// absolute: `canvas-core` decodes planner items from a raw value outside the
/// origin scope, so a relative `html_url` there does not resolve.
const LINK_HOST: &str = "https://canvas.example.edu";
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
        "html_url": format!("{LINK_HOST}/courses/{course_id}/assignments/{id}"),
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
        "html_url": format!("{LINK_HOST}/courses/{course_id}/assignments/{id}")
    })
}

fn announcement(course_id: i64, n: i64) -> Value {
    let id = course_id * 100 + 50 + n;
    json!({
        "id": id,
        "title": format!("Announcement {n}"),
        "message": format!("<p>Notice {n} for course {course_id}.</p>"),
        "posted_at": at(n, 9),
        "published": true,
        "locked": false,
        "is_announcement": true,
        "context_code": format!("course_{course_id}"),
        "html_url": format!("{LINK_HOST}/courses/{course_id}/discussion_topics/{id}")
    })
}

/// A timed event. The set holds no all-day event on purpose: `bench` shifts a
/// body by whole days by rewriting RFC 3339 timestamps, and `all_day_date` is
/// a bare date it leaves alone, so an all-day row would drift out of the
/// window the run asks for.
fn calendar_event(context_code: &str, id: i64, n: i64) -> Value {
    json!({
        "id": id,
        "title": format!("Event {n}"),
        "start_at": at(n, 15),
        "end_at": at(n, 16),
        "workflow_state": "active",
        "hidden": false,
        "all_day": false,
        "all_day_date": Value::Null,
        "context_code": context_code,
        "html_url": format!("{LINK_HOST}/calendar?event_id={id}")
    })
}

/// Every response in the set.
#[allow(clippy::too_many_lines)]
pub fn responses() -> Vec<Recorded> {
    let mut out = Vec::new();

    out.push(get(
        "/api/v1/users/self".to_owned(),
        json!({
            "id": USER_ID, "name": "Name 1", "short_name": "Name 1",
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
                "html_url": format!("{LINK_HOST}/courses/{course_id}/assignments/{}", course_id * 100 + 7)
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

    // `sync` refreshes announcements for every course in one request, and
    // `sync --full` adds the calendar events of those courses and of the user.
    // Both routes answer whatever `context_codes[]` set they are asked for:
    // the fixture is stored without query pairs, so `mount_set` matches them
    // on method and path alone.
    let announcements: Vec<Value> = COURSES
        .iter()
        .flat_map(|course_id| (1..=2).map(move |n| announcement(*course_id, n)))
        .collect();
    out.push(get(
        "/api/v1/announcements".to_owned(),
        json!(announcements),
    ));

    let mut events = vec![calendar_event(&format!("user_{USER_ID}"), 6000, 1)];
    for (index, course_id) in COURSES.iter().enumerate() {
        let n = i64::try_from(index).unwrap_or(0);
        events.push(calendar_event(
            &format!("course_{course_id}"),
            course_id * 10 + 7,
            n + 2,
        ));
    }
    out.push(get("/api/v1/calendar_events".to_owned(), json!(events)));

    // Per-file metadata for the course the benchmark downloads.
    for (id, size) in DOWNLOAD_FILES {
        out.push(get(format!("/api/v1/files/{id}"), file_row(id, size)));
    }

    push_reads(&mut out);
    push_writes(&mut out);
    out
}

/// The M8-a read surface: pages, discussions, and the inbox.
///
/// One page carries an iframe, a same-origin file link, and a cross-origin
/// link, so `page` has something to report as embedded, as a file, and as an
/// external link. The topics cover a plain thread, a thread gated behind an
/// initial post, and a graded group discussion.
fn push_reads(out: &mut Vec<Recorded>) {
    let course_id = DOWNLOAD_COURSE;
    out.push(with_query(
        get(
            format!("/api/v1/courses/{course_id}/pages"),
            json!(vec![
                page_row(course_id, 1, true),
                page_row(course_id, 2, false)
            ]),
        ),
        &[("sort", "title")],
    ));
    out.push(get(
        format!("/api/v1/courses/{course_id}/pages/page-{course_id}-1"),
        page_body(course_id, 1),
    ));

    let topics = vec![
        topic_row(course_id, 1),
        gated_topic_row(course_id, 2),
        graded_group_topic_row(course_id, 3),
    ];
    out.push(with_query(
        get(
            format!("/api/v1/courses/{course_id}/discussion_topics"),
            json!(topics.clone()),
        ),
        &[("only_announcements", "false")],
    ));
    for topic in &topics {
        let id = topic["id"].as_i64().unwrap_or_default();
        out.push(get(
            format!("/api/v1/courses/{course_id}/discussion_topics/{id}"),
            topic.clone(),
        ));
    }
    let plain = topics[0]["id"].as_i64().unwrap_or_default();
    out.push(get(
        format!("/api/v1/courses/{course_id}/discussion_topics/{plain}/entries"),
        json!((1..=3).map(|n| entry_row(plain, n)).collect::<Vec<_>>()),
    ));
    // The gate refuses the entries route; the topic itself still reads.
    let gated = topics[1]["id"].as_i64().unwrap_or_default();
    out.push(Recorded {
        status: 403,
        ..get(
            format!("/api/v1/courses/{course_id}/discussion_topics/{gated}/entries"),
            json!({"status": "unauthorized", "errors": [{"message": "initial post required"}]}),
        )
    });

    let conversations: Vec<Value> = (1..=3).map(conversation_row).collect();
    out.push(with_query(
        get("/api/v1/conversations".to_owned(), json!(conversations)),
        &[("scope", "inbox"), ("auto_mark_as_read", "false")],
    ));
    for n in 1..=3 {
        out.push(with_query(
            get(
                format!("/api/v1/conversations/{}", 7000 + n),
                conversation_detail(n),
            ),
            &[("auto_mark_as_read", "false")],
        ));
    }
    out.push(get(
        "/api/v1/conversations/unread_count".to_owned(),
        json!({"unread_count": "2"}),
    ));
}

/// The M8-b write surface: what a reply and a message need to be prepared.
///
/// Nothing here is a `POST` the benchmark issues. A write needs a recorded
/// human approval, which no harness can give, so the set carries the reads a
/// prepare makes — the topic, the entries, the conversation, the recipient —
/// plus the two upload routes an inbox attachment travels, so a recorded set
/// covers the whole path and not only its read half.
fn push_writes(out: &mut Vec<Recorded>) {
    let course_id = DOWNLOAD_COURSE;
    let locked = locked_topic_row(course_id, 4);
    let locked_id = locked["id"].as_i64().unwrap_or_default();
    out.push(get(
        format!("/api/v1/courses/{course_id}/discussion_topics/{locked_id}"),
        locked,
    ));
    out.push(get(
        format!("/api/v1/courses/{course_id}/discussion_topics/{locked_id}/entries"),
        json!([]),
    ));

    // One messageable recipient: `prepare` resolves every id this way, and an
    // id that resolves to nothing is refused as `unresolved`.
    out.push(with_query(
        get(
            "/api/v1/search/recipients".to_owned(),
            json!([{"id": 3001, "name": "Classmate 1", "common_courses": {}}]),
        ),
        &[("user_id", "3001")],
    ));

    // The §11 upload transport for a conversation attachment: the session
    // request, and the storage `POST` its response names.
    out.push(Recorded {
        method: "POST".to_owned(),
        status: 200,
        ..get(
            "/api/v1/users/self/files".to_owned(),
            json!({
                "upload_url": "/upload/user-files",
                "upload_params": {"key": "user-files/1", "filename": "note.txt"},
                "file_param": "file"
            }),
        )
    });
    out.push(Recorded {
        method: "POST".to_owned(),
        status: 201,
        ..get(
            "/upload/user-files".to_owned(),
            json!({
                "id": 9100,
                "display_name": "note.txt",
                "filename": "note.txt",
                "size": 12,
                "content-type": "text/plain"
            }),
        )
    });
}

fn page_row(course_id: i64, n: i64, front: bool) -> Value {
    json!({
        "page_id": course_id * 1000 + n,
        "url": format!("page-{course_id}-{n}"),
        "title": format!("Page {n}"),
        "created_at": at(-40, 0),
        "updated_at": at(-12, 3),
        "published": true,
        "front_page": front,
        "locked_for_user": false,
        "editing_roles": "teachers",
        "html_url": format!("{LINK_HOST}/courses/{course_id}/pages/page-{course_id}-{n}")
    })
}

fn page_body(course_id: i64, n: i64) -> Value {
    let mut row = page_row(course_id, n, true);
    let file_id = DOWNLOAD_FILES[0].0;
    row["body"] = json!(format!(
        concat!(
            "<p>Read the <a href=\"/courses/{course}/files/{file}\">handbook</a>.</p>",
            "<p>See <a href=\"https://example.org/notes\">the notes</a>.</p>",
            "<iframe src=\"https://player.example.net/embed/1\"></iframe>"
        ),
        course = course_id,
        file = file_id
    ));
    row
}

fn topic_row(course_id: i64, n: i64) -> Value {
    json!({
        "id": course_id * 100 + 90 + n,
        "title": format!("Discussion {n}"),
        "message": format!("<p>What did you make of week {n}?</p>"),
        "posted_at": at(-14, 2),
        "last_reply_at": at(-2, 1),
        "discussion_type": "threaded",
        "user_name": "Prof. Ada",
        "read_state": "unread",
        "unread_count": 2,
        "discussion_subentry_count": 3,
        "published": true,
        "locked": false,
        "locked_for_user": false,
        "pinned": false,
        "require_initial_post": false,
        "user_can_see_posts": true,
        "is_announcement": false,
        "subscribed": true,
        "context_code": format!("course_{course_id}"),
        "html_url": format!("{LINK_HOST}/courses/{course_id}/discussion_topics/{}", course_id * 100 + 90 + n)
    })
}

fn gated_topic_row(course_id: i64, n: i64) -> Value {
    let mut row = topic_row(course_id, n);
    row["title"] = json!("Introduce yourself");
    row["require_initial_post"] = json!(true);
    row["user_can_see_posts"] = json!(false);
    row["unread_count"] = json!(0);
    row
}

fn locked_topic_row(course_id: i64, n: i64) -> Value {
    let mut row = topic_row(course_id, n);
    row["title"] = json!("Week 1 wrap-up (closed)");
    row["locked"] = json!(true);
    row["locked_for_user"] = json!(true);
    row
}

fn graded_group_topic_row(course_id: i64, n: i64) -> Value {
    let mut row = topic_row(course_id, n);
    row["title"] = json!("Group lab report");
    row["assignment_id"] = json!(course_id * 100 + 1);
    row["points_possible"] = json!(15.0);
    row["group_category_id"] = json!(770 + n);
    row["group_topic_children"] = json!([
        {"id": course_id * 100 + 95, "group_id": 8801},
        {"id": course_id * 100 + 96, "group_id": 8802},
    ]);
    row
}

fn entry_row(topic_id: i64, n: i64) -> Value {
    json!({
        "id": topic_id * 10 + n,
        "parent_id": null,
        "user_id": 2000 + n,
        "user_name": format!("Student {n}"),
        "message": format!("<p>Reply {n}.</p>"),
        "created_at": at(-3, n),
        "updated_at": at(-3, n),
        "read_state": "unread",
        "has_more_replies": false,
        "recent_replies": []
    })
}

fn conversation_row(n: i64) -> Value {
    json!({
        "id": 7000 + n,
        "subject": format!("Conversation {n}"),
        "workflow_state": if n == 1 { "unread" } else { "read" },
        "last_message": "See you then.",
        "last_message_at": at(-n, 4),
        "message_count": 2,
        "subscribed": true,
        "private": true,
        "starred": false,
        "context_name": CODES[0],
        "participants": [
            {"id": USER_ID, "name": "You"},
            {"id": 3000 + n, "name": format!("Classmate {n}")},
        ]
    })
}

fn conversation_detail(n: i64) -> Value {
    let mut row = conversation_row(n);
    row["messages"] = json!([
        {
            "id": 8000 + n * 2,
            "author_id": 3000 + n,
            "created_at": at(-n, 3),
            "body": "Can we meet before the lab?",
            "generated": false,
            "attachments": []
        },
        {
            "id": 8001 + n * 2,
            "author_id": USER_ID,
            "created_at": at(-n, 4),
            "body": "See you then.",
            "generated": false,
            "attachments": []
        },
    ]);
    row
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
            // `sync` reads announcements, and `sync --full` reads the calendar.
            "/api/v1/announcements",
            "/api/v1/calendar_events",
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

    /// `sync --full` asks for the calendar of the user and of every course in
    /// one request. A context the set answers with nothing is a denial, and
    /// `sync` reports that as `partial` (exit 12), which fails the run.
    #[test]
    fn the_calendar_covers_the_user_and_every_course() {
        let responses = responses();
        let events = responses
            .iter()
            .find(|r| r.path == "/api/v1/calendar_events")
            .expect("calendar events");
        let contexts: Vec<&str> = events
            .body
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["context_code"].as_str().unwrap())
            .collect();
        assert!(contexts.contains(&format!("user_{USER_ID}").as_str()));
        for course_id in COURSES {
            assert!(
                contexts.contains(&format!("course_{course_id}").as_str()),
                "course_{course_id} has no event"
            );
        }

        let announcements = responses
            .iter()
            .find(|r| r.path == "/api/v1/announcements")
            .expect("announcements");
        assert_eq!(announcements.body.as_array().unwrap().len(), 10);
    }

    /// The generated bodies must decode with the models the binary uses.
    /// A fixture the client cannot parse turns into an unexplained `decode`
    /// error deep inside a benchmark run.
    #[test]
    fn every_body_decodes_with_the_real_models() {
        use canvas_api::models::{
            Announcement, Assignment, CalendarEvent, Conversation, Course, DiscussionEntry,
            DiscussionTopic, Enrollment, File, Folder, GradingPeriod, MissingSubmission, Module,
            PlannerItem, UnreadCount, User, WikiPage, WrappedCollection,
        };
        fn check<T: serde::de::DeserializeOwned>(path: &str, body: &Value) {
            serde_json::from_value::<T>(body.clone())
                .unwrap_or_else(|e| panic!("{path} does not decode: {e}"));
        }
        let origin: reqwest::Url = "https://canvas.example.edu".parse().unwrap();
        canvas_api::serde_util::with_origin(&origin, || {
            for recorded in responses() {
                let path = recorded.path.as_str();
                let body = &recorded.body;
                if path == "/api/v1/users/self" {
                    check::<User>(path, body);
                } else if path == "/api/v1/courses" {
                    check::<Vec<Course>>(path, body);
                } else if path == "/api/v1/users/self/enrollments" {
                    check::<Vec<Enrollment>>(path, body);
                } else if path == "/api/v1/planner/items" {
                    check::<Vec<PlannerItem>>(path, body);
                } else if path == "/api/v1/users/self/missing_submissions" {
                    check::<Vec<MissingSubmission>>(path, body);
                } else if path.ends_with("/grading_periods") {
                    check::<WrappedCollection<GradingPeriod>>(path, body);
                } else if path.ends_with("/assignments") {
                    check::<Vec<Assignment>>(path, body);
                } else if path == "/api/v1/search/recipients"
                    || path == "/api/v1/users/self/files"
                    || path == "/upload/user-files"
                {
                    // The M8-b write surface: `canvas-api` decodes the two
                    // upload responses with its own upload types, and the
                    // recipient search is read as a raw value by
                    // `operations::prepare`. Both are checked here as shape.
                    assert!(body.is_array() || body.is_object(), "{path}");
                } else if path.ends_with("/folders") {
                    check::<Vec<Folder>>(path, body);
                } else if path.ends_with("/files") {
                    check::<Vec<File>>(path, body);
                } else if path.ends_with("/modules") {
                    check::<Vec<Module>>(path, body);
                } else if path == "/api/v1/announcements" {
                    check::<Vec<Announcement>>(path, body);
                } else if path == "/api/v1/calendar_events" {
                    check::<Vec<CalendarEvent>>(path, body);
                } else if path.starts_with("/api/v1/files/") {
                    check::<File>(path, body);
                } else if path == "/api/v1/conversations/unread_count" {
                    check::<UnreadCount>(path, body);
                } else if path.ends_with("/pages") {
                    check::<Vec<WikiPage>>(path, body);
                } else if path.contains("/pages/") {
                    check::<WikiPage>(path, body);
                } else if path.ends_with("/discussion_topics") {
                    check::<Vec<DiscussionTopic>>(path, body);
                } else if path.ends_with("/entries") {
                    // The gated topic answers 403 with an error body, not a list.
                    if recorded.status == 200 {
                        check::<Vec<DiscussionEntry>>(path, body);
                    }
                } else if path.contains("/discussion_topics/") {
                    check::<DiscussionTopic>(path, body);
                } else if path == "/api/v1/conversations" {
                    check::<Vec<Conversation>>(path, body);
                } else if path.starts_with("/api/v1/conversations/") {
                    check::<Conversation>(path, body);
                } else {
                    panic!("{path} is not covered by the decode check");
                }
            }
        });
    }

    /// Only the file `url` is fetched, so only it must stay relative; every
    /// other link is decoration that points at a placeholder host.
    /// `canvas-core` decodes planner items from a raw value, outside the
    /// origin scope every other model is decoded in. The set must survive that.
    #[test]
    fn planner_items_decode_without_an_origin() {
        use canvas_api::models::PlannerItem;
        let planner = responses()
            .into_iter()
            .find(|r| r.path == "/api/v1/planner/items")
            .expect("planner items");
        for item in planner.body.as_array().expect("array") {
            serde_json::from_value::<PlannerItem>(item.clone())
                .unwrap_or_else(|e| panic!("planner item does not decode: {e}\n{item}"));
        }
    }

    #[test]
    fn fetched_file_urls_stay_relative_so_any_mock_address_works() {
        fn check(path: &str, value: &Value) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        // Only a file row's own `url` is fetched. A wiki page
                        // also has a `url`, and there it is the slug.
                        if key == "url"
                            && map.contains_key("filename")
                            && let Some(text) = child.as_str()
                        {
                            assert!(
                                text.starts_with(BLOB_PREFIX),
                                "{path}: a fetched url must be relative, got {text}"
                            );
                        }
                        check(path, child);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        check(path, item);
                    }
                }
                _ => {}
            }
        }
        let mut seen = 0;
        for recorded in responses() {
            check(&recorded.path, &recorded.body);
            seen += usize::from(recorded.path.starts_with("/api/v1/files/"));
        }
        assert_eq!(seen, DOWNLOAD_FILES.len());
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

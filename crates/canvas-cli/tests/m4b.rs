//! End-to-end M4-b tests: announcements, announcement, calendar, ICS,
//! and the `sync --full` assembly (§16).

use std::process::Command;
use std::sync::Mutex;

use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::{
    Mock, MockServer, Request, Respond, ResponseTemplate,
    matchers::{method, path},
};

const TOKEN: &str = "review-secret-token";

struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}

impl Fixture {
    fn new(origin: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new(origin, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        let key = doc.key.to_string();
        open.store
            .call_blocking(move |conns| {
                conns.state.execute(
                    "INSERT INTO credential (identity_key,token_sha256,validated_at)
                     VALUES (?1,?2,'2026-01-01T00:00:00Z')",
                    rusqlite::params![key, format!("{:x}", Sha256::digest(TOKEN.as_bytes()))],
                )?;
                Ok(())
            })
            .unwrap();
        Self { dir, doc }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_canvas"));
        command
            .env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("TZ", "America/New_York")
            .env("COLUMNS", "100")
            .env("CANVAS_TOKEN", TOKEN)
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        command
    }

    async fn run(&self, args: &[&str], code: i32) -> Value {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit"], code);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        value
    }

    fn seed_assignment(&self) {
        let paths = Paths::for_identity(self.dir.path().join("data"), &self.doc.key);
        let open = OpenIdentity::open(&paths, &self.doc).unwrap();
        open.store
            .call_blocking(|conns| {
                conns.cache.execute(
                    "INSERT INTO assignments
                         (id, course_id, name, due_at, points_possible, html_url,
                          submitted, graded, missing)
                     VALUES (500, 1, 'Problem Set 2', '2026-09-10T03:59:00Z', 10,
                             'https://canvas.test/courses/1/assignments/500', 0, 0, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// Human output plus stderr, with the expected exit code.
    async fn human(&self, args: &[&str], code: i32) -> (String, String) {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }
}

/// Canvas answers a batch as a unit: one unreadable context fails all of it.
struct Batches {
    denied: Vec<String>,
    seen: Mutex<Vec<Vec<String>>>,
    body: fn(&str) -> Value,
}

impl Batches {
    fn new(denied: &[&str], body: fn(&str) -> Value) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            denied: denied.iter().map(|s| (*s).to_owned()).collect(),
            seen: Mutex::new(Vec::new()),
            body,
        })
    }
}

struct Proxy(std::sync::Arc<Batches>);

impl Respond for Proxy {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let codes: Vec<String> = req
            .url
            .query_pairs()
            .filter(|(key, _)| key == "context_codes[]")
            .map(|(_, value)| value.into_owned())
            .collect();
        self.0.seen.lock().unwrap().push(codes.clone());
        if codes.iter().any(|code| self.0.denied.contains(code)) {
            return ResponseTemplate::new(403).set_body_string("user not authorized");
        }
        ResponseTemplate::new(200).set_body_json((self.0.body)(&codes.join(",")))
    }
}

fn courses() -> Value {
    json!([
        {"id": 1, "course_code": "CHEM", "name": "Chemistry"},
        {"id": 2, "course_code": "PHYS", "name": "Physics"},
    ])
}

async fn mount(server: &MockServer, url: &str, value: Value) {
    Mock::given(path(url))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .mount(server)
        .await;
}

/// Ports and identity keys change per run; snapshots must not carry them.
fn normalized(mut value: Value) -> Value {
    value["identity"] = json!({"origin":"https://canvas.test","user_id":"123","key":"fixture"});
    if let Some(items) = value
        .get_mut("result")
        .and_then(|r| r.get_mut("items"))
        .and_then(Value::as_array_mut)
    {
        for item in items {
            let uid = item["uid"].as_str().unwrap_or_default();
            let head = uid.split('@').next().unwrap_or_default().to_owned();
            item["uid"] = json!(format!("{head}@fixture"));
        }
    }
    for row in value
        .get_mut("freshness")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        // The context hash covers the course set, which is stable, but the
        // scope string is long; keep the dataset and the window.
        if let Some(scope) = row["scope"].as_str()
            && let Some((window, _)) = scope.split_once(":ctx:")
        {
            row["scope"] = json!(format!("{window}:ctx:<hash>"));
        }
    }
    value
}

fn announcement_body(codes: &str) -> Value {
    let mut items = Vec::new();
    for code in codes.split(',') {
        if code != "course_1" {
            continue;
        }
        items.push(json!({
            "id": 9001,
            "context_code": code,
            "title": "Lab 3 is posted",
            "message": "<p>Read <strong>chapter 3</strong> first.</p>",
            "posted_at": "2026-09-08T14:00:00Z",
            "read_state": "unread",
            "user_name": "Ada Lovelace",
            "html_url": "https://canvas.test/courses/1/discussion_topics/9001",
        }));
        items.push(json!({
            "id": 9000,
            "context_code": code,
            "title": "Quiz 2 moved to Friday",
            "message": "<p>Friday.</p>",
            "posted_at": "2026-09-02T18:30:00Z",
            "read_state": "read",
            "html_url": "https://canvas.test/courses/1/discussion_topics/9000",
        }));
    }
    Value::Array(items)
}

#[tokio::test]
async fn announcements_isolate_a_denied_course_and_filter_unread_locally() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount(&server, "/api/v1/courses", courses()).await;
    let batches = Batches::new(&["course_2"], announcement_body);
    Mock::given(method("GET"))
        .and(path("/api/v1/announcements"))
        .respond_with(Proxy(batches.clone()))
        .mount(&server)
        .await;

    let out = f.run(&["announcements"], 12).await;
    // One batch, then one request per course in it (§12.6).
    assert_eq!(
        batches.seen.lock().unwrap().clone(),
        vec![
            vec!["course_1".to_owned(), "course_2".to_owned()],
            vec!["course_1".to_owned()],
            vec!["course_2".to_owned()],
        ]
    );
    assert_eq!(out["requests"]["api"], 4);
    assert_eq!(out["outcome"], "partial");
    assert_eq!(out["partial"][0]["scope"], "announcements:course:2");
    assert_eq!(out["partial"][0]["http_status"], 403);
    assert_eq!(
        out["result"]["window"],
        json!({"start":"2026-08-26","end":"2026-09-09"})
    );
    let items = out["result"]["announcements"].as_array().unwrap();
    // Appendix D: `posted_at` descending, then id.
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], "9001");
    assert_eq!(items[0]["course_code"], "CHEM");
    assert_eq!(items[0]["author"], "Ada Lovelace");
    assert_eq!(items[0]["read"], false);
    assert_eq!(items[1]["id"], "9000");
    insta::assert_json_snapshot!("announcements_json", normalized(out));

    // The window is cached, so `--unread` needs no request and stays local.
    let unread = f.run(&["announcements", "--unread"], 12).await;
    assert_eq!(unread["requests"]["api"], 0);
    let items = unread["result"]["announcements"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "9001");

    // A cached read still names the course that could not be read.
    let offline = f.run(&["announcements", "--offline"], 12).await;
    assert_eq!(offline["partial"][0]["scope"], "announcements:course:2");

    let (stdout, stderr) = f.human(&["announcements", "--offline"], 12).await;
    assert!(stderr.contains("HTTP 403"), "stderr={stderr}");
    insta::assert_snapshot!("announcements_human", stdout);

    // Nothing is marked read in v1: every request was a read (§12.6).
    let seen = server.received_requests().await.unwrap();
    assert!(
        seen.iter().all(|r| r.method == wiremock::http::Method::GET),
        "a write reached Canvas"
    );

    // `--since` takes a duration.
    f.run(&["announcements", "--since", "soon"], 2).await;
    f.run(&["announcements", "--since", "0d"], 2).await;

    // A shorter `--since` filters the same cached window to the exact instant.
    let recent = f.run(&["announcements", "--since", "2d"], 12).await;
    let items = recent["result"]["announcements"].as_array().unwrap();
    assert_eq!(items.len(), 1, "only the newer announcement is inside 2d");
    assert_eq!(items[0]["id"], "9001");
    assert_eq!(
        recent["result"]["window"],
        json!({"start":"2026-09-07","end":"2026-09-09"})
    );
}

#[tokio::test]
async fn announcement_accepts_a_url_and_refuses_a_bare_id() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount(
        &server,
        "/api/v1/courses/1/discussion_topics/9001",
        json!({
            "id": 9001,
            "title": "Lab 3 is posted",
            "message": "<p>Read <strong>chapter 3</strong> first.</p>",
            "posted_at": "2026-09-08T14:00:00Z",
            "read_state": "unread",
            "user_name": "Ada Lovelace",
            "html_url": "https://canvas.test/courses/1/discussion_topics/9001",
        }),
    )
    .await;

    let bare = f.run(&["announcement", "9001"], 6).await;
    assert!(
        bare["result"]["message"]
            .as_str()
            .unwrap()
            .contains("bare announcement IDs are not accepted"),
        "{bare}"
    );

    let url = format!("{}/courses/1/discussion_topics/9001", server.uri());
    let out = f.run(&["announcement", &url], 0).await;
    assert_eq!(out["requests"]["api"], 1);
    let item = &out["result"]["announcement"];
    assert_eq!(item["id"], "9001");
    assert_eq!(item["course_id"], "1");
    assert_eq!(
        item["message_markdown"], "Read **chapter 3** first.",
        "the message is Markdown"
    );
    insta::assert_json_snapshot!("announcement_json", normalized(out));

    // `<course> <id>` reaches the same announcement, now from cache.
    let by_pair = f.run(&["announcement", "1", "9001"], 0).await;
    assert_eq!(by_pair["requests"]["api"], 0);
    assert_eq!(by_pair["result"]["announcement"]["id"], "9001");

    let foreign = f
        .run(
            &[
                "announcement",
                "https://other.instructure.com/courses/1/discussion_topics/9001",
            ],
            6,
        )
        .await;
    assert!(
        foreign["result"]["message"]
            .as_str()
            .unwrap()
            .contains("origin"),
        "{foreign}"
    );

    let (stdout, _) = f
        .human(&["announcement", "1", "9001", "--offline"], 0)
        .await;
    insta::assert_snapshot!("announcement_human", stdout);
}

fn planner_items() -> Value {
    json!([
        {
            "plannable_type": "assignment",
            "plannable_id": 500,
            "course_id": 1,
            "plannable_date": "2026-09-10T03:59:00Z",
            "plannable": {"id": 500, "title": "Problem Set 2", "due_at": "2026-09-10T03:59:00Z", "points_possible": 10},
            "html_url": "https://canvas.test/courses/1/assignments/500",
            "submissions": {"submitted": false, "graded": false},
        },
        {
            "plannable_type": "calendar_event",
            "plannable_id": 700,
            "course_id": 1,
            "plannable_date": "2026-09-10T15:00:00Z",
            "plannable": {"id": 700, "title": "Planner copy of the review session"},
            "html_url": "https://canvas.test/calendar?event_id=700",
        },
    ])
}

/// Canvas answers with the events in the requested window. The November
/// event is here on purpose: it is the all-day span across a DST change that
/// §12.5 asks the ICS rules to cover, and the tests ask for a 60-day window
/// so it falls inside.
fn calendar_events(_codes: &str) -> Value {
    json!([
        {
            "id": 700,
            "title": "Review session; bring notes, please",
            "context_code": "course_1",
            "start_at": "2026-09-10T15:00:00Z",
            "end_at": "2026-09-10T16:00:00Z",
            "html_url": "https://canvas.test/calendar?event_id=700",
            "workflow_state": "active",
        },
        {
            "id": 701,
            "title": "Reading day",
            "context_code": "user_123",
            "start_at": "2026-09-14T04:00:00Z",
            "end_at": "2026-09-14T04:00:00Z",
            "all_day": true,
            "all_day_date": "2026-09-14",
        },
        {
            "id": 702,
            "title": "Reading period starts",
            "context_code": "course_1",
            // 25 hours across the autumn DST change; still one civil day.
            "start_at": "2026-11-01T04:00:00Z",
            "end_at": "2026-11-02T05:00:00Z",
            "all_day": true,
            "all_day_date": "2026-11-01",
        },
    ])
}

async fn calendar_fixture(server: &MockServer) -> Fixture {
    let f = Fixture::new(&server.uri());
    // The planner carries no points; they come from the cached assignment,
    // which is what feeds the ICS DESCRIPTION (§12.5).
    f.seed_assignment();
    mount(server, "/api/v1/courses", courses()).await;
    mount(server, "/api/v1/planner/items", planner_items()).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/calendar_events"))
        .respond_with(Proxy(Batches::new(&[], calendar_events)))
        .mount(server)
        .await;
    f
}

#[tokio::test]
async fn calendar_deduplicates_events_and_keeps_the_event_fields() {
    let server = MockServer::start().await;
    let f = calendar_fixture(&server).await;

    let out = f.run(&["calendar", "--days", "60"], 0).await;
    // courses, planner, one batch of three contexts.
    assert_eq!(out["requests"]["api"], 3);
    let items = out["result"]["items"].as_array().unwrap();
    let events: Vec<_> = items.iter().filter(|i| i["kind"] == "event").collect();
    assert_eq!(events.len(), 3, "planner copy is de-duplicated: {items:#?}");
    let merged = events.iter().find(|i| i["id"] == "700").unwrap();
    assert_eq!(
        merged["title"], "Review session; bring notes, please",
        "the calendar-events representation wins"
    );
    assert_eq!(merged["is_deadline"], false);
    assert_eq!(merged["start_at"], "2026-09-10T15:00:00Z");
    assert_eq!(merged["end_at"], "2026-09-10T16:00:00Z");
    assert_eq!(merged["course_code"], "CHEM");
    assert!(
        merged["uid"]
            .as_str()
            .unwrap()
            .starts_with("canvas-event-700@")
    );

    let deadline = items.iter().find(|i| i["kind"] == "assignment").unwrap();
    assert_eq!(deadline["is_deadline"], true);
    assert_eq!(deadline["due_at"], "2026-09-10T03:59:00Z");
    assert!(deadline["start_at"].is_null());

    // An all-day event is one civil day; a longer span is warned about.
    let one_day = items.iter().find(|i| i["id"] == "701").unwrap();
    assert_eq!(one_day["all_day"], true);
    assert_eq!(one_day["all_day_date"], "2026-09-14");
    let warnings = out["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap()
            .contains("Reading period starts: all-day event with a longer span")),
        "{warnings:#?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("Reading day")),
        "equal start and end is not a longer span"
    );
    insta::assert_json_snapshot!("calendar_json", normalized(out));

    let (stdout, _) = f.human(&["calendar", "--days", "60", "--offline"], 0).await;
    insta::assert_snapshot!("calendar_human", stdout);

    // `--course` keeps the course's own items.
    let one_course = f
        .run(
            &["calendar", "--days", "60", "--course", "1", "--offline"],
            0,
        )
        .await;
    assert!(
        one_course["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["course_id"] == "1")
    );
}

#[tokio::test]
async fn ics_streams_raw_text_that_follows_rfc_5545() {
    let server = MockServer::start().await;
    let f = calendar_fixture(&server).await;
    f.run(&["calendar", "--days", "60"], 0).await;

    let (ics, stderr) = f
        .human(
            &[
                "calendar",
                "--days",
                "60",
                "--offline",
                "--ics",
                "-",
                "--alarm",
                "24h",
            ],
            0,
        )
        .await;
    // No envelope: the raw file is the whole of stdout (§7).
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"), "{ics}");
    assert!(!ics.contains("\"schema\""));
    assert!(ics.contains("VERSION:2.0\r\n"));
    assert!(ics.contains("PRODID:-//canvas-cli//EN\r\n"));
    assert!(ics.ends_with("END:VCALENDAR\r\n"));
    for line in ics.split("\r\n") {
        assert!(line.len() <= 75, "unfolded line: {line:?}");
    }
    assert!(
        stderr.contains("all-day event with a longer span"),
        "stderr={stderr}"
    );

    // §3.3.11 escaping in SUMMARY.
    assert!(
        ics.contains(r"SUMMARY:[CHEM] Review session\; bring notes\, please"),
        "{ics}"
    );
    // A timed event keeps DTEND; an all-day event is a date with no DTEND.
    assert!(ics.contains("DTSTART:20260910T150000Z\r\n"));
    assert!(ics.contains("DTEND:20260910T160000Z\r\n"));
    assert!(ics.contains("DTSTART;VALUE=DATE:20260914\r\n"));
    assert!(ics.contains("DTSTART;VALUE=DATE:20261101\r\n"));
    assert_eq!(
        ics.matches("DTEND").count(),
        1,
        "only the timed event has an end"
    );
    // A point deadline carries no DURATION, and the alarm rides on it.
    assert!(ics.contains("DTSTART:20260910T035900Z\r\n"));
    assert!(!ics.contains("DURATION"));
    assert_eq!(ics.matches("BEGIN:VALARM").count(), 1);
    assert!(ics.contains("TRIGGER:-PT24H\r\n"));
    assert!(ics.contains("DTSTAMP:20260909T170512Z\r\n"));
    assert_eq!(ics.matches("BEGIN:VEVENT").count(), 4);
    // §12.5: DESCRIPTION carries points and status for a deadline.
    assert!(ics.contains("DESCRIPTION:10 points · unknown\r\n"), "{ics}");
    assert_eq!(
        ics.matches("DESCRIPTION:").count(),
        2,
        "one alarm, one deadline"
    );

    // Without `--alarm` there is no VALARM at all.
    let (plain, _) = f
        .human(&["calendar", "--days", "60", "--offline", "--ics", "-"], 0)
        .await;
    assert!(!plain.contains("VALARM"));

    // `--ics PATH` writes the same text and keeps the envelope.
    let out = f.dir.path().join("canvas.ics");
    let written = f
        .run(
            &[
                "calendar",
                "--days",
                "60",
                "--offline",
                "--ics",
                out.to_str().unwrap(),
            ],
            0,
        )
        .await;
    assert_eq!(written["schema"], "canvas-cli/calendar@1");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), plain);

    // An unparseable alarm is a usage error (§14).
    f.run(
        &["calendar", "--days", "60", "--offline", "--alarm", "soon"],
        2,
    )
    .await;
}

#[tokio::test]
async fn sync_full_covers_every_dataset_and_counts_requests() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    mount(
        &server,
        "/api/v1/courses",
        json!([{"id": 1, "course_code": "CHEM", "name": "Chemistry"}]),
    )
    .await;
    for endpoint in [
        "/api/v1/users/self/enrollments",
        "/api/v1/courses/1/assignments",
        "/api/v1/users/self/missing_submissions",
        "/api/v1/planner/items",
        "/api/v1/courses/1/folders",
        "/api/v1/courses/1/files",
        "/api/v1/courses/1/modules",
    ] {
        mount(&server, endpoint, json!([])).await;
    }
    mount(
        &server,
        "/api/v1/courses/1/grading_periods",
        json!({"grading_periods": []}),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/announcements"))
        .respond_with(Proxy(Batches::new(&[], |_| json!([]))))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/calendar_events"))
        .respond_with(Proxy(Batches::new(&[], |_| json!([]))))
        .mount(&server)
        .await;

    // courses, enrollments, grading periods, assignments, missing, planner,
    // announcements.
    let base = f.run(&["sync"], 0).await;
    assert_eq!(base["requests"]["api"], 7);
    let names = dataset_names(&base);
    for name in [
        "courses",
        "enrollment_grades",
        "grading_periods",
        "assignments",
        "missing",
        "planner",
        "announcements",
    ] {
        assert!(
            names.contains(&name.to_owned()),
            "{name} missing: {names:?}"
        );
    }
    for name in ["files", "folders", "modules", "calendar_events"] {
        assert!(!names.contains(&name.to_owned()), "{name} is `--full` only");
    }

    // `--full` adds folders, files, modules with their items, and events.
    let full = f.run(&["sync", "--full"], 0).await;
    assert_eq!(full["requests"]["api"], 11);
    let names = dataset_names(&full);
    for name in ["folders", "files", "modules", "calendar_events"] {
        assert!(
            names.contains(&name.to_owned()),
            "{name} missing: {names:?}"
        );
    }
    let rows = full["result"]["datasets"].as_array().unwrap();
    let counted: u64 = rows
        .iter()
        .map(|row| row["requests"].as_u64().unwrap())
        .sum();
    assert_eq!(counted, 11, "every request belongs to a dataset");
    assert!(rows.iter().all(|row| row["source"] == "network"
        || row["dataset"] == "terms"
        || row["dataset"] == "course_totals"));
}

fn dataset_names(value: &Value) -> Vec<String> {
    value["result"]["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["dataset"].as_str().unwrap().to_owned())
        .collect()
}

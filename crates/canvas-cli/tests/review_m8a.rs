//! M8-a acceptance: pages, syllabus, discussions, and inbox reads.
//!
//! Every case here is one row of the REPORT §4 M8-a acceptance column.

use std::process::Command;

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CROSS_ORIGIN: &str = "https://elsewhere.test/notes";

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
        OpenIdentity::open(&paths, &doc).unwrap();
        Self { dir, doc }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_canvas"));
        cmd.env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("CANVAS_TOKEN", "m8a-test-token")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        cmd
    }

    async fn run(&self, args: &[&str], exit: i32) -> Value {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit"], exit);
        value
    }

    /// The human rendering of one command, for the table-mode snapshots.
    async fn text(&self, args: &[&str], exit: i32) -> String {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(exit), "{args:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

fn normalized(mut value: Value, origin: &str) -> Value {
    value["identity"] = json!({"origin":"https://canvas.test","user_id":"123","key":"fixture"});
    value["generated_at"] = json!("<generated_at>");
    // The mock server address changes per run; hide it from the snapshots.
    scrub(&mut value["result"], origin.trim_end_matches('/'));
    value
}

/// Hide the mock server address in a human-rendered snapshot.
fn scrub_text(text: &str, origin: &str) -> String {
    text.replace(origin.trim_end_matches('/'), "<origin>")
}

fn scrub(value: &mut Value, origin: &str) {
    match value {
        Value::String(text) => *text = text.replace(origin, "<origin>"),
        Value::Array(items) => items.iter_mut().for_each(|item| scrub(item, origin)),
        Value::Object(map) => map.values_mut().for_each(|item| scrub(item, origin)),
        _ => {}
    }
}

async fn mount(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route.to_owned()))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn page_row(id: i64, slug: &str, published: bool, front: bool) -> Value {
    json!({
        "page_id": id,
        "url": slug,
        "title": format!("Page {id}"),
        "created_at": "2026-08-01T12:00:00Z",
        "updated_at": "2026-09-01T14:00:00Z",
        "published": published,
        "front_page": front,
        "locked_for_user": false,
        "html_url": format!("https://canvas.test/courses/5/pages/{slug}")
    })
}

fn page_body(origin: &str) -> Value {
    let mut row = page_row(301, "course-overview", true, true);
    row["body"] = json!(format!(
        concat!(
            "<p>Read the <a href=\"{origin}/courses/5/files/42\">handbook</a>.</p>",
            "<p>Also <a href=\"{cross}\">these notes</a>.</p>",
            "<iframe src=\"https://player.test/embed/9\"></iframe>",
            "<iframe src=\"/courses/5/external_tools/retrieve?url=x\"></iframe>"
        ),
        origin = origin,
        cross = CROSS_ORIGIN
    ));
    row
}

fn topic(id: i64) -> Value {
    json!({
        "id": id,
        "title": format!("Topic {id}"),
        "message": "<p>What surprised you?</p>",
        "posted_at": "2026-09-01T14:00:00Z",
        "last_reply_at": "2026-09-08T10:00:00Z",
        "discussion_type": "threaded",
        "user_name": "Dr. Reed",
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
        "context_code": "course_5",
        "html_url": format!("https://canvas.test/courses/5/discussion_topics/{id}")
    })
}

fn gated_topic(id: i64) -> Value {
    let mut row = topic(id);
    row["title"] = json!("Introduce yourself");
    row["require_initial_post"] = json!(true);
    row["user_can_see_posts"] = json!(false);
    row
}

fn announcement_topic(id: i64) -> Value {
    let mut row = topic(id);
    row["title"] = json!("Welcome");
    row["is_announcement"] = json!(true);
    row
}

fn graded_group_topic(id: i64) -> Value {
    let mut row = topic(id);
    row["title"] = json!("Group lab report");
    row["assignment_id"] = json!(2);
    row["points_possible"] = json!(15.0);
    row["group_category_id"] = json!(9);
    row["group_topic_children"] = json!([{"id": 58, "group_id": 77}, {"id": 59, "group_id": 78}]);
    row
}

fn entry(id: i64, n: i64) -> Value {
    json!({
        "id": id,
        "parent_id": null,
        "user_id": 30 + n,
        "user_name": format!("Student {n}"),
        "message": format!("<p>Reply {n}.</p>"),
        "created_at": "2026-09-02T09:00:00Z",
        "updated_at": "2026-09-02T09:00:00Z",
        "read_state": "unread",
        "has_more_replies": false,
        "recent_replies": []
    })
}

fn conversation(id: i64) -> Value {
    json!({
        "id": id,
        "subject": format!("Conversation {id}"),
        "workflow_state": "unread",
        "last_message": "See you then.",
        "last_message_at": "2026-09-09T12:00:00Z",
        "message_count": 2,
        "subscribed": true,
        "private": true,
        "starred": false,
        "context_name": "CHEM-101",
        "participants": [{"id": 123, "name": "You"}, {"id": 31, "name": "Alex Kim"}]
    })
}

fn conversation_detail(id: i64) -> Value {
    let mut row = conversation(id);
    row["messages"] = json!([
        {
            "id": 9001,
            "author_id": 31,
            "created_at": "2026-09-09T12:00:00Z",
            "body": "Can we meet before the lab?",
            "generated": false,
            "attachments": [{"id": 42, "display_name": "notes.pdf", "size": 20480}]
        }
    ]);
    row
}

/// Mount the whole M8-a read surface for course 5.
async fn mount_all(server: &MockServer) {
    let origin = server.uri();
    mount(server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(
        server,
        "/api/v1/courses/5/pages",
        json!([
            page_row(301, "course-overview", true, true),
            page_row(302, "draft-notes", false, false),
        ]),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/5/pages/course-overview",
        page_body(&origin),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/5/discussion_topics",
        json!([
            topic(55),
            gated_topic(56),
            graded_group_topic(57),
            announcement_topic(60),
        ]),
    )
    .await;
    for row in [
        topic(55),
        gated_topic(56),
        graded_group_topic(57),
        announcement_topic(60),
    ] {
        let id = row["id"].as_i64().unwrap();
        mount(
            server,
            &format!("/api/v1/courses/5/discussion_topics/{id}"),
            row,
        )
        .await;
    }
    mount(
        server,
        "/api/v1/conversations",
        json!([conversation(700), conversation(701), conversation(702)]),
    )
    .await;
    mount(
        server,
        "/api/v1/conversations/700",
        conversation_detail(700),
    )
    .await;
    mount(
        server,
        "/api/v1/conversations/unread_count",
        json!({"unread_count": "2"}),
    )
    .await;
}

/// Two reply pages, with the second one either served or refused.
async fn mount_replies(server: &MockServer, second_page_ok: bool) {
    let next = format!(
        "{}/api/v1/courses/5/discussion_topics/55/entries?page=2",
        server.uri()
    );
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/discussion_topics/55/entries"))
        .and(query_param("per_page", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([entry(900, 1), entry(901, 2)]))
                .insert_header("Link", format!("<{next}>; rel=\"next\"").as_str()),
        )
        .mount(server)
        .await;
    let second = if second_page_ok {
        ResponseTemplate::new(200).set_body_json(json!([entry(902, 3)]))
    } else {
        ResponseTemplate::new(403).set_body_json(json!({"status": "unauthorized"}))
    };
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/discussion_topics/55/entries"))
        .and(query_param("page", "2"))
        .respond_with(second)
        .mount(server)
        .await;
}

/// SPEC §16: this package reads. Nothing it sends may change server state.
async fn assert_only_get(server: &MockServer) {
    for request in server.received_requests().await.unwrap() {
        assert_eq!(
            request.method.as_str(),
            "GET",
            "{} {} is not a read",
            request.method,
            request.url
        );
    }
}

fn requested(requests: &[Request], needle: &str) -> bool {
    requests.iter().any(|r| r.url.as_str().contains(needle))
}

#[tokio::test]
async fn a_page_reports_its_embedded_content_files_and_external_links() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());

    let listing = f.run(&["pages", "5"], 0).await;
    // A page Canvas reports as unpublished stays out until it is asked for.
    assert_eq!(listing["result"]["pages"].as_array().unwrap().len(), 1);
    assert_eq!(listing["result"]["listing"]["available"], true);
    let all = f.run(&["pages", "5", "--unpublished"], 0).await;
    assert_eq!(all["result"]["pages"].as_array().unwrap().len(), 2);

    let out = f.run(&["page", "5", "course-overview"], 0).await;
    let page = &out["result"]["page"];
    let kinds: Vec<&str> = page["embedded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["iframe", "lti"]);
    for row in page["embedded"].as_array().unwrap() {
        assert_eq!(row["reported"], "unavailable");
    }
    assert!(
        page["body_markdown"]
            .as_str()
            .unwrap()
            .contains("[embedded iframe: unavailable to this tool]")
    );
    // Same-origin file references are listed; a cross-origin one never is.
    assert_eq!(page["files"].as_array().unwrap().len(), 1);
    assert_eq!(page["files"][0]["file_id"], "42");
    assert_eq!(page["external_links"].as_array().unwrap().len(), 1);
    assert_eq!(page["external_links"][0]["url"], CROSS_ORIGIN);
    assert_eq!(page["truncated"], false);

    // Neither the file nor the embed is fetched: they are reported, not read.
    let requests = server.received_requests().await.unwrap();
    assert!(!requested(&requests, "/files/42"));
    assert!(!requested(&requests, "/embed/9"));
    assert!(!requested(&requests, "external_tools"));
    assert_only_get(&server).await;

    insta::assert_json_snapshot!("m8a_page_json", normalized(out, &server.uri()));
    insta::assert_snapshot!(
        "m8a_pages_table",
        scrub_text(&f.text(&["pages", "5"], 0).await, &server.uri())
    );
    insta::assert_snapshot!(
        "m8a_page_table",
        scrub_text(
            &f.text(&["page", "5", "course-overview"], 0).await,
            &server.uri()
        )
    );
}

#[tokio::test]
async fn a_denied_listing_is_partial_and_a_denied_page_is_refused() {
    let server = MockServer::start().await;
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/pages"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/pages/secret"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let f = Fixture::new(&server.uri());

    let listing = f.run(&["pages", "5"], 12).await;
    assert_eq!(listing["outcome"], "partial");
    assert_eq!(listing["partial"][0]["scope"], "pages:course:5");
    assert_eq!(listing["partial"][0]["http_status"], 403);
    assert_eq!(listing["result"]["listing"]["available"], false);
    // The denial is coverage, so a second read reports it from the cache.
    let cached = f.run(&["pages", "5"], 12).await;
    assert_eq!(cached["partial"][0]["http_status"], 403);

    let item = f.run(&["page", "5", "secret"], 8).await;
    assert_eq!(item["result"]["code"], "refused");
    assert_only_get(&server).await;
}

#[tokio::test]
async fn paginated_replies_report_complete_and_incomplete_coverage() {
    for complete in [true, false] {
        let server = MockServer::start().await;
        mount_all(&server).await;
        mount_replies(&server, complete).await;
        let f = Fixture::new(&server.uri());

        let exit = if complete { 0 } else { 12 };
        let out = f.run(&["discussion", "5", "55", "--replies"], exit).await;
        let topic = &out["result"]["discussion"];
        let coverage = &topic["replies_coverage"];
        assert_eq!(coverage["complete"], complete);
        if complete {
            assert_eq!(coverage["pages_fetched"], 2);
            assert_eq!(topic["replies"].as_array().unwrap().len(), 3);
            assert_eq!(coverage["blocked"], Value::Null);
        } else {
            // The pages that did load are kept; the thread is never called whole.
            assert_eq!(coverage["pages_fetched"], 1);
            assert_eq!(topic["replies"].as_array().unwrap().len(), 2);
            assert_eq!(coverage["blocked"], "page_failed");
            assert_eq!(out["outcome"], "partial");
            assert_eq!(out["partial"][0]["scope"], "discussion_entries:topic:55");
        }
        assert_only_get(&server).await;
        if complete {
            insta::assert_json_snapshot!("m8a_discussion_json", normalized(out, &server.uri()));
            insta::assert_snapshot!(
                "m8a_discussion_table",
                scrub_text(
                    &f.text(&["discussion", "5", "55", "--replies"], 0).await,
                    &server.uri()
                )
            );
        }
    }
}

#[tokio::test]
async fn a_topic_read_without_replies_never_claims_the_thread_is_covered() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["discussion", "5", "55"], 0).await;
    let coverage = &out["result"]["discussion"]["replies_coverage"];
    assert_eq!(coverage["complete"], false);
    assert_eq!(coverage["pages_fetched"], 0);
    assert_eq!(coverage["blocked"], "not_requested");
    assert!(
        out["result"]["discussion"]["replies"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // No reply route was touched at all.
    let requests = server.received_requests().await.unwrap();
    assert!(!requested(&requests, "/entries"));
}

#[tokio::test]
async fn the_initial_post_gate_refuses_replies_and_still_reads_the_topic() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/5/discussion_topics/56/entries"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"status": "unauthorized"})))
        .mount(&server)
        .await;
    let f = Fixture::new(&server.uri());

    let topic = f.run(&["discussion", "5", "56"], 0).await;
    assert_eq!(topic["result"]["discussion"]["require_initial_post"], true);

    let gated = f.run(&["discussion", "5", "56", "--replies"], 8).await;
    assert_eq!(gated["result"]["code"], "denied");
    assert!(
        gated["result"]["message"]
            .as_str()
            .unwrap()
            .starts_with("initial_post_required")
    );
    assert_only_get(&server).await;
}

#[tokio::test]
async fn a_graded_group_discussion_keeps_its_metadata() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["discussion", "5", "57"], 0).await;
    let topic = &out["result"]["discussion"];
    assert_eq!(topic["assignment_id"], "2");
    assert_eq!(topic["points_possible"], 15.0);
    assert_eq!(topic["group_category_id"], "9");
    let children = topic["group_topic_children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["id"], "58");
    assert_eq!(children[0]["group_id"], "77");

    // The listing endpoint is fixed; the flags filter what is shown.
    let listing = f.run(&["discussions", "5"], 0).await;
    assert_eq!(
        listing["result"]["discussions"].as_array().unwrap().len(),
        4
    );
    let no_announcements = f
        .run(&["discussions", "5", "--announcements", "no"], 0)
        .await;
    let shown = no_announcements["result"]["discussions"]
        .as_array()
        .unwrap();
    assert_eq!(shown.len(), 3);
    assert!(shown.iter().all(|row| row["is_announcement"] == false));
    let bad = f
        .run(&["discussions", "5", "--announcements", "maybe"], 2)
        .await;
    assert_eq!(bad["result"]["code"], "usage");
    // `--unread` filters on the stored state and marks nothing.
    let unread = f.run(&["discussions", "5", "--unread"], 0).await;
    assert_eq!(unread["result"]["discussions"].as_array().unwrap().len(), 4);
    insta::assert_json_snapshot!("m8a_discussions_json", normalized(listing, &server.uri()));
    insta::assert_snapshot!(
        "m8a_discussions_table",
        scrub_text(&f.text(&["discussions", "5"], 0).await, &server.uri())
    );
}

#[tokio::test]
async fn every_inbox_request_refuses_to_mark_anything_read() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());

    let listing = f.run(&["inbox"], 0).await;
    assert_eq!(listing["result"]["scope"], "inbox");
    assert_eq!(
        listing["result"]["conversations"].as_array().unwrap().len(),
        3
    );
    let one = f.run(&["inbox", "show", "700"], 0).await;
    assert_eq!(one["result"]["conversation"]["messages_complete"], true);
    assert_eq!(
        one["result"]["conversation"]["messages"][0]["attachments"][0]["file_id"],
        "42"
    );
    let count = f.run(&["inbox", "unread-count"], 0).await;
    assert_eq!(count["result"]["unread_count"], 2);

    let requests = server.received_requests().await.unwrap();
    let mut seen = 0;
    for request in &requests {
        let url = request.url.as_str();
        if !url.contains("/api/v1/conversations") {
            continue;
        }
        seen += 1;
        if url.contains("unread_count") {
            continue;
        }
        assert!(
            url.contains("auto_mark_as_read=false"),
            "{url} may mark a conversation read"
        );
        assert!(!url.contains("auto_mark_as_read=true"), "{url}");
    }
    assert!(seen >= 3, "the inbox routes were never called");
    assert_only_get(&server).await;

    insta::assert_json_snapshot!("m8a_inbox_json", normalized(listing, &server.uri()));
    insta::assert_json_snapshot!("m8a_conversation_json", normalized(one, &server.uri()));
    insta::assert_json_snapshot!("m8a_inbox_unread_json", normalized(count, &server.uri()));
    insta::assert_snapshot!(
        "m8a_inbox_table",
        scrub_text(&f.text(&["inbox"], 0).await, &server.uri())
    );
    insta::assert_snapshot!(
        "m8a_conversation_table",
        scrub_text(&f.text(&["inbox", "show", "700"], 0).await, &server.uri())
    );
    insta::assert_snapshot!(
        "m8a_inbox_unread_table",
        scrub_text(&f.text(&["inbox", "unread-count"], 0).await, &server.uri())
    );
}

#[tokio::test]
async fn a_bad_inbox_scope_is_a_usage_error_and_sends_nothing() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());
    let out = f.run(&["inbox", "--scope", "everything"], 2).await;
    assert_eq!(out["result"]["code"], "usage");
    let requests = server.received_requests().await.unwrap();
    assert!(!requested(&requests, "/api/v1/conversations"));
}

#[tokio::test]
async fn the_syllabus_reports_its_markdown_and_the_files_it_links() {
    let server = MockServer::start().await;
    let origin = server.uri();
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(
        &server,
        "/api/v1/courses/5",
        json!({
            "id": 5,
            "course_code": "CHEM-101",
            "name": "Chemistry",
            "updated_at": "2026-09-01T14:00:00Z",
            "syllabus_body": format!(
                concat!(
                    "<p>Bring a calculator. See the ",
                    "<a href=\"{origin}/courses/5/files/42\">handbook</a> ",
                    "and <a href=\"{cross}\">these notes</a>.</p>",
                    "<iframe src=\"https://player.test/embed/9\"></iframe>"
                ),
                origin = origin,
                cross = CROSS_ORIGIN
            ),
            "enrollments": [{"type": "student", "enrollment_state": "active"}]
        }),
    )
    .await;
    let f = Fixture::new(&origin);

    let out = f.run(&["syllabus", "5"], 0).await;
    let result = &out["result"];
    assert_eq!(result["course_id"], "5");
    assert!(
        result["syllabus_markdown"]
            .as_str()
            .unwrap()
            .contains("Bring a calculator")
    );
    assert_eq!(result["files"].as_array().unwrap().len(), 1);
    assert_eq!(result["files"][0]["file_id"], "42");
    assert_eq!(result["external_links"][0]["url"], CROSS_ORIGIN);
    assert_eq!(result["embedded"][0]["kind"], "iframe");
    assert_eq!(result["truncated"], false);

    // The cache keeps the projection, never the source HTML.
    let paths = Paths::for_identity(f.dir.path().join("data"), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).unwrap();
    open.store
        .call(|conns| {
            let data: String = conns
                .cache
                .query_row("SELECT data_json FROM courses", [], |r| r.get(0))?;
            assert!(!data.contains("<iframe"));
            assert!(!data.contains("<p>"));
            assert!(data.contains("syllabus_refs"));
            Ok(())
        })
        .await
        .unwrap();

    assert_only_get(&server).await;
    insta::assert_json_snapshot!("m8a_syllabus_json", normalized(out, &server.uri()));
    insta::assert_snapshot!(
        "m8a_syllabus_table",
        scrub_text(&f.text(&["syllabus", "5"], 0).await, &server.uri())
    );
}

#[tokio::test]
async fn a_body_over_the_bound_is_cut_and_never_called_complete() {
    let server = MockServer::start().await;
    let limit = canvas_core::markdown::BODY_LIMIT;
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(
        &server,
        "/api/v1/courses/5/pages",
        json!([page_row(301, "course-overview", true, true)]),
    )
    .await;
    let mut big = page_row(301, "course-overview", true, true);
    big["body"] = json!(format!("<p>{}</p>", "x".repeat(limit + 4096)));
    mount(&server, "/api/v1/courses/5/pages/course-overview", big).await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["page", "5", "course-overview"], 12).await;
    assert_eq!(out["result"]["page"]["truncated"], true);
    assert!(
        out["result"]["page"]["body_markdown"]
            .as_str()
            .unwrap()
            .len()
            <= limit
    );
    assert_eq!(out["outcome"], "partial");
    assert_eq!(out["partial"][0]["scope"], "page:301");
    assert!(
        out["partial"][0]["message"]
            .as_str()
            .unwrap()
            .contains("not complete")
    );
}

/// A reply is a document too: cutting one is a partial answer, never a
/// complete thread.
#[tokio::test]
async fn a_cut_reply_body_is_a_partial_answer() {
    let server = MockServer::start().await;
    let limit = canvas_core::markdown::BODY_LIMIT;
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(&server, "/api/v1/courses/5/discussion_topics/55", topic(55)).await;
    let mut long = entry(9001, 1);
    long["message"] = json!(format!("<p>{}</p>", "y".repeat(limit + 4096)));
    mount(
        &server,
        "/api/v1/courses/5/discussion_topics/55/entries",
        json!([long, entry(9002, 2)]),
    )
    .await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["discussion", "5", "55", "--replies"], 12).await;
    let replies = out["result"]["discussion"]["replies"].as_array().unwrap();
    assert_eq!(replies[0]["truncated"], true);
    assert!(replies[0]["message_markdown"].as_str().unwrap().len() <= limit);
    assert_eq!(replies[1]["truncated"], false);
    // The reply set itself is covered; only the one body was cut.
    assert_eq!(
        out["result"]["discussion"]["replies_coverage"]["complete"],
        true
    );
    assert_eq!(out["outcome"], "partial");
    assert_eq!(out["partial"][0]["scope"], "discussion_entries:topic:55");
    assert!(
        out["partial"][0]["message"]
            .as_str()
            .unwrap()
            .contains("not complete")
    );
}

/// A Canvas body and a Canvas `html_url` can both carry a capability. §15
/// keeps one out of the cache and out of the JSON.
#[tokio::test]
async fn a_capability_bearing_url_never_reaches_the_cache_or_the_json() {
    let server = MockServer::start().await;
    let origin = server.uri();
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    let mut row = page_row(301, "course-overview", true, true);
    row["html_url"] = json!(format!(
        "{origin}/courses/5/pages/x?verifier=private-capability"
    ));
    row["body"] = json!(format!(
        concat!(
            "<p><a href=\"{origin}/courses/5/files/42/download?verifier=private-capability\">h</a></p>",
            "<p><a href=\"https://elsewhere.test/n?sig=private-sig\">n</a></p>",
            "<iframe src=\"https://player.test/embed/9?access_token=private-token\"></iframe>"
        ),
        origin = origin
    ));
    mount(&server, "/api/v1/courses/5/pages/course-overview", row).await;
    let f = Fixture::new(&origin);

    let out = f.run(&["page", "5", "course-overview"], 0).await;
    let rendered = serde_json::to_string(&out).unwrap();
    assert!(!rendered.contains("private-"), "{rendered}");
    let page = &out["result"]["page"];
    assert_eq!(page["files"][0]["file_id"], "42");
    assert!(
        page["files"][0]["url"]
            .as_str()
            .unwrap()
            .ends_with("/files/42/download"),
        "{page}"
    );
    assert_eq!(page["external_links"][0]["url"], "https://elsewhere.test/n");
    assert_eq!(
        page["html_url"],
        json!(format!("{origin}/courses/5/pages/x"))
    );

    let paths = Paths::for_identity(f.dir.path().join("data"), &f.doc.key);
    let open = OpenIdentity::open(&paths, &f.doc).unwrap();
    open.store
        .call(|conns| {
            let data: String = conns
                .cache
                .query_row("SELECT data_json FROM pages", [], |r| r.get(0))?;
            assert!(!data.contains("private-"), "{data}");
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_url_operand_must_name_the_course_it_was_given() {
    let server = MockServer::start().await;
    mount_all(&server).await;
    let f = Fixture::new(&server.uri());
    let origin = server.uri();

    let ok = f
        .run(
            &[
                "page",
                "5",
                &format!("{origin}/courses/5/pages/course-overview"),
            ],
            0,
        )
        .await;
    assert_eq!(ok["result"]["page"]["url"], "course-overview");

    let wrong_course = f
        .run(
            &[
                "page",
                "5",
                &format!("{origin}/courses/6/pages/course-overview"),
            ],
            6,
        )
        .await;
    assert_eq!(wrong_course["result"]["code"], "resolution");

    let cross = f
        .run(
            &["page", "5", "https://elsewhere.test/courses/5/pages/x"],
            6,
        )
        .await;
    assert_eq!(cross["result"]["code"], "resolution");
}

/// The `assignment@1` rubric change is additive: a payload written before
/// M8-a still decodes, and the new keys appear with their absent values.
#[tokio::test]
async fn an_old_assignment_rubric_still_validates() {
    let server = MockServer::start().await;
    mount(&server, "/api/v1/users/self", json!({"id": 123})).await;
    mount(
        &server,
        "/api/v1/courses/5/assignments/2",
        json!({
            "id": 2,
            "course_id": 5,
            "name": "Homework 2",
            "points_possible": 10.0,
            "submission_types": ["online_upload"],
            "rubric": [{"id": "c1", "description": "Reasoning", "points": 10.0}]
        }),
    )
    .await;
    mount(
        &server,
        "/api/v1/courses/5/assignments/2/submissions/self",
        json!({"id": 20, "assignment_id": 2, "workflow_state": "unsubmitted"}),
    )
    .await;
    let f = Fixture::new(&server.uri());

    let out = f.run(&["assignment", "5", "2"], 0).await;
    let criterion = &out["result"]["assignment"]["rubric"][0];
    assert_eq!(criterion["id"], "c1");
    assert_eq!(criterion["description"], "Reasoning");
    assert_eq!(criterion["points"], 10.0);
    assert_eq!(criterion["long_description"], Value::Null);
    assert_eq!(criterion["criterion_use_range"], false);
    assert!(criterion["ratings"].as_array().unwrap().is_empty());
}

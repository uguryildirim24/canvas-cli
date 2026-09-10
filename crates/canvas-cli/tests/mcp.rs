//! End-to-end `canvas mcp` tests over a real stdio pipe.
//!
//! The requests here are hand-written JSON-RPC, not SDK calls, because the
//! wire format is the contract a host sees: the tool catalog, both protocol
//! revisions, the envelope inside a tool result, and the resource namespace.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "mcp-secret-token";
const PRIMARY: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";

/// The catalog REPORT §3.2 defines, in the server's order.
const CATALOG: &[&str] = &[
    "courses.list",
    "course.get",
    "todo.list",
    "assignments.list",
    "assignment.get",
    "grades.get",
    "files.list",
    "modules.list",
    "pages.list",
    "page.get",
    "syllabus.get",
    "announcements.list",
    "announcement.get",
    "discussions.list",
    "discussion.get",
    "inbox.list",
    "inbox.get",
    "inbox.unread_count",
    "calendar.list",
    "submission.get",
    "receipts.list",
    "receipts.show",
    "sync.run",
    "download.plan",
    "download.run",
    "submission.prepare",
    "submission.execute",
    "submission.reconcile",
    "receipts.acknowledge",
    "open.url",
];

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
            // `open` is the one command that would spawn a browser window.
            .env("CANVAS_TEST_NO_LAUNCH", "1")
            // A subscription re-reads the event log every two seconds in
            // production. Tests wait for notifications, so they shorten it.
            .env("CANVAS_TEST_MCP_POLL_MS", "100")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        command
    }

    /// Run the CLI and return its `--json` envelope with the exit code.
    async fn cli_any(&self, args: &[&str]) -> (Value, i32) {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (value, output.status.code().unwrap())
    }

    /// Run the CLI and return its `--json` envelope.
    async fn cli(&self, args: &[&str], code: i32) -> Value {
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
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Start `canvas mcp` on a pipe.
    fn mcp(&self, extra: &[&str]) -> Mcp {
        let mut command = self.command();
        command
            .args(extra)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Mcp {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 1,
            pending: Vec::new(),
        }
    }

    fn identity_json(&self) -> std::path::PathBuf {
        Paths::for_identity(self.dir.path().join("data"), &self.doc.key).identity_json()
    }

    /// Run one job against `state.sqlite`.
    fn state<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&mut canvas_core::store::StoreConns) -> Result<T, canvas_core::store::DbError>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let paths = Paths::for_identity(self.dir.path().join("data"), &self.doc.key);
        let open = OpenIdentity::open(&paths, &self.doc).unwrap();
        open.store.call_blocking(f).unwrap()
    }

    /// Append one event to the log, as a completed observation would.
    ///
    /// The producers live in `canvas-core::events` and have their own tests;
    /// what a subscription must do with a row is what this file measures, so
    /// the row is written directly.
    fn event(&self, kind: &str, dataset: &str, scope: &str, entity: &str) -> i64 {
        let key = self.doc.key.to_string();
        let generation = self.doc.generation.to_string();
        let (kind, dataset, scope, entity) = (
            kind.to_owned(),
            dataset.to_owned(),
            scope.to_owned(),
            entity.to_owned(),
        );
        self.state(move |conns| {
            conns.state.execute(
                "INSERT INTO events (
                     observation_id, kind, observed_at, identity_key, generation,
                     dataset, scope, entity_key, [before], [after]
                 ) VALUES (?1, ?2, '2026-09-09T17:05:12Z', ?3, ?4, ?5, ?6, ?7, '{}', '{}')",
                rusqlite::params![
                    format!("{dataset}:{scope}:1:2026-09-09T17:05:12Z"),
                    kind,
                    key,
                    generation,
                    dataset,
                    scope,
                    entity,
                ],
            )?;
            Ok(conns.state.last_insert_rowid())
        })
    }

    /// Wait until one consumer's durable position reaches `want`.
    fn wait_cursor(&self, consumer: &str, want: i64) {
        for _ in 0..100 {
            if self.cursor_of(consumer) == want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(self.cursor_of(consumer), want, "the cursor never moved");
    }

    /// The durable position of one consumer.
    fn cursor_of(&self, consumer: &str) -> i64 {
        let consumer = consumer.to_owned();
        self.state(move |conns| canvas_core::events::consumer_cursor(&conns.state, &consumer))
    }

    /// Put one consumer's position where the log cannot replay it.
    fn set_cursor(&self, consumer: &str, cursor: i64) {
        let consumer = consumer.to_owned();
        self.state(move |conns| {
            canvas_core::events::set_consumer_cursor(&mut conns.state, &consumer, cursor)
        });
    }
}

/// One `canvas mcp` process, spoken to as a host would.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    /// Notifications read while waiting for something else.
    pending: Vec<Value>,
}

impl Mcp {
    /// Send a request without waiting for its response.
    ///
    /// `subscriptions/listen` answers only when the stream ends, so a test
    /// that reads notifications cannot wait for the response first.
    fn send(&mut self, method: &str, mut params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": PRIMARY,
            "io.modelcontextprotocol/clientInfo": { "name": "test-host", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stdin = self.stdin.as_mut().expect("the pipe is open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        id
    }

    /// Discover the server, as a 2026-07-28 host does before anything else.
    ///
    /// The SDK answers the first request of a connection inline, before its
    /// service loop starts, so a connection whose first request is the
    /// long-lived `subscriptions/listen` would answer nothing else. A host
    /// discovers the surface first, which is also what this does.
    fn discover(&mut self) {
        assert!(
            self.primary("server/discover", json!({}))["result"]["capabilities"]["resources"]["subscribe"]
                == json!(true),
            "the server does not advertise resource subscriptions"
        );
    }

    /// Open a subscription that resumes from a cursor the host names.
    fn listen_from(&mut self, subscriptions: &Value, cursor: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "subscriptions/listen",
            "params": {
                "notifications": subscriptions,
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PRIMARY,
                    "io.modelcontextprotocol/clientInfo": { "name": "test-host", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {},
                    "dev.canvas-cli/cursor": cursor,
                },
            },
        });
        let stdin = self.stdin.as_mut().expect("the pipe is open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        id
    }

    /// Read one line, whatever it is.
    fn line(&mut self) -> Value {
        let mut buffer = String::new();
        let read = self.stdout.read_line(&mut buffer).unwrap();
        assert!(read > 0, "the server closed stdout");
        serde_json::from_str(&buffer).unwrap_or_else(|e| panic!("not JSON-RPC: {buffer:?} ({e})"))
    }

    /// Collect whatever the server has queued, keeping notifications.
    ///
    /// One round trip of a cheap request drains the pipe, so this always ends:
    /// it never blocks on a notification that may never come.
    fn drain(&mut self) {
        let id = self.send("tools/list", json!({}));
        loop {
            let message = self.line();
            if message["id"] == json!(id) {
                return;
            }
            self.pending.push(message);
        }
    }

    /// Wait until the server sends a notification of one method, and take it.
    fn wait_for(&mut self, method: &str) -> Value {
        for _ in 0..100 {
            self.drain();
            if let Some(at) = self
                .pending
                .iter()
                .position(|message| message["method"] == json!(method))
            {
                return self.pending.remove(at);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("the server never sent {method}: {:?}", self.pending);
    }

    /// Every resource URI the server has said is updated so far, once each.
    fn updated(&mut self) -> Vec<String> {
        self.drain();
        let mut uris: Vec<String> = self
            .pending
            .iter()
            .filter(|message| message["method"] == json!("notifications/resources/updated"))
            .map(|message| message["params"]["uri"].as_str().unwrap().to_owned())
            .collect();
        uris.sort();
        uris.dedup();
        uris
    }

    /// Wait until the server has named `want` distinct updated resources.
    fn updates(&mut self, want: usize) -> Vec<String> {
        let mut uris = Vec::new();
        for _ in 0..100 {
            uris = self.updated();
            if uris.len() >= want {
                return uris;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("only {uris:?} were invalidated, wanted {want}");
    }

    /// Send a request and read its response, skipping any notification.
    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stdin = self.stdin.as_mut().expect("the pipe is open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        loop {
            let mut buffer = String::new();
            let read = self.stdout.read_line(&mut buffer).unwrap();
            assert!(
                read > 0,
                "the server closed stdout while answering {method}"
            );
            let message: Value = match serde_json::from_str(&buffer) {
                Ok(message) => message,
                // Anything that is not a JSON-RPC line is not this protocol.
                Err(e) => panic!("not JSON-RPC: {buffer:?} ({e})"),
            };
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// A request on the primary revision, which carries its own `_meta`.
    fn primary(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": PRIMARY,
            "io.modelcontextprotocol/clientInfo": { "name": "test-host", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        self.request(method, &params)
    }

    /// A primary-revision request from a host that can show a form.
    fn eliciting(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": PRIMARY,
            "io.modelcontextprotocol/clientInfo": { "name": "test-host", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": { "elicitation": {} },
        });
        self.request(method, &params)
    }

    /// The second half of an MRTR round trip, under a new JSON-RPC id.
    fn retry(&mut self, name: &str, state: &Value, answer: &Value) -> Value {
        self.eliciting(
            "tools/call",
            json!({
                "name": name,
                "arguments": {},
                "requestState": state,
                "inputResponses": { "approval": answer },
            }),
        )
    }

    /// The `initialize` handshake of a legacy host.
    fn initialize(&mut self, version: &str) -> Value {
        self.request(
            "initialize",
            &json!({
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "test-host", "version": "0" },
            }),
        )
    }

    /// Close the pipe and wait for the exit code.
    fn stop(&mut self) -> Option<i32> {
        self.stdin.take();
        self.child.wait().unwrap().code()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn courses() -> Value {
    json!([
        {"id": 1, "course_code": "CHEM", "name": "Chemistry"},
        {"id": 2, "course_code": "PHYS", "name": "Physics"},
    ])
}

fn planner_items() -> Value {
    json!([
        {
            "plannable_type": "assignment",
            "plannable_id": 500,
            "course_id": 1,
            "plannable_date": "2026-09-10T03:59:00Z",
            "plannable": {
                "id": 500,
                "title": "Problem Set 2",
                "due_at": "2026-09-10T03:59:00Z",
                "points_possible": 10,
            },
            "html_url": "https://canvas.test/courses/1/assignments/500",
            "submissions": {"submitted": false, "graded": false},
        },
    ])
}

async fn mount(server: &MockServer, url: &str, value: Value) {
    Mock::given(path(url))
        .respond_with(ResponseTemplate::new(200).set_body_json(value))
        .mount(server)
        .await;
}

/// A fixture whose cache is primed, so every later read is offline and equal.
async fn primed(server: &MockServer) -> Fixture {
    let f = Fixture::new(&server.uri());
    mount(server, "/api/v1/courses", courses()).await;
    mount(server, "/api/v1/planner/items", planner_items()).await;
    for endpoint in [
        "/api/v1/users/self/enrollments",
        "/api/v1/courses/1/assignments",
        "/api/v1/courses/2/assignments",
        "/api/v1/users/self/missing_submissions",
    ] {
        mount(server, endpoint, json!([])).await;
    }
    for course in [1, 2] {
        mount(
            server,
            &format!("/api/v1/courses/{course}/grading_periods"),
            json!({"grading_periods": []}),
        )
        .await;
    }
    Mock::given(method("GET"))
        .and(path("/api/v1/announcements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
    f.cli(&["sync"], 0).await;
    f
}

/// A primed fixture that can also accept one text submission.
///
/// Assignment 500 of course 1 is the one the planner dataset already names,
/// so the same fixture answers the reads and the write.
async fn submittable(server: &MockServer) -> (Fixture, std::path::PathBuf) {
    let f = primed(server).await;
    Mock::given(path("/api/v1/courses/1/assignments/500"))
        .and(query_param("include[]", "can_submit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 500, "course_id": 1, "name": "Problem Set 2",
            "submission_types": ["online_text_entry"],
            "can_submit": true, "due_at": "2026-09-10T03:59:00Z",
            "submission": {"attempt": 0}
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/courses/1/assignments/500/submissions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 77, "attempt": 1, "submitted_at": "2026-09-09T17:05:12Z",
            "body": "<p>hello</p>"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/500/submissions/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "attempt": 1,
            "submission_history": [{
                "id": 77, "attempt": 1, "submitted_at": "2026-09-09T17:05:12Z",
                "body": "<p>hello</p>", "attachments": []
            }]
        })))
        .mount(server)
        .await;
    let input = f.dir.path().join("answer.txt");
    std::fs::write(&input, b"hello").unwrap();
    (f, input)
}

/// Prepare one plan and return its `plan@1` result.
fn prepare(mcp: &mut Mcp, input: &std::path::Path) -> Value {
    let prepared = mcp.eliciting(
        "tools/call",
        json!({
            "name": "submission.prepare",
            "arguments": { "course": "1", "assignment": "500", "text": input },
        }),
    );
    let result = &prepared["result"];
    assert_eq!(result["isError"], false, "{prepared}");
    assert_eq!(result["structuredContent"]["schema"], "canvas-cli/plan@1");
    result["structuredContent"]["result"]["plan"].clone()
}

/// How many POSTs the mock server has seen.
async fn posts(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method == wiremock::http::Method::POST)
        .count()
}

/// Ask for approval of one plan and return the opaque `requestState`.
fn ask(mcp: &mut Mcp, plan: &Value) -> Value {
    let asked = mcp.eliciting(
        "tools/call",
        json!({
            "name": "submission.execute",
            "arguments": { "plan_id": plan["plan_id"] },
        }),
    );
    let result = &asked["result"];
    assert_eq!(result["resultType"], "input_required", "{asked}");
    let request = &result["inputRequests"]["approval"];
    assert_eq!(request["method"], "elicitation/create");
    // The message names the exact bytes the approval binds.
    let message = request["params"]["message"].as_str().unwrap();
    assert!(
        message.contains(plan["plan_sha256"].as_str().unwrap()),
        "{message}"
    );
    result["requestState"].clone()
}

/// Preparing freezes a plan and sends nothing, and a refused approval keeps
/// it that way.
#[tokio::test]
async fn a_declined_or_cancelled_approval_dispatches_nothing() {
    let server = MockServer::start().await;
    let (f, input) = submittable(&server).await;
    let mut mcp = f.mcp(&[]);

    let plan = prepare(&mut mcp, &input);
    assert_eq!(plan["state"], "prepared");
    assert_eq!(plan["consumer"], "mcp:test-host");
    assert_eq!(posts(&server).await, 0, "prepare sent something");

    let state = ask(&mut mcp, &plan);
    assert_eq!(posts(&server).await, 0, "asking sent something");
    let declined = mcp.retry(
        "submission.execute",
        &state,
        &json!({ "action": "decline" }),
    );
    let envelope = &declined["result"]["structuredContent"];
    assert_eq!(envelope["exit"], 11, "{envelope}");
    assert_eq!(envelope["result"]["code"], "cancelled");
    assert_eq!(posts(&server).await, 0, "a decline sent something");

    // A declined plan is spent: the same handle can never be used again.
    let again = mcp.retry(
        "submission.execute",
        &state,
        &json!({ "action": "accept", "content": { "handle": handle_of(&state) } }),
    );
    let envelope = &again["result"]["structuredContent"];
    assert_eq!(envelope["outcome"], "refused", "{envelope}");
    assert_eq!(envelope["exit"], 8);
    assert_eq!(posts(&server).await, 0, "a spent handle sent something");

    // Cancelling ends the same way, through the other action.
    let plan = prepare(&mut mcp, &input);
    let state = ask(&mut mcp, &plan);
    let cancelled = mcp.retry("submission.execute", &state, &json!({ "action": "cancel" }));
    assert_eq!(
        cancelled["result"]["structuredContent"]["exit"], 11,
        "{cancelled}"
    );
    assert_eq!(posts(&server).await, 0, "a cancel sent something");
    mcp.stop();
}

/// An accepted approval submits once, and a second execute replays it.
#[tokio::test]
async fn an_accepted_approval_submits_once_and_replays_after_that() {
    let server = MockServer::start().await;
    let (f, input) = submittable(&server).await;
    let mut mcp = f.mcp(&[]);

    let plan = prepare(&mut mcp, &input);
    let state = ask(&mut mcp, &plan);
    let accepted = mcp.retry(
        "submission.execute",
        &state,
        &json!({ "action": "accept", "content": { "handle": handle_of(&state) } }),
    );
    let envelope = &accepted["result"]["structuredContent"];
    assert_eq!(envelope["schema"], "canvas-cli/submit@1", "{envelope}");
    assert_eq!(envelope["outcome"], "ok");
    assert_eq!(envelope["exit"], 0);
    assert_eq!(envelope["result"]["state"], "submitted");
    assert_eq!(envelope["result"]["replayed"], false);
    let journal = envelope["result"]["journal_id"].clone();
    assert_eq!(posts(&server).await, 1, "the accepted plan sent once");

    // The same plan returns the same journal and posts nothing more, with the
    // journal's own outcome and exit (SPEC §19 item 17).
    let replayed = mcp.eliciting(
        "tools/call",
        json!({
            "name": "submission.execute",
            "arguments": { "plan_id": plan["plan_id"] },
        }),
    );
    let envelope = &replayed["result"]["structuredContent"];
    assert_eq!(envelope["schema"], "canvas-cli/submit@1", "{envelope}");
    assert_eq!(envelope["outcome"], "ok");
    assert_eq!(envelope["exit"], 0);
    assert_eq!(envelope["result"]["journal_id"], journal);
    assert_eq!(envelope["result"]["replayed"], true);
    assert_eq!(posts(&server).await, 1, "the replay sent something");

    // The approval audit names the channel and the host that asked.
    let receipt = f
        .cli(&["receipts", "show", journal.as_str().unwrap()], 0)
        .await;
    let approval = &receipt["result"]["journal"]["approval"];
    assert_eq!(approval["channel"], "elicitation", "{receipt}");
    assert_eq!(approval["consumer"], "mcp:test-host");
    mcp.stop();
}

/// The handle inside a `requestState`.
fn handle_of(state: &Value) -> String {
    let state: Value = serde_json::from_str(state.as_str().expect("an opaque string")).unwrap();
    state["handle"].as_str().unwrap().to_owned()
}

/// A host that cannot ask a person gets a refusal, and nothing is dispatched.
#[tokio::test]
async fn a_host_without_elicitation_is_refused_and_nothing_is_dispatched() {
    let server = MockServer::start().await;
    let (f, input) = submittable(&server).await;
    let mut mcp = f.mcp(&[]);

    // `primary` declares no capabilities at all, which is such a host.
    let prepared = mcp.primary(
        "tools/call",
        json!({
            "name": "submission.prepare",
            "arguments": { "course": "1", "assignment": "500", "text": input },
        }),
    );
    let plan = &prepared["result"]["structuredContent"]["result"]["plan"];
    let refused = mcp.primary(
        "tools/call",
        json!({
            "name": "submission.execute",
            "arguments": { "plan_id": plan["plan_id"] },
        }),
    );
    let result = &refused["result"];
    // A domain refusal, not a protocol error and not an `input_required`.
    assert_eq!(result["isError"], true, "{refused}");
    let envelope = &result["structuredContent"];
    assert_eq!(envelope["outcome"], "refused");
    assert_eq!(envelope["exit"], 8);
    let details = &envelope["result"]["details"];
    assert_eq!(details["reason"], "approval_required");
    // The handle travels, so the approval can be recorded another way.
    assert!(details["handle"].as_str().is_some_and(|h| !h.is_empty()));
    assert_eq!(details["plan_id"], plan["plan_id"]);
    assert_eq!(posts(&server).await, 0, "the refusal sent something");

    // Nothing was uploaded either: the only requests are the reads.
    for request in server.received_requests().await.unwrap() {
        assert_eq!(
            request.method,
            wiremock::http::Method::GET,
            "{} reached Canvas",
            request.url
        );
    }
    mcp.stop();
}

/// A tool argument can never assert an approval.
#[tokio::test]
async fn an_approval_cannot_be_asserted_by_an_argument() {
    let server = MockServer::start().await;
    let (f, input) = submittable(&server).await;
    let mut mcp = f.mcp(&[]);

    let plan = prepare(&mut mcp, &input);
    for extra in [
        json!({ "plan_id": plan["plan_id"], "handle": "anything" }),
        json!({ "plan_id": plan["plan_id"], "approved": true }),
        json!({ "plan_id": plan["plan_id"], "yes": true }),
    ] {
        let refused = mcp.eliciting(
            "tools/call",
            json!({ "name": "submission.execute", "arguments": extra }),
        );
        // An argument the schema does not name never reaches the command.
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
    }
    // A forged request state cannot approve either: the handle it names was
    // never issued, so `approve` refuses it.
    let forged = mcp.retry(
        "submission.execute",
        &json!(
            serde_json::to_string(&json!({
                "plan_id": plan["plan_id"],
                "handle": "00000000000000000000000000000000",
            }))
            .unwrap()
        ),
        &json!({ "action": "accept", "content": { "handle": "00000000000000000000000000000000" } }),
    );
    let envelope = &forged["result"]["structuredContent"];
    assert_eq!(envelope["outcome"], "refused", "{envelope}");
    assert_eq!(envelope["exit"], 8);
    assert_eq!(posts(&server).await, 0, "a forged state sent something");

    // Only `submission.execute` ever asks for a decision, so a retry that
    // names another tool records nothing: that call never asked for one.
    let state = ask(&mut mcp, &plan);
    for other in ["todo.list", "receipts.acknowledge", "sync.run"] {
        let misrouted = mcp.retry(
            other,
            &state,
            &json!({ "action": "accept", "content": { "handle": handle_of(&state) } }),
        );
        assert_eq!(misrouted["error"]["code"], -32602, "{misrouted}");
        assert_eq!(posts(&server).await, 0, "{other} sent something");
    }
    // The plan is untouched, so the tool that did ask can still be answered.
    let accepted = mcp.retry(
        "submission.execute",
        &state,
        &json!({ "action": "accept", "content": { "handle": handle_of(&state) } }),
    );
    assert_eq!(
        accepted["result"]["structuredContent"]["outcome"], "ok",
        "{accepted}"
    );
    assert_eq!(posts(&server).await, 1);
    mcp.stop();
}

/// Only the two revisions this server implements are accepted.
#[tokio::test]
async fn both_revisions_handshake_and_an_unknown_one_is_refused() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());

    // The legacy revision negotiates through `initialize`.
    let mut mcp = f.mcp(&[]);
    let answer = mcp.initialize(LEGACY);
    assert_eq!(answer["result"]["protocolVersion"], LEGACY);
    assert_eq!(answer["result"]["serverInfo"]["name"], "canvas-cli");
    mcp.stop();

    // The primary revision needs no handshake at all: `server/discover`
    // reports what the server speaks, and the result is never shared.
    let mut mcp = f.mcp(&[]);
    let discovered = mcp.primary("server/discover", json!({}));
    let versions = discovered["result"]["supportedVersions"]
        .as_array()
        .unwrap();
    assert_eq!(versions, &[json!(PRIMARY), json!(LEGACY)]);
    assert_eq!(discovered["result"]["cacheScope"], "private");
    assert!(discovered["result"]["capabilities"]["tools"].is_object());
    assert!(discovered["result"]["capabilities"]["resources"].is_object());
    mcp.stop();

    // Anything else fails the handshake, and never silently downgrades.
    for unknown in ["2024-11-05", "2025-03-26", "2099-01-01"] {
        let mut mcp = f.mcp(&[]);
        let refused = mcp.initialize(unknown);
        assert!(
            refused["result"].is_null(),
            "{unknown} was accepted: {refused}"
        );
        assert_eq!(refused["error"]["code"], -32022, "{unknown}");
        assert_eq!(refused["error"]["data"]["requested"], unknown);
        mcp.stop();
    }

    // A per-request version the server does not speak is refused too.
    let mut mcp = f.mcp(&[]);
    let refused = mcp.request(
        "tools/list",
        &json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2099-01-01",
                "io.modelcontextprotocol/clientInfo": { "name": "t", "version": "0" },
                "io.modelcontextprotocol/clientCapabilities": {},
            }
        }),
    );
    assert_eq!(refused["error"]["code"], -32022);
    mcp.stop();
}

/// The catalog on the wire is the catalog REPORT §3.2 names, and no more.
#[tokio::test]
async fn the_tool_list_is_the_report_catalog_with_effect_annotations() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    let mut mcp = f.mcp(&[]);

    let listed = mcp.primary("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, CATALOG);
    assert_eq!(listed["result"]["cacheScope"], "private");

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let annotations = &tool["annotations"];
        assert!(annotations["title"].is_string(), "{name} has no title");
        assert_eq!(
            annotations["destructiveHint"], false,
            "{name} is advertised as destructive"
        );
        // A domain failure keeps the envelope, so both shapes are described.
        let branches = tool["outputSchema"]["oneOf"].as_array().unwrap();
        assert_eq!(branches.len(), 2, "{name}");
        assert_eq!(
            branches[1]["properties"]["schema"]["const"], "canvas-cli/error@1",
            "{name}"
        );
        // Nothing in the catalog takes an argument that widens its reach.
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for forbidden in ["yes", "force", "dest", "out", "reveal", "token", "host"] {
            assert!(
                !properties.contains_key(forbidden),
                "{name} exposes {forbidden}"
            );
        }
    }

    let reads = ["courses.list", "todo.list", "open.url", "download.plan"];
    for name in reads {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
    }
    for name in ["sync.run", "download.run", "receipts.acknowledge"] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], false, "{name}");
    }
    mcp.stop();
}

/// A tool result is the CLI's `--json` document, byte for byte.
#[tokio::test]
async fn a_tool_result_is_the_same_envelope_the_cli_prints() {
    let server = MockServer::start().await;
    let f = primed(&server).await;

    // Both sides read the primed cache, so nothing but the transport differs.
    let expected = f.cli(&["--offline", "todo"], 0).await;
    let mut mcp = f.mcp(&["--offline"]);
    let answer = mcp.primary(
        "tools/call",
        json!({ "name": "todo.list", "arguments": {} }),
    );
    let result = &answer["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"], expected);
    // The text block carries the same document, for a host that shows text.
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed, expected);
    // A private result, with a budget bounded by the cache it read.
    let meta = &result["_meta"];
    assert_eq!(meta["dev.canvas-cli/cacheScope"], "private");
    assert!(meta["dev.canvas-cli/ttlMs"].is_u64(), "{meta}");
    assert!(!text.contains(TOKEN));

    // A local-only tool answers with the same envelope as well.
    let receipts = mcp.primary(
        "tools/call",
        json!({ "name": "receipts.list", "arguments": {} }),
    );
    assert_eq!(
        receipts["result"]["structuredContent"],
        f.cli(&["--offline", "receipts", "list"], 0).await
    );
    mcp.stop();
}

/// Every tool in the catalog, with the `canvas` invocation behind it.
///
/// The tool and the command must report the same envelope for the same
/// arguments, whatever that envelope says: an answer, a refusal, or the usage
/// error a command that needs the network gives while `--offline`.
///
/// `submission.execute` is the one tool with no command behind it. It names a
/// stored plan rather than a course, and the plan flow it drives has its own
/// tests above.
const EQUIVALENTS: &[(&str, &str, &[&str])] = &[
    ("courses.list", r"{}", &["courses"]),
    ("course.get", r#"{"course":"1"}"#, &["course", "1"]),
    ("todo.list", r"{}", &["todo"]),
    (
        "assignments.list",
        r#"{"course":"1"}"#,
        &["assignments", "1"],
    ),
    (
        "assignment.get",
        r#"{"course":"1","assignment":"500"}"#,
        &["assignment", "1", "500"],
    ),
    ("grades.get", r#"{"course":"1"}"#, &["grades", "1"]),
    ("files.list", r#"{"course":"1"}"#, &["files", "1"]),
    ("modules.list", r#"{"course":"1"}"#, &["modules", "1"]),
    ("pages.list", r#"{"course":"1"}"#, &["pages", "1"]),
    (
        "page.get",
        r#"{"course":"1","page":"course-overview"}"#,
        &["page", "1", "course-overview"],
    ),
    ("syllabus.get", r#"{"course":"1"}"#, &["syllabus", "1"]),
    ("announcements.list", r"{}", &["announcements"]),
    (
        "announcement.get",
        r#"{"course":"1","id":"9001"}"#,
        &["announcement", "1", "9001"],
    ),
    (
        "discussions.list",
        r#"{"course":"1"}"#,
        &["discussions", "1"],
    ),
    (
        "discussion.get",
        r#"{"course":"1","discussion":"55"}"#,
        &["discussion", "1", "55"],
    ),
    ("inbox.list", r"{}", &["inbox"]),
    ("inbox.get", r#"{"id":"700"}"#, &["inbox", "show", "700"]),
    ("inbox.unread_count", r"{}", &["inbox", "unread-count"]),
    ("calendar.list", r"{}", &["calendar"]),
    (
        "submission.get",
        r#"{"course":"1","assignment":"500"}"#,
        &["submission", "1", "500"],
    ),
    ("receipts.list", r"{}", &["receipts", "list"]),
    (
        "receipts.show",
        r#"{"id":"no-such-receipt"}"#,
        &["receipts", "show", "no-such-receipt"],
    ),
    ("sync.run", r"{}", &["sync"]),
    (
        "download.plan",
        r#"{"course":"1"}"#,
        &["download", "1", "--dry-run"],
    ),
    ("download.run", r#"{"course":"1"}"#, &["download", "1"]),
    (
        "submission.prepare",
        r#"{"course":"1","assignment":"500","text":"answer.txt"}"#,
        &["submit", "1", "500", "--text", "answer.txt"],
    ),
    (
        "submission.reconcile",
        r#"{"journal_id":"no-such-journal"}"#,
        &["submission", "reconcile", "no-such-journal"],
    ),
    (
        "receipts.acknowledge",
        r#"{"journal_id":"no-such-journal"}"#,
        &["receipts", "acknowledge", "no-such-journal"],
    ),
    ("open.url", r#"{"target":"CHEM"}"#, &["open", "CHEM"]),
];

/// One implementation per command: every tool returns the CLI's envelope.
#[tokio::test]
async fn every_tool_returns_the_envelope_the_cli_prints() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);

    let covered: Vec<&str> = EQUIVALENTS.iter().map(|(tool, ..)| *tool).collect();
    let mut expected: Vec<&str> = CATALOG.to_vec();
    expected.retain(|name| *name != "submission.execute");
    assert_eq!(covered, expected, "a tool has no command behind it");

    for (tool, arguments, args) in EQUIVALENTS {
        let arguments: Value = serde_json::from_str(arguments).expect("arguments");
        let mut cli = vec!["--offline"];
        cli.extend_from_slice(args);
        let (want, code) = f.cli_any(&cli).await;
        let answer = mcp.primary(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        let result = &answer["result"];
        assert_eq!(
            result["structuredContent"],
            want,
            "{tool} and `canvas {}` disagree",
            args.join(" ")
        );
        // Both sides answered with a §7 envelope, not with nothing.
        let schema = want["schema"].as_str().unwrap_or_default();
        assert!(schema.starts_with("canvas-cli/"), "{tool} returned {want}");
        assert!(want["outcome"].is_string(), "{tool} returned {want}");
        // A domain failure is marked, and a success is not. Every case in this
        // table is an answer, a refusal, or a usage error, so the exit alone
        // says which; `partial` (12) is the one outcome that carries a
        // non-zero exit and is still not an error result.
        assert!(
            code != 12,
            "{tool} returned a partial result: assert its `isError` explicitly"
        );
        assert_eq!(result["isError"], json!(code != 0), "{tool} exit {code}");
        // The text block is the same document, for a host that shows text.
        let text = result["content"][0]["text"].as_str().expect("text content");
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("text is the document"),
            want,
            "{tool}"
        );
        assert!(!text.contains(TOKEN), "{tool} leaked the token");
    }
    mcp.stop();
}

/// The M8-a read surface for course 1, warmed into the cache.
///
/// `EQUIVALENTS` runs `--offline` against a cache that holds none of these
/// reads, so the eight M8-a tools and their commands meet there only as the
/// same refusal — an envelope both sides reach before any operand past the
/// first is looked at. This fixture answers the reads instead, so the
/// comparison below covers the arguments themselves.
async fn primed_reads(server: &MockServer) -> Fixture {
    let f = primed(server).await;
    mount(
        server,
        "/api/v1/courses/1",
        json!({
            "id": 1, "course_code": "CHEM", "name": "Chemistry",
            "syllabus_body": "<p>Late work loses 10% a day.</p>",
            "updated_at": "2026-09-01T14:00:00Z"
        }),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/pages",
        json!([
            {
                "page_id": 301, "url": "course-overview", "title": "Course overview",
                "updated_at": "2026-09-01T14:00:00Z", "published": true,
                "front_page": true, "locked_for_user": false
            },
            {
                "page_id": 302, "url": "draft-notes", "title": "Draft notes",
                "updated_at": "2026-09-02T14:00:00Z", "published": false,
                "front_page": false, "locked_for_user": false
            },
        ]),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/pages/course-overview",
        json!({
            "page_id": 301, "url": "course-overview", "title": "Course overview",
            "updated_at": "2026-09-01T14:00:00Z", "published": true,
            "front_page": true, "locked_for_user": false,
            "body": "<p>Read the handbook.</p>"
        }),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/discussion_topics",
        json!([discussion_topic(55), read_topic(56)]),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/discussion_topics/55",
        discussion_topic(55),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/discussion_topics/56",
        read_topic(56),
    )
    .await;
    mount(
        server,
        "/api/v1/courses/1/discussion_topics/55/entries",
        json!([{
            "id": 900, "parent_id": null, "user_id": 31, "user_name": "Alex Kim",
            "message": "<p>Reply 1.</p>", "created_at": "2026-09-02T09:00:00Z",
            "read_state": "unread", "has_more_replies": false, "recent_replies": []
        }]),
    )
    .await;
    mount(server, "/api/v1/conversations", json!([conversation(700)])).await;
    mount(server, "/api/v1/conversations/700", conversation_detail()).await;
    mount(
        server,
        "/api/v1/conversations/unread_count",
        json!({"unread_count": "1"}),
    )
    .await;

    // Warm every dataset the comparison reads, one command each. `--scope`
    // keys its own dataset row, so the scope the tool asks for is warmed too.
    for args in [
        &["pages", "1"][..],
        &["page", "1", "course-overview"],
        &["syllabus", "1"],
        &["discussions", "1"],
        &["discussion", "1", "55", "--replies"],
        &["inbox"],
        &["inbox", "--scope", "unread"],
        &["inbox", "show", "700"],
        &["inbox", "unread-count"],
    ] {
        f.cli(args, 0).await;
    }
    f
}

fn discussion_topic(id: i64) -> Value {
    json!({
        "id": id,
        "title": format!("Topic {id}"),
        "message": "<p>What surprised you?</p>",
        "posted_at": "2026-09-01T14:00:00Z",
        "last_reply_at": "2026-09-08T10:00:00Z",
        "discussion_type": "threaded",
        "user_name": "Dr. Reed",
        "read_state": "unread",
        "unread_count": 1,
        "discussion_subentry_count": 1,
        "published": true,
        "locked": false,
        "locked_for_user": false,
        "pinned": false,
        "require_initial_post": false,
        "user_can_see_posts": true,
        "is_announcement": false,
        "subscribed": true,
        "context_code": "course_1"
    })
}

/// A topic this identity has already read, so `--unread` has one to drop.
fn read_topic(id: i64) -> Value {
    let mut row = discussion_topic(id);
    row["read_state"] = json!("read");
    row["unread_count"] = json!(0);
    row
}

fn conversation(id: i64) -> Value {
    json!({
        "id": id,
        "subject": format!("Conversation {id}"),
        "workflow_state": "unread",
        "last_message": "See you then.",
        "last_message_at": "2026-09-09T12:00:00Z",
        "message_count": 1,
        "subscribed": true,
        "private": true,
        "starred": false,
        "context_name": "CHEM",
        "participants": [{"id": 123, "name": "You"}, {"id": 31, "name": "Alex Kim"}]
    })
}

fn conversation_detail() -> Value {
    let mut row = conversation(700);
    row["messages"] = json!([{
        "id": 9001, "author_id": 31, "created_at": "2026-09-09T12:00:00Z",
        "body": "Can we meet before the lab?", "generated": false,
        "attachments": [{"id": 42, "display_name": "notes.pdf", "size": 20480}]
    }]);
    row
}

/// The eight M8-a reads, each with every argument it takes.
///
/// `EQUIVALENTS` names them with their operands only. These rows add the
/// flags — `unpublished`, `unread`, `replies`, `page`, `scope` — so a tool
/// that dropped one, or that read its operands in the wrong order, cannot
/// still answer with the command's envelope.
const M8A_READS: &[(&str, &str, &[&str])] = &[
    (
        "pages.list",
        r#"{"course":"1","unpublished":true}"#,
        &["pages", "1", "--unpublished"],
    ),
    (
        "page.get",
        r#"{"course":"1","page":"course-overview"}"#,
        &["page", "1", "course-overview"],
    ),
    ("syllabus.get", r#"{"course":"1"}"#, &["syllabus", "1"]),
    (
        "discussions.list",
        r#"{"course":"1","unread":true}"#,
        &["discussions", "1", "--unread"],
    ),
    (
        "discussion.get",
        r#"{"course":"1","discussion":"55","replies":true}"#,
        &["discussion", "1", "55", "--replies"],
    ),
    // A window past the end: `page` must reach the command, or the answer
    // carries the first page of replies instead of an empty one.
    (
        "discussion.get",
        r#"{"course":"1","discussion":"55","replies":true,"page":2}"#,
        &["discussion", "1", "55", "--replies", "--page", "2"],
    ),
    (
        "inbox.list",
        r#"{"scope":"unread"}"#,
        &["inbox", "--scope", "unread"],
    ),
    ("inbox.get", r#"{"id":"700"}"#, &["inbox", "show", "700"]),
    ("inbox.unread_count", r"{}", &["inbox", "unread-count"]),
];

/// The M8-a reads answer, and the answer is the CLI's, argument for argument.
#[tokio::test]
async fn the_m8a_read_tools_answer_with_the_envelope_their_command_prints() {
    let server = MockServer::start().await;
    let f = primed_reads(&server).await;
    let mut mcp = f.mcp(&["--offline"]);

    // Every M8-a tool is exercised here, not a subset of them.
    let covered: BTreeSet<&str> = M8A_READS.iter().map(|(tool, ..)| *tool).collect();
    let expected: BTreeSet<&str> = [
        "pages.list",
        "page.get",
        "syllabus.get",
        "discussions.list",
        "discussion.get",
        "inbox.list",
        "inbox.get",
        "inbox.unread_count",
    ]
    .into_iter()
    .collect();
    assert_eq!(covered, expected);

    for (tool, arguments, args) in M8A_READS {
        let arguments: Value = serde_json::from_str(arguments).expect("arguments");
        let mut cli = vec!["--offline"];
        cli.extend_from_slice(args);
        let want = f.cli(&cli, 0).await;
        // The cache answers, so this is a result and not the refusal
        // `EQUIVALENTS` compares.
        assert_eq!(want["outcome"], "ok", "{tool} did not read the cache");
        assert!(
            want["schema"].as_str().unwrap_or_default() != "canvas-cli/error@1",
            "{tool} answered with an error branch"
        );
        let answer = mcp.primary(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        let result = &answer["result"];
        assert_eq!(result["isError"], json!(false), "{tool}");
        assert_eq!(
            result["structuredContent"],
            want,
            "{tool} and `canvas {}` disagree",
            args.join(" ")
        );
        let text = result["content"][0]["text"].as_str().expect("text content");
        assert!(!text.contains(TOKEN), "{tool} leaked the token");
    }

    // The reads are reads: the whole fixture saw nothing but `GET` (§16).
    for request in server.received_requests().await.expect("the request log") {
        assert_eq!(request.method.as_str(), "GET", "{}", request.url);
    }
    mcp.stop();
}

/// A domain failure is a result that keeps `outcome` and `exit`.
#[tokio::test]
async fn a_domain_failure_keeps_the_envelope_and_a_protocol_failure_does_not() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);

    // Whatever the CLI reports for the same arguments, the tool reports.
    for (tool, arguments, args) in [
        (
            "course.get",
            json!({ "course": "NOPE" }),
            vec!["--offline", "course", "NOPE"],
        ),
        (
            "receipts.show",
            json!({ "id": "no-such-receipt" }),
            vec!["--offline", "receipts", "show", "no-such-receipt"],
        ),
    ] {
        let (expected, exit) = f.cli_any(&args).await;
        assert!(exit > 0, "{tool} was expected to fail: {expected}");
        let answer = mcp.primary(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        let result = &answer["result"];
        assert!(
            answer["error"].is_null(),
            "{tool}: a domain failure is not an MCP error: {answer}"
        );
        assert_eq!(result["isError"], true, "{tool}");
        assert_eq!(result["structuredContent"], expected, "{tool}");
        assert_eq!(result["structuredContent"]["exit"], exit, "{tool}");
        // Nothing is resolved, so the host may not cache the answer.
        assert_eq!(result["_meta"]["dev.canvas-cli/ttlMs"], 0, "{tool}");
    }

    // An unknown tool never ran: that is a protocol error.
    let unknown = mcp.primary(
        "tools/call",
        json!({ "name": "auth.token", "arguments": { "reveal": true } }),
    );
    assert_eq!(unknown["error"]["code"], -32601);
    assert!(unknown["result"].is_null());

    // So is an argument the tool's schema does not admit.
    for (name, arguments) in [
        ("download.run", json!({ "force": true })),
        ("download.run", json!({ "dest": "/tmp/anywhere" })),
        ("calendar.list", json!({ "ics": "-" })),
        ("courses.list", json!({ "yes": true })),
        (
            "assignments.list",
            json!({ "course": "1", "bucket": "any" }),
        ),
    ] {
        let refused = mcp.primary(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert_eq!(
            refused["error"]["code"], -32602,
            "{name} accepted {refused}"
        );
    }
    mcp.stop();
}

/// `open.url` resolves and returns the URL. It never launches anything.
#[tokio::test]
async fn open_url_resolves_without_launching() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);

    let answer = mcp.primary(
        "tools/call",
        json!({ "name": "open.url", "arguments": { "target": "CHEM" } }),
    );
    let result = &answer["result"]["structuredContent"]["result"];
    assert_eq!(result["target_kind"], "course");
    assert_eq!(result["url"], format!("{}/courses/1", server.uri()));
    assert_eq!(result["launched"], false);
    mcp.stop();
}

/// Resources belong to one identity generation and nothing else.
#[tokio::test]
async fn resources_are_private_to_the_identity_generation() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);

    let listed = mcp.primary("resources/list", json!({}));
    let resources = listed["result"]["resources"].as_array().unwrap();
    assert_eq!(listed["result"]["cacheScope"], "private");
    let doc = IdentityDocument::read(&f.identity_json()).unwrap();
    let prefix = format!("canvas://{}/{}/", doc.key.as_str(), doc.generation);
    for resource in resources {
        let uri = resource["uri"].as_str().unwrap();
        assert!(uri.starts_with(&prefix), "{uri} is not namespaced");
    }

    let templates = mcp.primary("resources/templates/list", json!({}));
    let templates = templates["result"]["resourceTemplates"].as_array().unwrap();
    assert!(
        templates
            .iter()
            .all(|t| t["uriTemplate"].as_str().unwrap().starts_with(&prefix)),
        "{templates:?}"
    );

    // The todo resource answers with the same envelope the tool returns.
    let read = mcp.primary("resources/read", json!({ "uri": format!("{prefix}todo") }));
    assert_eq!(read["result"]["cacheScope"], "private");
    let text = read["result"]["contents"][0]["text"].as_str().unwrap();
    let document: Value = serde_json::from_str(text).unwrap();
    assert_eq!(document, f.cli(&["--offline", "todo"], 0).await);

    // Another generation of the same identity addresses nothing.
    let other = format!(
        "canvas://{}/{}/todo",
        doc.key.as_str(),
        uuid::Uuid::new_v4()
    );
    let refused = mcp.primary("resources/read", json!({ "uri": other }));
    assert!(refused["result"].is_null(), "{refused}");
    assert!(
        refused["error"]["code"] == json!(-32002) || refused["error"]["code"] == json!(-32602),
        "{refused}"
    );

    // So does another identity.
    let foreign = "canvas://other.test-9-99999999/11111111-1111-4111-8111-111111111111/receipts";
    let refused = mcp.primary("resources/read", json!({ "uri": foreign }));
    assert!(refused["result"].is_null(), "{refused}");

    // The consumer bridge is a later package: the resource exists and says so.
    let context = mcp.primary(
        "resources/read",
        json!({ "uri": format!("{prefix}context/some-consumer") }),
    );
    let text = context["result"]["contents"][0]["text"].as_str().unwrap();
    let document: Value = serde_json::from_str(text).unwrap();
    assert_eq!(document["outcome"], "refused");
    assert_eq!(document["exit"], 8);
    // The reason travels in `reason`, as every §3.2 refusal does.
    assert_eq!(document["result"]["code"], "refused");
    assert_eq!(document["result"]["details"]["reason"], "not_attached");
    assert_eq!(context["result"]["ttlMs"], 0);
    mcp.stop();
}

/// The consumer name a `subscriptions/listen` stream keeps its cursor under.
const SUBSCRIBER: &str = "mcp:test-host";

/// The `canvas://` prefix of the bound identity generation.
fn prefix(f: &Fixture) -> String {
    let doc = IdentityDocument::read(&f.identity_json()).unwrap();
    format!("canvas://{}/{}/", doc.key.as_str(), doc.generation)
}

/// An event on a scope invalidates the resources that read that scope, and
/// nothing else (REPORT §3.6, item 7).
#[tokio::test]
async fn an_event_invalidates_the_matching_resource_and_no_other() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    let prefix = prefix(&f);
    let todo = format!("{prefix}todo");
    let course_1 = format!("{prefix}course/1/assignments");
    let course_2 = format!("{prefix}course/2/assignments");
    let receipts = format!("{prefix}receipts");
    let foreign = "canvas://other.test-9-99999999/11111111-1111-4111-8111-111111111111/todo";
    // A consumer context is served by a read, but no event names it and it is
    // another consumer's routing, so it is never subscribable (REPORT §3.2).
    let context = format!("{prefix}context/other-consumer");

    let id = mcp.send(
        "subscriptions/listen",
        json!({ "notifications": {
            "resourceSubscriptions": [&todo, &course_1, &course_2, foreign, &context],
        }}),
    );

    // The acknowledgment names the subscription and only the URIs this
    // instance can invalidate: another identity, and another consumer's
    // context, address nothing here.
    let ack = mcp.wait_for("notifications/subscriptions/acknowledged");
    assert_eq!(
        ack["params"]["notifications"]["resourceSubscriptions"],
        json!([&todo, &course_1, &course_2]),
        "{ack}"
    );
    assert_eq!(
        ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        json!(id),
        "{ack}"
    );

    // One course's assignment membership changed. The todo window reads the
    // assignment rows the planner names, so it changed with it.
    f.event("assignment.changed", "assignments", "course:1", "500");
    assert_eq!(mcp.updates(2), vec![course_1.clone(), todo.clone()]);
    let update = mcp
        .pending
        .iter()
        .find(|message| message["method"] == json!("notifications/resources/updated"))
        .cloned()
        .unwrap();
    assert_eq!(
        update["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        json!(id),
        "{update}"
    );

    // A journal transition invalidates the receipts, which this host did not
    // subscribe to, and an announcement has no resource at all. Neither can
    // name anything more, and the log position still moves past both.
    f.event(
        "submission.state",
        "submission_journal",
        "assignment:500",
        "journal-1",
    );
    f.event("announcement.new", "announcements", "courses", "9");
    f.wait_cursor(SUBSCRIBER, 3);
    let seen = mcp.updated();
    assert_eq!(seen, vec![course_1, todo], "{seen:?}");
    assert!(!seen.contains(&receipts), "{seen:?}");
    assert!(!seen.contains(&course_2), "{seen:?}");
    assert!(!seen.contains(&context), "{seen:?}");
    mcp.stop();
}

/// A cursor this log cannot replay asks the host to re-read what it holds,
/// exactly as `watch --since` emits `resync_required` (REPORT §3.2).
#[tokio::test]
async fn a_cursor_the_log_cannot_replay_asks_for_a_resync() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    // A position no row of this log ever issued: another identity's cursor.
    f.set_cursor(SUBSCRIBER, 99);
    f.event("missing.new", "missing", "self", "500");

    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    let prefix = prefix(&f);
    let todo = format!("{prefix}todo");
    let receipts = format!("{prefix}receipts");
    mcp.send(
        "subscriptions/listen",
        json!({ "notifications": { "resourceSubscriptions": [&todo, &receipts] }}),
    );
    mcp.wait_for("notifications/subscriptions/acknowledged");

    // Everything subscribed is invalidated once, including the resource no
    // event names: the gap is reported, never hidden, and the host rebuilds
    // its own baseline by reading again.
    assert_eq!(mcp.updates(2), vec![receipts, todo]);
    // The stream then follows the log from its high water mark, so the next
    // connection resumes instead of reporting the same gap again.
    f.wait_cursor(SUBSCRIBER, 1);
    mcp.stop();
}

/// A host may name the cursor it resumes from, and the replay starts there.
#[tokio::test]
async fn a_host_resumes_from_the_cursor_it_names() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    // Two events on two scopes, so the replay window is visible.
    assert_eq!(f.event("missing.new", "missing", "self", "500"), 1);
    assert_eq!(
        f.event(
            "submission.state",
            "submission_journal",
            "assignment:500",
            "journal-1"
        ),
        2
    );
    let prefix = prefix(&f);
    let todo = format!("{prefix}todo");
    let receipts = format!("{prefix}receipts");
    let subscriptions = json!({ "resourceSubscriptions": [&todo, &receipts] });

    // Without a cursor the stream replays the whole log: both rows, so both
    // resources.
    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    mcp.send(
        "subscriptions/listen",
        json!({ "notifications": subscriptions }),
    );
    mcp.wait_for("notifications/subscriptions/acknowledged");
    assert_eq!(mcp.updates(2), vec![receipts.clone(), todo.clone()]);
    mcp.stop();

    // The same log, resumed from cursor 1: only the second row is replayed.
    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    mcp.listen_from(&subscriptions, "1");
    mcp.wait_for("notifications/subscriptions/acknowledged");
    let seen = mcp.updates(1);
    assert_eq!(seen, vec![receipts], "cursor 1 replays only the second row");
    assert!(!seen.contains(&todo), "{seen:?}");
    mcp.stop();
}

/// A cursor the *host* names is the host's own position. When the log cannot
/// replay it the host is told to resync, but the consumer's stored position is
/// left alone: it is still replayable, and it still names rows this consumer
/// was never told about. `notify` draws the same line at `--since`.
#[tokio::test]
async fn a_named_cursor_that_asks_for_a_resync_leaves_the_stored_position_alone() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    assert_eq!(f.event("missing.new", "missing", "self", "500"), 1);
    assert_eq!(
        f.event(
            "submission.state",
            "submission_journal",
            "assignment:500",
            "journal-1"
        ),
        2
    );
    // The consumer has been told about the first row and no further.
    f.set_cursor(SUBSCRIBER, 1);
    let prefix = prefix(&f);
    let todo = format!("{prefix}todo");
    let receipts = format!("{prefix}receipts");

    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    // A position no row of this log ever issued, named by the host itself.
    mcp.listen_from(
        &json!({ "resourceSubscriptions": [&todo, &receipts] }),
        "99",
    );
    mcp.wait_for("notifications/subscriptions/acknowledged");
    // The gap is reported: everything subscribed is invalidated once.
    assert_eq!(mcp.updates(2), vec![receipts, todo]);
    // The stored position is untouched, so a reconnection that names nothing
    // still replays the second row instead of stepping over it.
    assert_eq!(
        f.cursor_of(SUBSCRIBER),
        1,
        "the stored position was replaced"
    );
    mcp.stop();
}

/// Without an identity there is nothing to serve (§14 exit 3).
#[tokio::test]
async fn the_server_refuses_to_start_without_an_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_canvas"));
    command
        .arg("mcp")
        .env("CANVAS_DATA_ROOT", dir.path().join("data"))
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env_remove("CANVAS_IDENTITY_KEY")
        .env_remove("CANVAS_HOST")
        .env_remove("CANVAS_TOKEN")
        .env_remove("CANVAS_PROFILE");
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty(), "the server answered nothing");
}

/// Replacing the identity stops the instance (§10).
#[tokio::test]
async fn replacing_the_identity_stops_the_instance() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);
    // The instance is bound and answering.
    assert!(mcp.primary("tools/list", json!({}))["result"]["tools"].is_array());

    // A new generation of the same identity: the old instance must let go.
    let replaced = IdentityDocument::new(server.uri(), 123, "2026-02-02T00:00:00Z");
    assert_ne!(replaced.generation, f.doc.generation);
    replaced.write(&f.identity_json()).unwrap();

    // The watchdog polls every two seconds.
    let mut exit = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Some(status) = mcp.child.try_wait().unwrap() {
            exit = status.code();
            break;
        }
    }
    assert_eq!(
        exit,
        Some(13),
        "§10: a replaced identity stops the instance with `identity changed`"
    );
}

/// A live subscription holds the identity store open for the whole stream.
/// That must not keep a replaced identity's instance alive: one instance
/// serves one generation, and it stops when that generation is gone (§10).
#[tokio::test]
async fn a_live_subscription_does_not_keep_a_replaced_identity_alive() {
    let server = MockServer::start().await;
    let f = primed(&server).await;
    let mut mcp = f.mcp(&["--offline"]);
    mcp.discover();
    let prefix = prefix(&f);
    let todo = format!("{prefix}todo");
    mcp.send(
        "subscriptions/listen",
        json!({ "notifications": { "resourceSubscriptions": [&todo] }}),
    );
    mcp.wait_for("notifications/subscriptions/acknowledged");
    // The stream is established and reading the log.
    f.event("missing.new", "missing", "self", "500");
    assert_eq!(mcp.updates(1), vec![todo]);

    let replaced = IdentityDocument::new(server.uri(), 123, "2026-02-02T00:00:00Z");
    assert_ne!(replaced.generation, f.doc.generation);
    replaced.write(&f.identity_json()).unwrap();

    let mut exit = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Some(status) = mcp.child.try_wait().unwrap() {
            exit = status.code();
            break;
        }
    }
    assert_eq!(
        exit,
        Some(13),
        "a subscribed host must not outlive the generation it is bound to"
    );
}

//! End-to-end `canvas mcp` tests over a real stdio pipe.
//!
//! The requests here are hand-written JSON-RPC, not SDK calls, because the
//! wire format is the contract a host sees: the tool catalog, both protocol
//! revisions, the envelope inside a tool result, and the resource namespace.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
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
    "announcements.list",
    "announcement.get",
    "calendar.list",
    "submission.get",
    "receipts.list",
    "receipts.show",
    "sync.run",
    "download.plan",
    "download.run",
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
        }
    }

    fn identity_json(&self) -> std::path::PathBuf {
        Paths::for_identity(self.dir.path().join("data"), &self.doc.key).identity_json()
    }
}

/// One `canvas mcp` process, spoken to as a host would.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
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
    assert_eq!(document["result"]["code"], "not_attached");
    assert_eq!(document["outcome"], "refused");
    assert_eq!(document["exit"], 8);
    assert_eq!(context["result"]["ttlMs"], 0);
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

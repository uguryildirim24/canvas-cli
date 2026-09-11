//! End-to-end `canvas mcp` tests over a real stdio pipe.
//!
//! The requests here are hand-written JSON-RPC, not SDK calls, because the
//! wire format is the contract a host sees: the one tool, both protocol
//! revisions, and what the tool answers.
//!
//! The surface is one tool (§21.2). There is no catalog to diff, no resource
//! namespace, and no subscription, so what is left to pin is that the one
//! tool is the only reachable name, that its answer really is the whole
//! `canvas` command line, and that the handshake still behaves.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::MockServer;

const TOKEN: &str = "mcp-secret-token";
const PRIMARY: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";

/// The only tool `canvas mcp` serves (§19 item 50).
const TOOL: &str = "getclitools";

/// Every name the server used to serve, and serves no longer.
///
/// The 21 writes M9 removed and the 22 reads M9-b removed. A host that held
/// either catalog must get `METHOD_NOT_FOUND`, never a quiet success: each of
/// these is a `canvas` command now, and `getclitools` says which.
const GONE: &[&str] = &[
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
    "discussion.reply.prepare",
    "discussion.reply.execute",
    "inbox.send.prepare",
    "inbox.send.execute",
    "inbox.reply.prepare",
    "inbox.reply.execute",
    "operation.status",
    "operation.reconcile",
    "receipts.acknowledge",
    "open.url",
    "context.attach",
    "context.here",
    "context.detach",
    "context.note",
    "context.follow",
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

    /// Run the CLI and return its raw stdout.
    ///
    /// `canvas schema --list` is raw output: it prints the registry listing
    /// with no §7 envelope, and `--json` is a usage error.
    async fn cli_raw(&self, args: &[&str]) -> String {
        let mut cmd = self.command();
        cmd.args(args);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
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
    /// Send a request and read its response.
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

    /// Call the one tool and return the text it answered with.
    fn reference(&mut self) -> String {
        let answer = self.primary("tools/call", json!({ "name": TOOL, "arguments": {} }));
        assert!(answer["error"].is_null(), "{answer}");
        let result = &answer["result"];
        assert_eq!(result["isError"], false, "{result}");
        let blocks = result["content"].as_array().expect("content blocks");
        assert_eq!(blocks.len(), 1, "one document, one block: {result}");
        assert_eq!(blocks[0]["type"], "text");
        blocks[0]["text"].as_str().expect("text").to_owned()
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

/// Only the two revisions this server implements are accepted, and the
/// surface behind them declares tools and nothing else.
#[tokio::test]
async fn both_revisions_handshake_and_an_unknown_one_is_refused() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());

    // The legacy revision negotiates through `initialize`.
    let mut mcp = f.mcp(&[]);
    let answer = mcp.initialize(LEGACY);
    assert_eq!(answer["result"]["protocolVersion"], LEGACY);
    assert_eq!(answer["result"]["serverInfo"]["name"], "canvas-cli");
    // §21.2: no resources, so no resource capability to advertise.
    assert!(answer["result"]["capabilities"]["tools"].is_object());
    assert!(answer["result"]["capabilities"]["resources"].is_null());
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
    assert!(
        discovered["result"]["capabilities"]["resources"].is_null(),
        "the server still advertises resources: {discovered}"
    );
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

/// `tools/list` is one tool, it is `getclitools`, and it is small.
///
/// The size is the point of this round. A host pays for the whole `tools/list`
/// response once per session, before the model has read anything, and the
/// 22-tool catalog cost 167 955 bytes because every tool inlined the §7
/// envelope twice. The ceiling here is measured, not guessed: the test prints
/// what it found, so a regression names its own number.
#[tokio::test]
async fn the_tool_list_is_one_tool_named_getclitools_and_it_is_small() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    let mut mcp = f.mcp(&[]);

    let listed = mcp.primary("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "one tool: {listed}");
    let tool = &tools[0];
    assert_eq!(tool["name"], TOOL);
    assert_eq!(listed["result"]["cacheScope"], "private");

    // It reads a document the binary already holds: nothing changes, the
    // answer is the same every time, and it reaches nothing.
    let annotations = &tool["annotations"];
    assert!(annotations["title"].is_string());
    assert_eq!(annotations["readOnlyHint"], true);
    assert_eq!(annotations["destructiveHint"], false);
    assert_eq!(annotations["idempotentHint"], true);
    assert_eq!(annotations["openWorldHint"], false);
    // No arguments, and no output schema: the answer is prose, not an
    // envelope, so there is nothing for a host validator to check.
    assert!(tool["outputSchema"].is_null(), "{tool}");
    assert_eq!(tool["inputSchema"]["type"], "object");
    assert!(
        tool["inputSchema"]["properties"]
            .as_object()
            .is_none_or(serde_json::Map::is_empty),
        "{tool}"
    );

    let bytes = serde_json::to_vec(&listed["result"]).unwrap().len();
    let tokens = bytes.div_ceil(4);
    println!("tools/list result: {bytes} bytes, ~{tokens} tokens");
    assert!(
        bytes < 2_000,
        "the whole tool list is {bytes} bytes (~{tokens} tokens)"
    );
    mcp.stop();
}

/// The one tool answers with the whole `canvas` command line.
///
/// Not "non-empty": the commands are named, the writes the old catalog could
/// never perform are named with the flags they take, and the schema listing
/// at the end is the one `canvas schema --list` prints on this same build.
#[tokio::test]
async fn getclitools_returns_the_canvas_command_reference() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    let mut mcp = f.mcp(&[]);
    let text = mcp.reference();

    // The preamble says what to do with it.
    assert!(
        text.contains("run `canvas <command> ...` yourself"),
        "{text}"
    );
    assert!(text.contains("Never pass `--yes`"));

    // Reads, writes, and the commands no tool ever covered.
    for command in [
        "### canvas todo",
        "### canvas courses",
        "### canvas assignments",
        "### canvas assignment",
        "### canvas grades",
        "### canvas inbox",
        "### canvas inbox send",
        "### canvas inbox reply",
        "### canvas discussion",
        "### canvas discussion reply",
        "### canvas submit",
        "### canvas submission reconcile",
        "### canvas receipts acknowledge",
        "### canvas download",
        "### canvas sync",
        "### canvas watch",
        "### canvas notify",
        "### canvas here",
        "### canvas note",
        "### canvas open",
        "### canvas bridge status",
        "### canvas doctor",
        "### canvas schema",
    ] {
        assert!(
            text.contains(command),
            "the reference never names {command}"
        );
    }

    // Operands and flags, not only names.
    assert!(text.contains("--text-file"), "no flags are described");
    assert!(text.contains("<COURSE>"), "no operands are described");

    // What each command returns, from the same registry `canvas schema` reads.
    for schema in [
        "`canvas-cli/todo@1` with `--json`",
        "`canvas-cli/submit@1` with `--json`",
        "`canvas-cli/operation@1` with `--json`",
        "`canvas-cli/watch@1` with `--jsonl`",
    ] {
        assert!(text.contains(schema), "the reference never says {schema}");
    }
    // A command with no §7 envelope says so rather than promising one.
    assert!(text.contains("Returns: raw output."));

    // The listing is the CLI's own, not a second description of it.
    let listing = f.cli_raw(&["schema", "--list"]).await;
    assert!(
        text.contains(&listing),
        "the reference does not carry `canvas schema --list`"
    );
    // The names `canvas schema` resolves include the writes an agent runs.
    for row in [
        "discussion reply\tcanvas-cli/operation@1",
        "inbox send\tcanvas-cli/operation@1",
        "inbox reply\tcanvas-cli/operation@1",
    ] {
        assert!(listing.contains(row), "`canvas schema --list` omits {row}");
    }

    // Building the reference reaches nothing: no session, no network.
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "the one tool made a request to Canvas"
    );
    mcp.stop();
}

/// `getclitools` is the only reachable name, and it never asks for a decision.
///
/// Every tool of both removed catalogs is a protocol error; so is a resource
/// read and a subscription, which this server no longer implements at all;
/// so is an argument the tool's schema does not admit; and so is a
/// `requestState`, the second half of an approval round trip this server has
/// not served since M9.
#[tokio::test]
async fn the_one_tool_is_the_only_name_and_never_asks_for_an_approval() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    let mut mcp = f.mcp(&["--offline"]);

    for name in GONE {
        let refused = mcp.primary(
            "tools/call",
            json!({ "name": name, "arguments": json!({}) }),
        );
        assert_eq!(
            refused["error"]["code"], -32601,
            "{name} is still reachable: {refused}"
        );
        assert!(refused["result"].is_null(), "{name}: {refused}");
    }

    // The resource namespace and the subscription are gone from the wire.
    // Nothing here implements them any more, so reading a resource and
    // opening a subscription are both unroutable, and the two list methods
    // the SDK answers by default have nothing to list.
    let read = mcp.primary(
        "resources/read",
        json!({ "uri": format!("canvas://{}/{}/todo", f.doc.key.as_str(), f.doc.generation) }),
    );
    assert_eq!(read["error"]["code"], -32601, "{read}");
    let subscription = mcp.primary(
        "subscriptions/listen",
        json!({ "notifications": { "resourcesListChanged": true } }),
    );
    assert_eq!(subscription["error"]["code"], -32601, "{subscription}");
    for (method, key) in [
        ("resources/list", "resources"),
        ("resources/templates/list", "resourceTemplates"),
    ] {
        let listed = mcp.primary(method, json!({}));
        assert_eq!(
            listed["result"][key].as_array().map(Vec::len),
            Some(0),
            "{method} still names something: {listed}"
        );
    }

    // An argument the schema does not name never runs the tool.
    for arguments in [
        json!({ "command": "todo" }),
        json!({ "yes": true }),
        json!({ "reveal": true }),
    ] {
        let refused = mcp.primary(
            "tools/call",
            json!({ "name": TOOL, "arguments": arguments }),
        );
        assert_eq!(
            refused["error"]["code"], -32602,
            "{TOOL} accepted {arguments}: {refused}"
        );
        assert!(refused["result"].is_null());
    }

    // A replayed approval state is refused, not applied to a read.
    let state = json!(
        serde_json::to_string(&json!({
            "plan_id": "plan-anything",
            "handle": "00000000000000000000000000000000",
        }))
        .unwrap()
    );
    let misrouted = mcp.primary(
        "tools/call",
        json!({
            "name": TOOL,
            "arguments": {},
            "requestState": state,
            "inputResponses": { "approval": { "action": "accept" } },
        }),
    );
    assert_eq!(
        misrouted["error"]["code"], -32602,
        "a request state was accepted: {misrouted}"
    );

    // Nothing on this surface reached Canvas at all.
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "the MCP surface made a request to Canvas"
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
    let f = Fixture::new(&server.uri());
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

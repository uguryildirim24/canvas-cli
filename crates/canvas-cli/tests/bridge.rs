//! End-to-end `canvas bridge` tests against a real broker process.
//!
//! The extension side is spoken here by hand — native-messaging framing on
//! the child's pipes — because the framing and the message vocabulary are the
//! contract Chrome sees. The consumer side is the shipped CLI, so what these
//! tests exercise is exactly what a person runs.
//!
//! Every message that crosses either pipe is recorded, so one test can assert
//! that no secret, cookie file, or token appears anywhere on the wire.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const TOKEN: &str = "bridge-secret-token";
const EXTENSION: &str = "abcdefghijklmnopabcdefghijklmnop";
const OTHER_EXTENSION: &str = "ponmlkjihgfedcbaponmlkjihgfedcba";
/// A short origin, deliberately.
///
/// A Unix socket path may hold about a hundred bytes, and a temporary
/// directory on macOS already spends sixty of them, so a realistic Canvas
/// host name would not leave room for `<key>.sock`. The origin is not what
/// these tests are about.
const ORIGIN: &str = "https://s.test";

/// A planted secret: it must never appear on either pipe.
const PAGE_SECRET: &str = "SECRET-CSRF-abcdef123456";

// ------------------------------------------------------------------ fixture

struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp");
        let doc = IdentityDocument::new(ORIGIN, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).expect("identity");
        let open = OpenIdentity::open(&paths, &doc).expect("open");
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
            .expect("credential");
        let fixture = Self { dir, doc };
        // The host serves only the extension this identity is configured for.
        fixture.run(
            &["config", "set", "bridge.extension_id", EXTENSION],
            Some(0),
        );
        fixture
    }

    fn data_root(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn endpoint(&self) -> PathBuf {
        self.data_root()
            .join("bridge")
            .join(format!("{}.sock", self.doc.key.as_str()))
    }

    fn owner_lock(&self) -> PathBuf {
        self.data_root()
            .join("bridge")
            .join(format!("{}.lock", self.doc.key.as_str()))
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_canvas"));
        command
            .env("CANVAS_DATA_ROOT", self.data_root())
            // `identity remove` resolves its own paths; both must agree.
            .env("CANVAS_DATA_DIR", self.data_root())
            .env("CANVAS_CONFIG_DIR", self.dir.path().join("config"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_BRIDGE_HOME", self.dir.path().join("home"))
            .env("CANVAS_NOW", "2026-09-10T16:04:40Z")
            .env("TZ", "America/New_York")
            .env("COLUMNS", "100")
            .env("CANVAS_TOKEN", TOKEN)
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        command
    }

    /// Run one CLI command and return its stdout, asserting the exit code.
    fn run(&self, args: &[&str], code: Option<i32>) -> String {
        let output = self.command().args(args).output().expect("run the CLI");
        if let Some(code) = code {
            assert_eq!(
                output.status.code(),
                Some(code),
                "`canvas {}`\nstdout={}\nstderr={}",
                args.join(" "),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Run a `--json` command and return its envelope with the exit code.
    fn json(&self, args: &[&str]) -> (Value, i32) {
        let output = self
            .command()
            .args(args)
            .args(["--json", "--color", "never"])
            .output()
            .expect("run the CLI");
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "`canvas {}` printed no envelope ({e})\nstdout={}\nstderr={}",
                args.join(" "),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().unwrap_or(-1))
    }

    /// Start `canvas bridge host` the way Chrome does.
    fn host(&self, extension_id: &str) -> Host {
        let mut command = self.command();
        command
            .arg(format!("chrome-extension://{extension_id}/"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn the host");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Host {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

// ------------------------------------------------------- the extension side

/// The companion, as far as the host can tell.
struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    /// Every message that crossed either pipe, as raw JSON.
    log: Arc<Mutex<Vec<String>>>,
}

impl Host {
    /// Write one native-messaging message: a native-endian length, then JSON.
    fn send(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).expect("encode");
        self.log
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&body).into_owned());
        let stdin = self.stdin.as_mut().expect("stdin");
        let len = u32::try_from(body.len()).expect("bounded");
        stdin.write_all(&len.to_ne_bytes()).expect("length");
        stdin.write_all(&body).expect("body");
        stdin.flush().expect("flush");
    }

    /// Read one message the host sent.
    fn recv(&mut self) -> Value {
        let mut header = [0u8; 4];
        if let Err(e) = self
            .stdout
            .as_mut()
            .expect("stdout")
            .read_exact(&mut header)
        {
            panic!("no message arrived ({e}); the host said: {}", self.stderr());
        }
        let len = u32::from_ne_bytes(header) as usize;
        let mut body = vec![0u8; len];
        self.stdout
            .as_mut()
            .expect("stdout")
            .read_exact(&mut body)
            .expect("a message body");
        self.log
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(&body).into_owned());
        serde_json::from_slice(&body).expect("json")
    }

    /// Read until a message of this type arrives.
    fn recv_type(&mut self, kind: &str) -> Value {
        for _ in 0..8 {
            let message = self.recv();
            if message["type"] == kind {
                return message;
            }
        }
        panic!("no {kind} message arrived");
    }

    /// Everything that crossed either pipe, as one string.
    fn wire(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }

    fn hello(&mut self, extension_id: &str) {
        self.send(&json!({
            "type": "hello",
            "protocol": "bridge-native@1",
            "extension_id": extension_id,
            "profile_instance": "profile-a",
        }));
    }

    fn attach(&mut self, observation: &Value) {
        self.send(&json!({ "type": "attach", "observation": observation, "consumer": null }));
    }

    fn stop(mut self) -> Option<i32> {
        drop(self.stdin.take());
        drop(self.stdout.take());
        self.child.wait().ok().and_then(|status| status.code())
    }

    /// Everything the host wrote to stderr, for a failure message.
    fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One open assignment page, as the companion would report it.
fn observation() -> Value {
    json!({
        "origin": ORIGIN,
        "tab_id": 7,
        "document_id": "6C1F7A2E9B3D4A58",
        "frame_id": 0,
        "navigation_generation": 1,
        "route": {
            "kind": "assignment",
            "course_id": "45679",
            "assignment_id": "98765",
            "topic_id": null,
            "quiz_id": null,
            "page_url": null,
        },
        "zone": "open",
        "account": { "user_id": "123", "observed_at": "2026-09-10T16:04:40Z" },
        "url": format!("{ORIGIN}/courses/45679/assignments/98765"),
        "title": "Essay 1",
        "observed_at": "2026-09-10T16:04:40Z",
    })
}

/// Wait for a predicate, or give up after two seconds.
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

// -------------------------------------------------------------------- tests

/// M7-a acceptance: only the configured extension, from a real extension
/// origin, may start the broker.
#[test]
fn a_wrong_extension_id_or_a_wrong_origin_is_refused() {
    let f = Fixture::new();

    let wrong = f.host(OTHER_EXTENSION).stop();
    assert_eq!(wrong, Some(8), "another extension started the host");

    // A caller that is not an extension origin at all.
    let output = f
        .command()
        .args(["bridge", "host", "https://evil.test/"])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(8));

    // And no argument at all: Chrome always supplies one.
    let output = f.command().args(["bridge", "host"]).output().expect("run");
    assert_eq!(output.status.code(), Some(8));

    // Nothing above left an endpoint behind.
    assert!(!exists(&f.endpoint()), "a refused host bound the endpoint");
}

/// The whole path: the person attaches a tab, the CLI reads it, and a second
/// consumer sees nothing until it opts in.
#[test]
fn an_attached_tab_is_served_to_the_cli_and_only_to_an_opted_in_consumer() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    let ready = host.recv_type("ready");
    assert_eq!(ready["protocol"], "bridge-native@1");
    assert_eq!(ready["identity_key"], f.doc.key.as_str());

    host.hello(EXTENSION);
    host.attach(&observation());
    assert_eq!(host.recv_type("attached")["state"], "attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    // `bridge status` names the attachment without naming the page.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["schema"], "canvas-cli/bridge@1");
    assert_eq!(status["result"]["owner"]["live"], true);
    let attachment = &status["result"]["attachments"][0];
    assert_eq!(attachment["state"], "attached");
    assert_eq!(attachment["origin"], ORIGIN);
    assert!(
        !status.to_string().contains("Essay 1"),
        "the listing named the page: {status}"
    );

    // The CLI is the person: it reads the sole attachment.
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{here}");
    assert_eq!(here["schema"], "canvas-cli/here@1");
    assert_eq!(here["result"]["state"], "attached");
    assert_eq!(here["result"]["consumer"], Value::Null);
    let browser = &here["result"]["browser"];
    assert_eq!(browser["zone"], "open");
    assert_eq!(browser["course_id"], "45679");
    assert_eq!(browser["assignment_id"], "98765");
    assert_eq!(browser["title"], "Essay 1");
    assert_eq!(browser["account"]["user_id"], "123");
    // Browser context is an observation, never a cached dataset.
    assert_eq!(browser["ttl_ms"], 0);
    assert_eq!(here["freshness"], json!([]));
    // Metadata alone never asks the page for text.
    assert_eq!(browser["text"], Value::Null);
    assert_eq!(browser["selection"], Value::Null);

    host.stop();
}

/// M7-a acceptance: no secret, cookie file, or token appears on either pipe.
#[test]
fn nothing_on_the_wire_carries_a_secret() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);

    // A page whose URL carries a capability the companion already stripped.
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));
    f.json(&["--offline", "here"]);
    f.json(&["bridge", "status"]);

    let wire = host.wire();
    for forbidden in [
        TOKEN,
        PAGE_SECRET,
        "verifier",
        "X-Amz-Signature",
        "Cookie",
        "cookie",
        "authenticity_token",
        "Authorization",
        "Bearer",
        "cookies.sqlite",
        "Cookies",
    ] {
        assert!(
            !wire.contains(forbidden),
            "{forbidden} crossed the pipe:\n{wire}"
        );
    }
    host.stop();
}

/// M7-a acceptance: a stale socket is cleaned only by an owner, and a live
/// endpoint is never unlinked.
#[test]
fn a_restarted_broker_clears_only_a_stale_endpoint() {
    let f = Fixture::new();

    // A stale socket from a host that died without cleaning up.
    let bridge = f.data_root().join("bridge");
    std::fs::create_dir_all(&bridge).expect("dir");
    std::fs::write(f.endpoint(), b"").expect("stale socket");

    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));
    // The stale file was replaced by a socket that answers.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], true);

    // A second host for the same identity reports the owner and exits 8. It
    // must not unlink the live endpoint on its way out.
    let second = f.host(EXTENSION).stop();
    assert_eq!(second, Some(8), "a second host took the identity");
    assert!(exists(&f.endpoint()), "the live endpoint was unlinked");
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], true);

    host.stop();
    // The owner cleans up after itself, and the ownership lock stays.
    wait_for("the endpoint to go", || !exists(&f.endpoint()));
    assert!(exists(&f.owner_lock()), "the ownership lock was removed");
}

/// M7-a acceptance: `identity remove` with a live host completes after the
/// cooperative release, and the root identity lock survives.
#[test]
fn identity_remove_releases_a_live_host() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    let key = f.doc.key.to_string();
    let (removed, code) = f.json(&["identity", "remove", &key, "--yes"]);
    assert_eq!(code, 0, "{removed}");
    assert_eq!(removed["result"]["removed"], true);

    // The host was told to let go, and it did.
    let detach = host.recv_type("detach");
    assert_eq!(detach["reason"], "identity_released");
    assert_eq!(host.stop(), Some(0));

    // The endpoint and the ownership lock of that identity are gone; the
    // broker directory and the root lock directory are not.
    assert!(!exists(&f.endpoint()));
    assert!(!exists(&f.owner_lock()));
    assert!(f.data_root().join("bridge").is_dir());
    assert!(f.data_root().join("locks").is_dir());
}

/// A refusal is an answer: no broker means exit 8 with a named reason.
#[test]
fn an_absent_broker_refuses_with_a_reason() {
    let f = Fixture::new();
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 8, "{here}");
    assert_eq!(here["outcome"], "refused");
    assert_eq!(here["result"]["reason"], "bridge_unavailable");
    assert_eq!(here["result"]["state"], "not_attached");
    assert_eq!(here["result"]["browser"], Value::Null);

    let (detached, code) = f.json(&["bridge", "detach"]);
    assert_eq!(code, 8, "{detached}");
    assert_eq!(detached["result"]["detached"], false);
    assert_eq!(detached["result"]["reason"], "bridge_unavailable");

    // `bridge status` is setup information: it answers without a broker.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], false);
    assert_eq!(status["result"]["attachments"], json!([]));
}

/// `bridge install` writes a private manifest naming this binary, and the
/// human output tells a person what to do next.
#[test]
fn install_writes_the_manifest_and_the_steps() {
    let f = Fixture::new();
    let (installed, code) = f.json(&["bridge", "install", "--extension-id", EXTENSION]);
    assert_eq!(code, 0, "{installed}");
    let result = &installed["result"];
    assert_eq!(result["written"], true);
    assert_eq!(result["host_name"], "com.canvas_cli.bridge");
    assert_eq!(result["extension_id"], EXTENSION);
    let path = PathBuf::from(result["manifest_path"].as_str().expect("a path"));
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(manifest["type"], "stdio");
    assert_eq!(
        manifest["allowed_origins"],
        json!([format!("chrome-extension://{EXTENSION}/")])
    );
    assert!(
        Path::new(manifest["path"].as_str().expect("a path")).is_absolute(),
        "{manifest}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the manifest is not private");
    }

    let human = f.run(&["bridge", "install", "--extension-id", EXTENSION], Some(0));
    assert!(human.contains("Load unpacked"), "{human}");
}

// --------------------------------------------------------- the agent surface

/// One `canvas mcp` instance, speaking hand-written JSON-RPC.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    /// The host name this session reports; the consumer handle follows it.
    host: String,
}

impl Mcp {
    /// One request on the primary revision, which names the host in `_meta`.
    fn call(&mut self, method: &str, mut params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": { "name": self.host.clone(), "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if writeln!(self.stdin.as_mut().expect("stdin"), "{request}")
            .and_then(|()| self.stdin.as_mut().expect("stdin").flush())
            .is_err()
        {
            panic!("the server closed the pipe: {}", self.stderr());
        }
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read");
            assert!(read > 0, "the server closed the pipe: {}", self.stderr());
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value["id"] == json!(id) {
                return value;
            }
        }
    }

    /// The §7 envelope inside one tool result.
    fn tool(&mut self, name: &str, arguments: &Value) -> Value {
        let answer = self.call(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        answer["result"]["structuredContent"].clone()
    }

    fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    }

    fn stop(mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Fixture {
    /// Start `canvas mcp` under a named host, and finish the handshake.
    fn mcp(&self, host_name: &str) -> Mcp {
        let mut command = self.command();
        command
            .args(["--offline", "mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Mcp {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 1,
            host: host_name.to_owned(),
        }
    }
}

/// M7-a acceptance: two consumers, and only the one that opted in sees the
/// bundle. The resource attaches nobody.
#[test]
fn only_the_consumer_that_attached_reads_the_bundle() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    let mut alpha = f.mcp("alpha");
    let mut beta = f.mcp("beta");
    let prefix = format!(
        "canvas://{}/{}/context",
        f.doc.key.as_str(),
        f.doc.generation
    );

    // Nobody has opted in, so nobody reads the page.
    let before = alpha.tool("context.here", &json!({}));
    assert_eq!(before["outcome"], "refused", "{before}");
    assert_eq!(before["result"]["reason"], "not_attached");
    assert_eq!(before["result"]["browser"], Value::Null);

    // Reading the resource is not an opt-in either.
    let read = alpha.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["reason"], "not_attached", "{document}");

    // Alpha opts in. The handle comes back; the page does not.
    let attached = alpha.tool("context.attach", &json!({}));
    assert_eq!(attached["outcome"], "ok", "{attached}");
    assert_eq!(attached["result"]["state"], "attached");
    assert_eq!(attached["result"]["consumer"], "mcp:alpha");
    assert_eq!(attached["result"]["browser"], Value::Null);
    let handle = attached["result"]["attachment"]
        .as_str()
        .expect("an attachment handle")
        .to_owned();

    // Now alpha reads the page, through the tool and through its resource.
    let here = alpha.tool("context.here", &json!({}));
    assert_eq!(here["outcome"], "ok", "{here}");
    assert_eq!(here["result"]["consumer"], "mcp:alpha");
    assert_eq!(here["result"]["browser"]["title"], "Essay 1");
    let read = alpha.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["browser"]["title"], "Essay 1");
    assert_eq!(read["result"]["ttlMs"], 0);

    // Beta never opted in. Alpha's handle does not serve beta either.
    let refused = beta.tool("context.here", &json!({}));
    assert_eq!(refused["result"]["reason"], "not_attached", "{refused}");
    // Nor does alpha's *resource*: a consumer handle is not a name anyone may
    // read under (REPORT section 3.2).
    let borrowed = beta.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = borrowed["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(
        document["result"]["details"]["reason"], "not_attached",
        "beta read alpha's context: {document}"
    );
    assert!(
        !text.contains("Essay 1"),
        "beta read alpha's page: {document}"
    );
    let stolen = beta.tool("context.here", &json!({ "attachment_id": handle }));
    assert_eq!(stolen["result"]["reason"], "not_attached", "{stolen}");
    let read = beta.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:beta") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["reason"], "not_attached", "{document}");

    // Alpha lets go. The tab stays attached for the person.
    let detached = alpha.tool("context.detach", &json!({}));
    assert_eq!(detached["outcome"], "ok", "{detached}");
    assert_eq!(detached["result"]["detached"], true);
    let after = alpha.tool("context.here", &json!({}));
    assert_eq!(after["result"]["reason"], "not_attached", "{after}");
    let (still, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{still}");
    assert_eq!(still["result"]["browser"]["title"], "Essay 1");

    alpha.stop();
    beta.stop();
    host.stop();
}

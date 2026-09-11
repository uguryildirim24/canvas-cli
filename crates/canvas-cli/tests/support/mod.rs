//! The fixture every companion test runs against.
//!
//! One identity, one temporary data root, one `canvas bridge host` child with
//! the native-messaging framing spoken by hand, and one `canvas mcp` child
//! speaking JSON-RPC. The extension side is written out here rather than
//! mocked, because the framing and the message vocabulary are the contract
//! Chrome sees.
//!
//! It is a module rather than a test binary, so `tests/bridge.rs` (M7-a) and
//! `tests/m7b.rs` (the panel, notes, and follow) drive the same harness.

// Each test binary uses part of this, and neither uses all of it.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) const TOKEN: &str = "bridge-secret-token";
pub(crate) const EXTENSION: &str = "abcdefghijklmnopabcdefghijklmnop";
pub(crate) const OTHER_EXTENSION: &str = "ponmlkjihgfedcbaponmlkjihgfedcba";
/// A short origin, deliberately.
///
/// A Unix socket path may hold about a hundred bytes, and a temporary
/// directory on macOS already spends sixty of them, so a realistic Canvas
/// host name would not leave room for `<key>.sock`. The origin is not what
/// these tests are about.
pub(crate) const ORIGIN: &str = "https://s.test";

/// A planted secret: it must never appear on either pipe.
pub(crate) const PAGE_SECRET: &str = "SECRET-CSRF-abcdef123456";

// ------------------------------------------------------------------ fixture

pub(crate) struct Fixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) doc: IdentityDocument,
}

impl Fixture {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn data_root(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    pub(crate) fn endpoint(&self) -> PathBuf {
        self.data_root()
            .join("bridge")
            .join(format!("{}.sock", self.doc.key.as_str()))
    }

    pub(crate) fn owner_lock(&self) -> PathBuf {
        self.data_root()
            .join("bridge")
            .join(format!("{}.lock", self.doc.key.as_str()))
    }

    pub(crate) fn command(&self) -> Command {
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
    pub(crate) fn run(&self, args: &[&str], code: Option<i32>) -> String {
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
    pub(crate) fn json(&self, args: &[&str]) -> (Value, i32) {
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
    pub(crate) fn host(&self, extension_id: &str) -> Host {
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
            answers: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

// ------------------------------------------------------- the extension side

/// The companion, as far as the host can tell.
pub(crate) struct Host {
    pub(crate) child: Child,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) stdout: Option<ChildStdout>,
    /// Every message that crossed either pipe, as raw JSON.
    pub(crate) log: Arc<Mutex<Vec<String>>>,
    /// Only the messages the host wrote.
    pub(crate) answers: Arc<Mutex<Vec<String>>>,
}

impl Host {
    /// Write one native-messaging message: a native-endian length, then JSON.
    pub(crate) fn send(&mut self, message: &Value) {
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
    pub(crate) fn recv(&mut self) -> Value {
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
        let text = String::from_utf8_lossy(&body).into_owned();
        self.log.lock().unwrap().push(text.clone());
        self.answers.lock().unwrap().push(text);
        serde_json::from_slice(&body).expect("json")
    }

    /// Read until a message of this type arrives.
    pub(crate) fn recv_type(&mut self, kind: &str) -> Value {
        for _ in 0..8 {
            let message = self.recv();
            if message["type"] == kind {
                return message;
            }
        }
        panic!("no {kind} message arrived");
    }

    /// Everything that crossed either pipe, as one string.
    pub(crate) fn wire(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }

    /// Only what the host wrote, which is the half this package promises.
    pub(crate) fn answered(&self) -> String {
        self.answers.lock().unwrap().join("\n")
    }

    pub(crate) fn hello(&mut self, extension_id: &str) {
        self.send(&json!({
            "type": "hello",
            "protocol": "bridge-native@1",
            "extension_id": extension_id,
            "profile_instance": "profile-a",
        }));
    }

    pub(crate) fn attach(&mut self, observation: &Value) {
        self.send(&json!({ "type": "attach", "observation": observation, "consumer": null }));
    }

    pub(crate) fn stop(mut self) -> Option<i32> {
        drop(self.stdin.take());
        drop(self.stdout.take());
        self.child.wait().ok().and_then(|status| status.code())
    }

    /// Everything the host wrote to stderr, for a failure message.
    pub(crate) fn stderr(&mut self) -> String {
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
pub(crate) fn observation() -> Value {
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

impl Fixture {
    /// Speak `bridge-ipc@1` to the broker by hand, and read one answer.
    ///
    /// The CLI is the ordinary client; this is here so a test can send what
    /// the CLI would never send, and see the broker refuse it.
    pub(crate) fn socket(&self, request: &Value) -> Value {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(self.endpoint()).expect("connect to the broker");
        writeln!(stream, "{request}").expect("write");
        stream.flush().expect("flush");
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("the broker said {line:?} ({e})"))
    }
}

/// Wait for a predicate, or give up after two seconds.
pub(crate) fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

pub(crate) fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

// --------------------------------------------------------- the agent surface

/// One `canvas mcp` instance, speaking hand-written JSON-RPC.
pub(crate) struct Mcp {
    pub(crate) child: Child,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) stdout: BufReader<ChildStdout>,
    pub(crate) next_id: u64,
    /// The host name this session reports; the consumer handle follows it.
    pub(crate) host: String,
}

impl Mcp {
    /// One request on the primary revision, which names the host in `_meta`.
    pub(crate) fn call(&mut self, method: &str, mut params: Value) -> Value {
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

    pub(crate) fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    }

    pub(crate) fn stop(mut self) {
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
    pub(crate) fn mcp(&self, host_name: &str) -> Mcp {
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

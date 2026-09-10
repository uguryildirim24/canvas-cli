//! What the browser companion costs: `cargo xtask bench --bridge`.
//!
//! Two numbers, each measured the way a consumer actually pays it:
//!
//! - a warm metadata `here` over the `bridge-ipc@1` socket, target
//!   p95 < 100 ms;
//! - a **follow acknowledgement**: the socket round trip that ends when the
//!   companion says it took the navigation, target p95 < 300 ms. It is not
//!   the time to load a page. Nothing here waits for a load, and nothing
//!   here could: the load outcome is a separate message that arrives later
//!   (REPORT §3.2). What this measures is the cost of asking.
//!
//! What is deliberately outside the measurement:
//!
//! - **The account probe.** It runs in the browser, against the user's own
//!   Canvas, and only when text is requested (REPORT §3.3 step 4). A metadata
//!   read never triggers one, so no probe is in these numbers.
//! - **The API side of the bundle.** `canvas here` also resolves the route
//!   through the shared command handlers, and those are already measured by
//!   the SPEC §13 metrics above. This measures the broker alone.
//! - **Process start.** The client here connects to the socket directly, the
//!   way a resident `canvas mcp` does.
//!
//! The extension side is spoken by hand — native-messaging framing on the
//! host's pipes — so the broker under measurement is the shipped one.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use rusqlite::params;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::bench::percentile;

/// Target for one warm metadata `here` over the socket, in milliseconds.
pub const HERE_P95_MS: f64 = 100.0;

/// Target for one follow acknowledgement, in milliseconds.
pub const FOLLOW_P95_MS: f64 = 300.0;

/// Round trips discarded before the timed ones.
pub const WARMUP: u32 = 5;

/// The extension the measured host is configured for.
const EXTENSION: &str = "abcdefghijklmnopabcdefghijklmnop";

/// A deliberately short origin.
///
/// A Unix socket path holds about a hundred bytes, and a temporary directory
/// already spends sixty of them. The origin does not change what is measured.
const ORIGIN: &str = "https://b.test";

const TOKEN: &str = "bench-bridge-token";

/// Latency of one warm socket round trip.
pub struct Latency {
    pub p50: f64,
    pub p95: f64,
    pub runs: u32,
}

impl Latency {
    #[must_use]
    pub fn missed_at(&self, target: f64) -> bool {
        self.p95 > target
    }
}

/// Everything `--bridge` adds to the report.
pub struct Report {
    /// Warm metadata `here` over the socket.
    pub here: Latency,
    /// Bytes of one `here` answer on the socket.
    pub answer_bytes: usize,
    /// The socket round trip that ends at the companion's acknowledgement.
    pub follow: Latency,
}

impl Report {
    #[must_use]
    pub fn missed(&self) -> bool {
        self.here.missed_at(HERE_P95_MS) || self.follow.missed_at(FOLLOW_P95_MS)
    }
}

/// Measure the broker against a host started the way Chrome starts it.
pub fn measure(binary: &Path, runs: u32) -> Result<Report> {
    let bed = Bed::new(binary)?;
    let mut host = bed.start()?;
    host.hello()?;
    host.attach()?;
    host.wait_for("attached")?;
    bed.wait_for_endpoint()?;

    let mut client = Client::connect(&bed.socket())?;
    for _ in 0..WARMUP {
        client.here()?;
    }
    let mut samples = Vec::with_capacity(runs as usize);
    let mut answer_bytes = 0;
    for _ in 0..runs {
        let (bytes, elapsed) = client.here()?;
        answer_bytes = bytes;
        samples.push(elapsed);
    }
    samples.sort_by(f64::total_cmp);

    // The follow round trip needs the companion to answer while the consumer
    // waits, so the extension side moves to its own thread for this part. It
    // answers every navigation at once: what is being measured is the broker
    // and the two pipes, not how fast a browser decides.
    let mut companion = Companion::start(host);
    for _ in 0..WARMUP {
        client.follow(&format!("{ORIGIN}/courses/45679"))?;
    }
    let mut follows = Vec::with_capacity(runs as usize);
    for run in 0..runs {
        let (_, elapsed) = client.follow(&format!("{ORIGIN}/courses/45679/assignments/{run}"))?;
        follows.push(elapsed);
    }
    follows.sort_by(f64::total_cmp);
    companion.stop(&mut client)?;

    Ok(Report {
        here: Latency {
            p50: percentile(&samples, 50.0),
            p95: percentile(&samples, 95.0),
            runs,
        },
        answer_bytes,
        follow: Latency {
            p50: percentile(&follows, 50.0),
            p95: percentile(&follows, 95.0),
            runs,
        },
    })
}

// ---------------------------------------------------- the answering companion

/// The extension side, answering navigations while a consumer waits.
struct Companion {
    worker: Option<JoinHandle<Result<Host>>>,
    stop: std::sync::mpsc::Sender<()>,
}

impl Companion {
    fn start(mut host: Host) -> Self {
        let (stop, stopped) = channel();
        let worker = std::thread::spawn(move || -> Result<Host> {
            answer_navigations(&mut host, &stopped)?;
            Ok(host)
        });
        Self {
            worker: Some(worker),
            stop,
        }
    }

    /// Stop answering, and take the extension side back.
    ///
    /// The thread is blocked reading the host's pipe, and a flag alone would
    /// leave it there until a message it has no reason to expect arrives. So
    /// the flag is set and then one more follow is asked for: the thread
    /// answers it, sees the flag, and returns.
    fn stop(&mut self, client: &mut Client) -> Result<()> {
        let _ = self.stop.send(());
        client.follow(&format!("{ORIGIN}/courses/45679"))?;
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(host) => host?.stop(),
                Err(_) => bail!("the companion thread panicked"),
            }
        }
        Ok(())
    }
}

/// Acknowledge every navigation until the measurement is over.
///
/// The stop flag is read after a message is handled, never before one is
/// waited for, so the last navigation is always answered.
fn answer_navigations(host: &mut Host, stopped: &Receiver<()>) -> Result<()> {
    loop {
        let message = host.recv()?;
        if message["type"] == "navigate" {
            host.send(&json!({
                "type": "navigate_ack",
                "request_id": message["request_id"],
                "accepted": true,
                "reason": null,
            }))?;
            match stopped.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

// ------------------------------------------------------------------ the bed

/// One identity, in a directory short enough to hold a socket path.
struct Bed {
    binary: PathBuf,
    home: tempfile::TempDir,
    key: String,
}

impl Bed {
    fn new(binary: &Path) -> Result<Self> {
        let home = tempfile::Builder::new().prefix("cb").tempdir()?;
        let data_root = home.path().join("d");
        let document = IdentityDocument::new(ORIGIN, 1, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(&data_root, &document.key);
        document.write(&paths.identity_json())?;
        let open = OpenIdentity::open(&paths, &document)?;
        let key = document.key.to_string();
        let stored = key.clone();
        open.store
            .call_blocking(move |conns| {
                conns.state.execute(
                    "INSERT INTO credential (identity_key, token_sha256, validated_at)
                     VALUES (?1, ?2, '2026-01-01T00:00:00Z')",
                    params![stored, format!("{:x}", Sha256::digest(TOKEN.as_bytes()))],
                )?;
                Ok(())
            })
            .context("seed the bridge benchmark credential")?;
        drop(open);

        // The host serves only the extension the config names.
        let config = home.path().join("c").join("canvas-cli");
        std::fs::create_dir_all(&config)?;
        std::fs::write(
            config.join("config.toml"),
            format!("[bridge]\nextension_id = \"{EXTENSION}\"\n"),
        )?;

        let bed = Self {
            binary: binary.to_path_buf(),
            home,
            key,
        };
        let socket = bed.socket();
        let length = socket.as_os_str().len();
        if length > canvas_core::bridge::endpoint::MAX_SOCKET_PATH {
            bail!(
                "the benchmark socket path is {length} bytes, over the {} a Unix \
                 socket allows: {}",
                canvas_core::bridge::endpoint::MAX_SOCKET_PATH,
                socket.display()
            );
        }
        Ok(bed)
    }

    fn data_root(&self) -> PathBuf {
        self.home.path().join("d")
    }

    fn socket(&self) -> PathBuf {
        self.data_root()
            .join("bridge")
            .join(format!("{}.sock", self.key))
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .env("CANVAS_DATA_ROOT", self.data_root())
            .env("CANVAS_DATA_DIR", self.data_root())
            .env("CANVAS_CONFIG_DIR", self.home.path().join("c/canvas-cli"))
            .env("XDG_CONFIG_HOME", self.home.path().join("c"))
            .env("CANVAS_IDENTITY_KEY", &self.key)
            .env("CANVAS_TOKEN", TOKEN)
            .env("TZ", "UTC")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE")
            .env_remove("CANVAS_NOW");
        command
    }

    /// Start `canvas bridge host` the way Chrome starts it.
    fn start(&self) -> Result<Host> {
        let mut command = self.command();
        command
            .arg(format!("chrome-extension://{EXTENSION}/"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("spawn canvas bridge host")?;
        let stdin = child.stdin.take().context("no stdin pipe")?;
        let stdout = child.stdout.take().context("no stdout pipe")?;
        let mut host = Host {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
        };
        host.wait_for("ready")?;
        Ok(host)
    }

    fn wait_for_endpoint(&self) -> Result<()> {
        let socket = self.socket();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if socket.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        bail!("the broker never bound {}", socket.display())
    }
}

// ------------------------------------------------------- the extension side

/// The companion, as far as the broker can tell.
struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

impl Host {
    fn send(&mut self, message: &Value) -> Result<()> {
        let body = serde_json::to_vec(message)?;
        let stdin = self.stdin.as_mut().context("the pipe is closed")?;
        stdin.write_all(&u32::try_from(body.len())?.to_ne_bytes())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Value> {
        let stdout = self.stdout.as_mut().context("the pipe is closed")?;
        let mut header = [0u8; 4];
        stdout.read_exact(&mut header)?;
        let mut body = vec![0u8; u32::from_ne_bytes(header) as usize];
        stdout.read_exact(&mut body)?;
        Ok(serde_json::from_slice(&body)?)
    }

    fn wait_for(&mut self, kind: &str) -> Result<Value> {
        for _ in 0..8 {
            let message = self.recv()?;
            if message["type"] == kind {
                return Ok(message);
            }
        }
        bail!("the host never sent a {kind} message")
    }

    fn hello(&mut self) -> Result<()> {
        self.send(&json!({
            "type": "hello",
            "protocol": "bridge-native@1",
            "extension_id": EXTENSION,
            "profile_instance": "bench",
        }))
    }

    fn attach(&mut self) -> Result<()> {
        self.send(&json!({
            "type": "attach",
            "consumer": null,
            "observation": {
                "origin": ORIGIN,
                "tab_id": 1,
                "document_id": "bench-document",
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
                "account": { "user_id": "1", "observed_at": "2026-01-01T00:00:00Z" },
                "url": format!("{ORIGIN}/courses/45679/assignments/98765"),
                "title": "Essay 1",
                "observed_at": "2026-01-01T00:00:00Z",
            },
        }))
    }

    fn stop(mut self) {
        self.stdin.take();
        self.stdout.take();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// --------------------------------------------------------- the consumer side

/// One resident consumer on the broker socket.
struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    fn connect(socket: &Path) -> Result<Self> {
        let stream =
            UnixStream::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            next_id: 1,
        })
    }

    /// One warm metadata `here`: bytes of the answer, and the round trip.
    fn here(&mut self) -> Result<(usize, f64)> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let request = json!({
            "v": "bridge-ipc@1",
            "id": id,
            "op": "here",
            "include_text": false,
        });
        let started = Instant::now();
        writeln!(self.stream, "{request}")?;
        self.stream.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            bail!("the broker closed the endpoint");
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let answer: Value = serde_json::from_str(&line).context("not bridge-ipc@1")?;
        if answer["result"] != "context" {
            bail!("the broker refused a warm here: {answer}");
        }
        Ok((line.len(), elapsed))
    }

    /// One follow: the round trip that ends at the acknowledgement.
    fn follow(&mut self, url: &str) -> Result<(usize, f64)> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let request = json!({
            "v": "bridge-ipc@1",
            "id": id,
            "op": "follow",
            "generation": 1,
            "url": url,
        });
        let started = Instant::now();
        writeln!(self.stream, "{request}")?;
        self.stream.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            bail!("the broker closed the endpoint");
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let answer: Value = serde_json::from_str(&line).context("not bridge-ipc@1")?;
        if answer["result"] != "followed" {
            bail!("the broker refused a follow: {answer}");
        }
        Ok((line.len(), elapsed))
    }
}

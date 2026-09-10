//! `cargo xtask bench [--fixture SET] [--runs N] [--no-fail]`.
//!
//! Measures the SPEC §13 targets against a five-course fixture set served by
//! `wiremock`, with a release build of `canvas`. Appendix A pins no benchmark
//! crate, so the timing is `std::time::Instant` around repeated runs of the
//! real binary, which is also what §13 describes.
//!
//! Two limitations are inherent and are repeated in `docs/bench.md`:
//!
//! - **Cold start.** Dropping the operating-system page cache needs root. The
//!   benchmark emulates a cold start with a fresh copy of the cache files and
//!   a new process, so the `SQLite` page cache, the connection, and the process
//!   itself are cold. The file bytes may still sit in the OS cache, so the
//!   measured number is a lower bound.
//! - **The concurrent download.** A transfer over plain `http` is refused
//!   unless the binary was built with debug assertions (SPEC §11 keeps the
//!   `https` rule in release builds), and `wiremock` serves no TLS. The
//!   measured `todo` runs therefore use the release binary, while the
//!   download that loads the server alongside them uses a debug build.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use serde_json::Value;
use sha2::{Digest, Sha256};
use wiremock::matchers::{method as method_matcher, path as path_matcher, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::bench_fixture::{self, BLOB_PREFIX, DOWNLOAD_COURSE, DOWNLOAD_FILES};
use crate::bench_mcp;
use crate::fixture::{Recorded, load_manifest, load_set};

/// The set `bench` uses when none is named.
pub const DEFAULT_SET: &str = "bench-5";

/// The token the benchmark identity uses. It never leaves the local mock.
const TOKEN: &str = "bench-fixture-token";

/// The user the fixture set's `users/self` response reports.
const USER_ID: i64 = 1001;

/// Options of the bench task.
pub struct Options {
    pub fixture: String,
    pub runs: u32,
    pub no_fail: bool,
    /// Also measure the agent surface (`canvas mcp`).
    pub mcp: bool,
    /// Measure one `watch` tick, and the §13 targets with `watch` running.
    pub watch: bool,
    /// Where the report goes. `None` means `docs/bench.md`.
    pub doc: Option<PathBuf>,
}

/// One SPEC §13 target.
struct Target {
    metric: &'static str,
    label: &'static str,
    p50_ms: Option<f64>,
    p95_ms: f64,
}

const TARGETS: [Target; 3] = [
    Target {
        metric: "todo_first_output",
        label: "cached todo, first output",
        p50_ms: Some(50.0),
        p95_ms: 150.0,
    },
    Target {
        metric: "todo_total",
        label: "cached todo, full run",
        p50_ms: None,
        p95_ms: 250.0,
    },
    Target {
        metric: "cold_start",
        label: "cold start",
        p50_ms: None,
        p95_ms: 400.0,
    },
];

/// What the served fixture set actually holds.
///
/// The report must describe the set it served, not the set it expected. SPEC
/// §13 fixes the shape at five courses; a set with another count still
/// benchmarks, but the number it was measured against has to be on the page.
struct SetShape {
    courses: usize,
    extra_pages: usize,
}

impl SetShape {
    fn of(entries: &[(String, Recorded)]) -> Self {
        let courses = entries
            .iter()
            .find(|(_, r)| r.path == "/api/v1/courses" && r.page.unwrap_or(1) == 1)
            .and_then(|(_, r)| r.body.as_array())
            .map_or(0, Vec::len);
        let extra_pages = entries
            .iter()
            .filter(|(_, r)| r.page.is_some_and(|p| p > 1))
            .count();
        Self {
            courses,
            extra_pages,
        }
    }
}

/// One measured metric under one load condition.
struct Measured {
    label: &'static str,
    load: Load,
    p50: f64,
    p95: f64,
    target_p50: Option<f64>,
    target_p95: f64,
}

/// What else was running while a metric was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Load {
    /// Nothing.
    Idle,
    /// One `canvas download` stream against the same server.
    Download,
    /// One resident `canvas watch` on the same identity (REPORT §3.6).
    Watch,
}

impl Load {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Download => "download",
            Self::Watch => "watch",
        }
    }
}

/// One `canvas watch --jsonl --once` tick.
struct Tick {
    p50: f64,
    p95: f64,
    /// Events the last measured tick streamed.
    events: u64,
    /// API requests the last measured tick made.
    requests: u64,
}

/// Everything one bench run measured, apart from the agent surface.
struct Measurements {
    targets: Vec<Measured>,
    tick: Option<Tick>,
}

impl Measured {
    fn missed(&self) -> bool {
        self.p95 > self.target_p95 || self.target_p50.is_some_and(|t| self.p50 > t)
    }
}

/// Run the bench task. `Ok(false)` means a target was missed.
pub fn run(options: &Options) -> Result<bool> {
    if options.runs == 0 {
        bail!("--runs must be at least 1");
    }
    let root = workspace_root()?;
    let set_dir = root
        .join(crate::fixture::TRACKED_FIXTURES)
        .join(&options.fixture);
    if !set_dir.exists() {
        if options.fixture != DEFAULT_SET {
            bail!(
                "fixture set {} does not exist; record one, or use the generated {DEFAULT_SET}",
                set_dir.display()
            );
        }
        let count = bench_fixture::generate(&set_dir)?;
        eprintln!(
            "generated {count} synthetic responses in {}",
            set_dir.display()
        );
    }
    let entries = load_set(&set_dir)?;
    let shape = SetShape::of(&entries);
    if shape.courses != 5 {
        eprintln!(
            "warning: {} holds {} courses; SPEC section 13 measures a 5-course set",
            options.fixture, shape.courses
        );
    }
    let manifest = load_manifest(&set_dir)?;
    let shift = manifest
        .as_ref()
        .filter(|m| m.synthetic)
        .and_then(|m| day_shift(&m.recorded_at))
        .unwrap_or(0);

    eprintln!("building canvas (release, and debug for the download load)");
    let release = build_binary(&root, true)?;
    let debug = build_binary(&root, false)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let measurements = runtime.block_on(async move {
        let server = MockServer::start().await;
        mount_set(&server, &entries, shift).await;
        mount_blobs(&server).await;
        let uri = server.uri();
        let runs = options.runs;
        let mcp = options.mcp;
        let watch = options.watch;
        let outcome = tokio::task::spawn_blocking(move || {
            let harness = Harness::new(release, debug, uri)?;
            harness.prime()?;
            let measured = harness.measure_all(runs, watch)?;
            let agent = if mcp {
                Some(harness.measure_mcp(runs)?)
            } else {
                None
            };
            anyhow::Ok((measured, agent))
        })
        .await?;
        // A failure is almost always a request the set does not answer. Name
        // the requests the server saw, so the cause is visible at once.
        if outcome.is_err()
            && let Some(seen) = server.received_requests().await
        {
            eprintln!("requests the fixture server received:");
            for request in seen {
                eprintln!("  {} {}", request.method, request.url);
            }
        }
        // Keep the server alive until the measurements finish.
        drop(server);
        anyhow::Ok(outcome?)
    })?;

    let (measurements, agent) = measurements;
    let passed = report(&root, options, &shape, &measurements, agent.as_ref())?;
    Ok(passed || options.no_fail)
}

// ---------------------------------------------------------------- fixtures

/// Whole days between a set's record date and today.
///
/// The difference is taken in seconds. A `Span` between two timestamps
/// carries its own largest unit, so asking one for hours can answer zero.
fn day_shift(recorded_at: &str) -> Option<i64> {
    let recorded: jiff::Timestamp = recorded_at.parse().ok()?;
    let seconds = jiff::Timestamp::now().as_second() - recorded.as_second();
    Some(seconds / 86_400)
}

/// Move every RFC 3339 timestamp in a body forward by `days`.
fn shift_days(value: &Value, days: i64) -> Value {
    match value {
        Value::String(text) => match text.parse::<jiff::Timestamp>() {
            Ok(t) if looks_like_timestamp(text) => {
                Value::String((t + jiff::SignedDuration::from_hours(days * 24)).to_string())
            }
            _ => value.clone(),
        },
        Value::Array(items) => Value::Array(items.iter().map(|i| shift_days(i, days)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), shift_days(v, days)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `jiff` parses more shapes than Canvas sends; require the wire form.
fn looks_like_timestamp(text: &str) -> bool {
    text.len() >= 20
        && text.as_bytes()[4] == b'-'
        && text.as_bytes()[10] == b'T'
        && (text.ends_with('Z') || text.contains('+'))
}

/// Mount every recorded response.
///
/// The match is method plus path, narrowed by the query pairs that separate
/// two calls to the same path. `per_page` and `include[]` are boilerplate the
/// client adds, and the planner window moves with the clock, so neither can
/// take part in the match.
async fn mount_set(server: &MockServer, entries: &[(String, Recorded)], shift: i64) {
    for (_, recorded) in entries {
        let mut mock = Mock::given(method_matcher(recorded.method.as_str()))
            .and(path_matcher(recorded.path.clone()));
        for (key, value) in &recorded.query {
            if key == "per_page"
                || key.starts_with("include[")
                || key == "start_date"
                || key == "end_date"
            {
                continue;
            }
            mock = mock.and(query_param(key.clone(), value.clone()));
        }
        let body = if shift == 0 {
            recorded.body.clone()
        } else {
            shift_days(&recorded.body, shift)
        };
        let mut response = ResponseTemplate::new(recorded.status).set_body_json(body);
        for (name, value) in &recorded.headers {
            if name == "link" {
                // A recorded `Link` points at the recorded host; a replay would
                // send the client off-origin. Pagination is not part of what the
                // benchmark measures, so the header is dropped.
                continue;
            }
            response = response.append_header(name.as_str(), value.as_str());
        }
        server.register(mock.respond_with(response)).await;
    }
}

/// Mount the file bodies the download load streams.
async fn mount_blobs(server: &MockServer) {
    for (id, size) in DOWNLOAD_FILES {
        let body = vec![b'c'; usize::try_from(size).unwrap_or(0)];
        server
            .register(
                Mock::given(method_matcher("GET"))
                    .and(path_matcher(format!("{BLOB_PREFIX}{id}")))
                    // No artificial delay: a stream that spends its time
                    // waiting would load neither the server nor the client.
                    // The measurement loop restarts the download whenever it
                    // finishes, so a stream is always in flight.
                    .respond_with(ResponseTemplate::new(200).set_body_bytes(body)),
            )
            .await;
    }
}

// ---------------------------------------------------------------- building

/// The workspace root, from this crate's manifest directory.
fn workspace_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .context("xtask has no parent directory")
}

/// Build `canvas` and return the path cargo reports.
fn build_binary(root: &Path, release: bool) -> Result<PathBuf> {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command
        .current_dir(root)
        .args(["build", "--bin", "canvas", "--message-format", "json"]);
    if release {
        command.arg("--release");
    }
    let output = command.stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        bail!("cargo build failed");
    }
    let mut found = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] == "compiler-artifact"
            && let Some(path) = message["executable"].as_str()
            && path.ends_with("canvas")
        {
            found = Some(PathBuf::from(path));
        }
    }
    found.context("cargo did not report a canvas executable")
}

// ------------------------------------------------------------- measurement

/// Everything a measured run needs.
struct Harness {
    release: PathBuf,
    debug: PathBuf,
    origin: String,
    home: tempfile::TempDir,
    identity: String,
}

impl Harness {
    /// Seed the identity the measured runs use.
    ///
    /// A command needs an identity key before it does anything (SPEC §8), and
    /// `auth login` is interactive, so the benchmark writes the identity
    /// document and the validated-credential row the same way the end-to-end
    /// tests do. The token is a local constant and never leaves the mock.
    fn new(release: PathBuf, debug: PathBuf, origin: String) -> Result<Self> {
        let home = tempfile::tempdir()?;
        let data_root = home.path().join("data");
        // A `watch` run gets its own config directory with every TTL at zero,
        // so a measured tick does the whole §10 refresh pass instead of finding
        // the cache fresh. The measured `todo --offline` runs keep the default
        // config, which they read the cache under.
        let watch_config = home.path().join("watch-config");
        std::fs::create_dir_all(&watch_config)?;
        std::fs::write(
            watch_config.join("config.toml"),
            "[cache]\nttl_courses = \"0m\"\nttl_grades = \"0m\"\n\
             ttl_assignments = \"0m\"\nttl_missing = \"0m\"\n\
             ttl_planner = \"0m\"\nttl_announcements = \"0m\"\n",
        )?;
        let document = IdentityDocument::new(&origin, USER_ID, "2026-01-01T00:00:00Z");
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
                    rusqlite::params![stored, format!("{:x}", Sha256::digest(TOKEN.as_bytes()))],
                )?;
                Ok(())
            })
            .context("seed the benchmark credential")?;
        drop(open);
        Ok(Self {
            release,
            debug,
            origin,
            home,
            identity: key,
        })
    }

    fn data_root(&self) -> PathBuf {
        self.home.path().join("data")
    }

    fn command(&self, binary: &Path, data_root: &Path) -> Command {
        let mut command = Command::new(binary);
        command
            .env("CANVAS_DATA_ROOT", data_root)
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("CANVAS_HOST", &self.origin)
            .env("CANVAS_TOKEN", TOKEN)
            .env("TZ", "UTC")
            .env("COLUMNS", "100")
            .env_remove("CANVAS_PROFILE")
            .env_remove("CANVAS_NOW");
        if !self.identity.is_empty() {
            command.env("CANVAS_IDENTITY_KEY", &self.identity);
        }
        command
    }

    /// Prime the cache: `canvas sync` for the courses and grades datasets,
    /// then one online `todo` for the planner and missing datasets it reads.
    fn prime(&self) -> Result<()> {
        let output = self
            .command(&self.release.clone(), &self.data_root())
            .args(["sync", "--json", "--color", "never"])
            .output()?;
        let envelope: Value = serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "sync produced no JSON envelope: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
        })?;
        if !output.status.success() {
            bail!(
                "sync exited {:?}: {}",
                output.status.code(),
                envelope["result"]
            );
        }
        let reported = envelope["identity"]["key"]
            .as_str()
            .context("sync reported no identity")?;
        if reported != self.identity {
            bail!(
                "sync bound {reported}, not the seeded identity {}",
                self.identity
            );
        }

        let todo = self
            .command(&self.release.clone(), &self.data_root())
            .args(["todo", "--json", "--color", "never", "-v"])
            .output()?;
        if !todo.status.success() {
            bail!(
                "priming todo exited {:?}: {} STDERR {}",
                todo.status.code(),
                String::from_utf8_lossy(&todo.stdout).trim(),
                String::from_utf8_lossy(&todo.stderr).trim()
            );
        }
        // The measured runs read the cache only; prove that works before timing it.
        let offline = self
            .command(&self.release.clone(), &self.data_root())
            .args(["todo", "--offline", "--color", "never"])
            .output()?;
        if !offline.status.success() {
            bail!(
                "cached todo exited {:?}: {}",
                offline.status.code(),
                String::from_utf8_lossy(&offline.stderr).trim()
            );
        }
        self.probe_download()
    }

    /// One `todo` run: time to the first byte on stdout, and to exit.
    fn todo_once(&self, data_root: &Path) -> Result<(f64, f64)> {
        let started = Instant::now();
        let mut child = self
            .command(&self.release, data_root)
            .args(["todo", "--offline", "--color", "never"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().context("no stdout pipe")?;
        let mut first = [0u8; 1];
        let read = stdout.read(&mut first)?;
        let first_output = started.elapsed().as_secs_f64() * 1000.0;
        let mut rest = Vec::new();
        stdout.read_to_end(&mut rest)?;
        let status = child.wait()?;
        let total = started.elapsed().as_secs_f64() * 1000.0;
        if !status.success() || read == 0 {
            bail!("todo exited {:?} with {read} bytes read", status.code());
        }
        Ok((first_output, total))
    }

    /// The config directory a `watch` run reads.
    fn watch_config(&self) -> PathBuf {
        self.home.path().join("watch-config")
    }

    /// A `canvas watch` command: its own config, and plain `http` allowed.
    fn watch_command(&self, binary: &Path) -> Command {
        let mut command = self.command(binary, &self.data_root());
        command
            .env("CANVAS_CONFIG_DIR", self.watch_config())
            .env("CANVAS_TEST_ALLOW_HTTP", "1");
        command
    }

    /// One `canvas watch --jsonl --once` tick: refresh, observe, and stream.
    fn watch_tick_once(&self) -> Result<(f64, u64, u64)> {
        let started = Instant::now();
        let output = self
            .watch_command(&self.release)
            .args(["watch", "--jsonl", "--once", "--color", "never"])
            .stderr(Stdio::null())
            .output()?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if !output.status.success() {
            bail!("watch --once exited {:?}", output.status.code());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let summary: Value = text
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .and_then(|line| serde_json::from_str(line).ok())
            .context("watch --once printed no summary document")?;
        Ok((
            elapsed,
            summary["result"]["events"].as_u64().unwrap_or_default(),
            summary["requests"]["api"].as_u64().unwrap_or_default(),
        ))
    }

    /// Start a resident `canvas watch`, ticking fast enough to be a load.
    ///
    /// The tick interval is a debug-only test override, exactly like the
    /// download load's plain-`http` opt-in, so the load uses the debug binary
    /// while the measured runs stay on the release one.
    fn start_watch(&self) -> Result<Child> {
        Ok(self
            .watch_command(&self.debug)
            .args(["watch", "--jsonl", "--color", "never"])
            .env("CANVAS_TEST_WATCH_TICK_MS", "200")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }

    /// The quiesced copy every cold run is made from.
    ///
    /// Copying the live data root while a download writes to it can race a
    /// journal file that the downloader removes between the listing and the
    /// copy. One snapshot, taken before any load starts, also keeps the two
    /// load groups reading the same bytes.
    fn snapshot(&self) -> PathBuf {
        self.home.path().join("cold-source")
    }

    /// One cold start: a fresh copy of the cache and a new process.
    fn cold_once(&self, index: u32) -> Result<f64> {
        let cold = self.home.path().join(format!("cold-{index}"));
        copy_dir(&self.snapshot(), &cold)?;
        let started = Instant::now();
        let output = self
            .command(&self.release, &cold)
            .args(["todo", "--offline", "--color", "never"])
            .output()?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if !output.status.success() {
            bail!("cold todo exited {:?}", output.status.code());
        }
        std::fs::remove_dir_all(&cold)?;
        Ok(elapsed)
    }

    /// Prove the download path works against this fixture before it is used
    /// as load. A download that fails at once would leave the "under load"
    /// group measuring an idle server.
    fn probe_download(&self) -> Result<()> {
        let (mut child, dest) = self.start_download()?;
        let status = child.wait()?;
        if !status.success() {
            bail!(
                "the load download exited {:?}; it cannot load the server",
                status.code()
            );
        }
        let mut bytes = 0;
        for entry in walkdir(&dest)? {
            bytes += entry.metadata()?.len();
        }
        let want: u64 = DOWNLOAD_FILES.iter().map(|(_, size)| size).sum();
        if bytes < want {
            bail!("the load download wrote {bytes} bytes, expected at least {want}");
        }
        std::fs::remove_dir_all(&dest)?;
        Ok(())
    }

    /// Start a download against the same mock, to load it during a run.
    fn start_download(&self) -> Result<(Child, PathBuf)> {
        let dest = self.home.path().join(format!(
            "dl-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dest)?;
        let mut command = self.command(&self.debug, &self.data_root());
        command
            .arg("download")
            .arg(DOWNLOAD_COURSE.to_string())
            .arg("--dest")
            .arg(&dest)
            .args(["--jobs", "1", "--color", "never"]);
        for (id, _) in DOWNLOAD_FILES {
            command.args(["--file", &id.to_string()]);
        }
        // The transfer rules refuse plain `http` unless the build carries debug
        // assertions and the caller opts in; both hold only for this child.
        command
            .env("CANVAS_TEST_ALLOW_HTTP", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok((command.spawn()?, dest))
    }

    /// Runs discarded before each group. The first run after a build pays for
    /// a cold binary and a cold directory, which is not what §13 describes.
    const WARMUP: u32 = 2;

    /// Measure the agent surface over a real stdio pipe.
    ///
    /// The cache is already primed, and the server runs `--offline`, so the
    /// numbers bound the client's own work: the schema it hands a host, and
    /// the round trip of one warm tool call.
    fn measure_mcp(&self, runs: u32) -> Result<bench_mcp::Report> {
        let data_root = self.data_root();
        let factory = || {
            let mut command = self.command(&self.release, &data_root);
            command.args(["--offline", "--color", "never"]);
            command
        };
        bench_mcp::measure(&factory, runs, DOWNLOAD_COURSE, DOWNLOAD_COURSE * 100 + 1)
    }

    fn measure_all(&self, runs: u32, watch: bool) -> Result<Measurements> {
        copy_dir(&self.data_root(), &self.snapshot())?;
        let mut out = Vec::new();
        let groups: &[Load] = if watch {
            &[Load::Idle, Load::Download, Load::Watch]
        } else {
            &[Load::Idle, Load::Download]
        };
        for group in groups.iter().copied() {
            let mut load = match group {
                Load::Idle => None,
                Load::Download => Some(self.start_download()?.0),
                Load::Watch => Some(self.start_watch()?),
            };
            for _ in 0..Self::WARMUP {
                self.todo_once(&self.data_root())?;
            }
            self.cold_once(u32::MAX)?;
            let mut first = Vec::new();
            let mut total = Vec::new();
            let mut cold = Vec::new();
            for index in 0..runs {
                if let Some(child) = load.as_mut()
                    && child.try_wait()?.is_some()
                {
                    // The load ended early; start another so every run in this
                    // group carries the same load.
                    *child = match group {
                        Load::Watch => self.start_watch()?,
                        _ => self.start_download()?.0,
                    };
                }
                let (f, t) = self.todo_once(&self.data_root())?;
                first.push(f);
                total.push(t);
                cold.push(self.cold_once(index)?);
            }
            if let Some(mut child) = load {
                let _ = child.kill();
                let _ = child.wait();
            }
            for target in &TARGETS {
                let samples = match target.metric {
                    "todo_first_output" => &first,
                    "todo_total" => &total,
                    _ => &cold,
                };
                out.push(Measured {
                    label: target.label,
                    load: group,
                    p50: percentile(samples, 0.50),
                    p95: percentile(samples, 0.95),
                    target_p50: target.p50_ms,
                    target_p95: target.p95_ms,
                });
            }
        }
        let tick = if watch {
            // The first tick of a scope only sets its baseline, so it is a
            // warm-up here as well as in the report's own terms.
            self.watch_tick_once()?;
            let mut samples = Vec::new();
            let (mut events, mut requests) = (0, 0);
            for _ in 0..runs {
                let (elapsed, streamed, api) = self.watch_tick_once()?;
                samples.push(elapsed);
                events = streamed;
                requests = api;
            }
            Some(Tick {
                p50: percentile(&samples, 0.50),
                p95: percentile(&samples, 0.95),
                events,
                requests,
            })
        } else {
            None
        };
        Ok(Measurements { targets: out, tick })
    }
}

/// Nearest-rank percentile in milliseconds.
pub(crate) fn percentile(samples: &[f64], q: f64) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// Every file under a directory tree.
fn walkdir(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.extend(walkdir(&entry.path())?);
        } else {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Copy a directory tree. The cache is a handful of files; no symlink in it.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- reporting

/// Print the table and write `docs/bench.md`. Returns false on a missed target.
fn report(
    root: &Path,
    options: &Options,
    shape: &SetShape,
    measurements: &Measurements,
    agent: Option<&bench_mcp::Report>,
) -> Result<bool> {
    let mut passed = true;
    println!(
        "{:<28} {:<10} {:>9} {:>9} {:>12} {:>7}",
        "metric", "load", "p50 ms", "p95 ms", "target p95", "verdict"
    );
    for m in &measurements.targets {
        if m.missed() {
            passed = false;
        }
        println!(
            "{:<28} {:<10} {:>9.1} {:>9.1} {:>12.0} {:>7}",
            m.label,
            m.load.label(),
            m.p50,
            m.p95,
            m.target_p95,
            if m.missed() { "MISS" } else { "ok" }
        );
    }
    if let Some(tick) = &measurements.tick {
        println!(
            "{:<28} {:<10} {:>9.1} {:>9.1} {:>12} {:>7}",
            "watch tick", "idle", tick.p50, tick.p95, "none", "—"
        );
    }
    if let Some(agent) = agent {
        if agent.missed() {
            passed = false;
        }
        println!(
            "{:<28} {:<10} {:>9.1} {:>9.1} {:>12.0} {:>7}",
            "warm todo.list over stdio",
            "mcp",
            agent.latency.p50,
            agent.latency.p95,
            bench_mcp::ROUND_TRIP_P95_MS,
            if agent.missed() { "MISS" } else { "ok" }
        );
        println!(
            "\ncatalog: {} tools, {} bytes, ~{} tokens per `tools/list`",
            agent.catalog.len(),
            agent.catalog_bytes(),
            agent.catalog_tokens()
        );
    }
    let doc = options
        .doc
        .clone()
        .unwrap_or_else(|| root.join("docs").join("bench.md"));
    if let Some(parent) = doc.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&doc, document(options, shape, measurements, agent, passed)?)?;
    println!("\nwrote {}", doc.display());
    if !passed {
        eprintln!("a SPEC section 13 target was missed");
    }
    Ok(passed)
}

/// The commit the numbers belong to.
fn commit(root: &Path) -> String {
    Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

/// A one-line machine description.
fn machine() -> String {
    let mut parts = BTreeMap::new();
    for (label, program, args) in [("os", "uname", vec!["-sr"]), ("arch", "uname", vec!["-m"])] {
        if let Ok(out) = Command::new(program).args(&args).output()
            && out.status.success()
        {
            parts.insert(
                label,
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            );
        }
    }
    let cpu = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let os = parts.get("os").cloned().unwrap_or_default();
    let arch = parts.get("arch").cloned().unwrap_or_default();
    if cpu.is_empty() {
        format!("{os} {arch}").trim().to_owned()
    } else {
        format!("{cpu}, {os} {arch}")
    }
}

#[allow(clippy::too_many_lines)]
fn document(
    options: &Options,
    shape: &SetShape,
    measurements: &Measurements,
    agent: Option<&bench_mcp::Report>,
    passed: bool,
) -> Result<String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    writeln!(out, "# Benchmarks\n")?;
    writeln!(
        out,
        "Generated by `cargo xtask bench{} --runs {}`. Do not edit by hand.\n",
        if options.mcp { " --mcp" } else { "" },
        options.runs
    )?;
    writeln!(out, "| | |")?;
    writeln!(out, "|---|---|")?;
    writeln!(out, "| Date | {} |", jiff::Timestamp::now())?;
    writeln!(out, "| Commit | `{}` |", commit(&workspace_root()?))?;
    writeln!(out, "| Machine | {} |", machine())?;
    writeln!(out, "| Fixture set | `{}` |", options.fixture)?;
    writeln!(out, "| Courses in set | {} |", shape.courses)?;
    writeln!(out, "| Runs per metric | {} |", options.runs)?;
    writeln!(
        out,
        "| Verdict | {} |\n",
        if passed {
            "every target met"
        } else {
            "a target was missed"
        }
    )?;

    writeln!(out, "## Targets and measurements\n")?;
    writeln!(
        out,
        "Targets are SPEC §13: cached `todo` first output p50 < 50 ms and \
         p95 < 150 ms; full cached `todo` p95 < 250 ms; cold start p95 < 400 ms. \
         Every metric is measured idle and while one `download` stream runs \
         against the same mock server{}.\n",
        if options.watch {
            ", and once more with a resident `canvas watch` on the same identity"
        } else {
            ""
        }
    )?;
    writeln!(
        out,
        "| Metric | Load | p50 ms | p95 ms | Target p50 | Target p95 | Verdict |"
    )?;
    writeln!(out, "|---|---|---:|---:|---:|---:|---|")?;
    for m in &measurements.targets {
        writeln!(
            out,
            "| {} | {} | {:.1} | {:.1} | {} | {:.0} | {} |",
            m.label,
            m.load.label(),
            m.p50,
            m.p95,
            m.target_p50
                .map_or_else(|| "—".to_owned(), |t| format!("{t:.0}")),
            m.target_p95,
            if m.missed() { "**miss**" } else { "ok" }
        )?;
    }

    if let Some(tick) = &measurements.tick {
        writeln!(out, "\n## Watch\n")?;
        writeln!(
            out,
            "One `canvas watch --jsonl --once` tick against the same fixture \
             set: retention, any pending observation, the §10 refresh pass, the \
             baseline comparison, and the stream write. Every TTL is set to zero \
             for these runs, so each tick refreshes the whole set; a steady-state \
             watch refreshes only what its TTL has expired, so this is an upper \
             bound on tick cost, not a typical one.\n"
        )?;
        writeln!(out, "| Metric | p50 ms | p95 ms | Target |")?;
        writeln!(out, "|---|---:|---:|---|")?;
        writeln!(
            out,
            "| watch tick, full refresh | {:.1} | {:.1} | none (REPORT §3.6: \
             dataset TTL plus backoff, no 60 s freshness guarantee) |",
            tick.p50, tick.p95
        )?;
        writeln!(
            out,
            "\nThe last measured tick made {} API request(s) and streamed {} \
             event(s). REPORT §3.6 sets no latency target for `watch`; the \
             number is recorded so a later change can be compared with it.",
            tick.requests, tick.events
        )?;
        writeln!(
            out,
            "\nThe `watch` rows of the table above are the §13 `todo` targets \
             measured while that resident `watch` was running, which is the \
             REPORT §4 acceptance condition for M6-c."
        )?;
    } else {
        writeln!(out, "\n## Watch\n")?;
        writeln!(
            out,
            "Not measured in this run. `cargo xtask bench --watch` adds one \
             `canvas watch --jsonl --once` tick and repeats the §13 targets \
             with a resident `watch` on the same identity."
        )?;
    }

    if let Some(agent) = agent {
        writeln!(out, "\n## Agent surface (`canvas mcp`)\n")?;
        writeln!(
            out,
            "Measured over a real stdio pipe with hand-written JSON-RPC on \
             protocol `2026-07-28`, against the same primed cache, with the \
             server run `--offline`.\n"
        )?;
        writeln!(out, "| Metric | p50 ms | p95 ms | Target p95 | Verdict |")?;
        writeln!(out, "|---|---:|---:|---:|---|")?;
        writeln!(
            out,
            "| warm `todo.list` round trip | {:.1} | {:.1} | {:.0} | {} |",
            agent.latency.p50,
            agent.latency.p95,
            bench_mcp::ROUND_TRIP_P95_MS,
            if agent.missed() { "**miss**" } else { "ok" }
        )?;
        writeln!(
            out,
            "\n{} timed calls after {} warm-up calls, on one long-lived \
             connection. The round trip is the time from writing the request \
             line to reading the response line, so it carries the process's \
             own work and the pipe, and no model time.\n",
            agent.latency.runs,
            bench_mcp::WARMUP
        )?;

        writeln!(out, "### Schema cost per tool\n")?;
        writeln!(
            out,
            "The bytes each tool definition puts on the wire in `tools/list`, \
             which a host pays once per session before the model has read \
             anything. The token column is an estimate: one token per {} \
             bytes of UTF-8. That is a rule of thumb for JSON with English \
             identifiers, not a tokenizer run; the byte column is exact.\n\n\
             Most of each row is the output schema, which is the whole §7 \
             envelope in both shapes: the command's result and the `error@1` \
             branch. Both are self-contained, with every sub-schema inlined, \
             because a host validator reads a tool definition on its own. \
             That is why the total is what it is, and it is the number to \
             beat if the catalog is ever trimmed.\n",
            bench_mcp::BYTES_PER_TOKEN
        )?;
        writeln!(out, "| Tool | Bytes | ~Tokens |")?;
        writeln!(out, "|---|---:|---:|")?;
        for tool in &agent.catalog {
            writeln!(
                out,
                "| `{}` | {} | {} |",
                tool.name, tool.bytes, tool.tokens
            )?;
        }
        writeln!(
            out,
            "| **{} tools** | **{}** | **~{}** |",
            agent.catalog.len(),
            agent.catalog_bytes(),
            agent.catalog_tokens()
        )?;

        writeln!(out, "\n### Round trips per workflow\n")?;
        writeln!(
            out,
            "One row per workflow of the shipped skill \
             (`skill/canvas-cli/`). Each round trip is one host turn, so the \
             call count is what a workflow costs in conversation. `outcome \
             (exit)` is what each call reported against this fixture.\n"
        )?;
        writeln!(out, "| Workflow | Calls | Measured ms | Sequence |")?;
        writeln!(out, "|---|---:|---:|---|")?;
        for workflow in &agent.workflows {
            let sequence: Vec<String> = workflow
                .calls
                .iter()
                .map(|(tool, outcome)| format!("`{tool}` {outcome}"))
                .collect();
            writeln!(
                out,
                "| {} | {} | {:.1} | {} |",
                workflow.name,
                workflow.total_calls(),
                workflow.total_ms,
                sequence.join(" → ")
            )?;
        }
        for workflow in &agent.workflows {
            if workflow.unmeasured > 0 {
                writeln!(
                    out,
                    "\n- **{}** costs {} more call(s) this run did not issue: {}.",
                    workflow.name, workflow.unmeasured, workflow.unmeasured_note
                )?;
            }
        }
    } else {
        writeln!(out, "\n## Agent surface (`canvas mcp`)\n")?;
        writeln!(
            out,
            "Not measured in this run. `cargo xtask bench --mcp` adds the \
             schema cost per tool, the warm `todo.list` round trip over \
             stdio, and the round trips per skill workflow."
        )?;
    }

    writeln!(out, "\n## Method\n")?;
    writeln!(
        out,
        "- `wiremock` serves the `{}` fixture set: {} courses, their \
         assignments, files, folders, modules, and grading periods, plus the \
         planner and missing-submission lists.{}",
        options.fixture,
        shape.courses,
        if shape.courses == 5 {
            ""
        } else {
            " SPEC §13 fixes the shape at five courses; this set does not \
             match it, so the numbers are not comparable with a 5-course run."
        }
    )?;
    writeln!(
        out,
        "- The cache is primed with `canvas sync` and one online `canvas todo`, \
         which is the dataset pair the measured command reads."
    )?;
    writeln!(
        out,
        "- Each measured run is `canvas todo --offline`, timed with \
         `std::time::Instant`. First output is the time until the first byte \
         reaches stdout; the full run is the time until the process exits. \
         Percentiles are nearest-rank."
    )?;
    writeln!(
        out,
        "- A synthetic fixture set carries a record date, and every timestamp \
         it serves is shifted by whole days to that date, so the planner window \
         holds the same workload whenever the benchmark runs."
    )?;
    writeln!(
        out,
        "- Each group discards {} warm-up runs. The first run after a build \
         pays for a cold binary and a cold directory, which is not the steady \
         state §13 describes.",
        Harness::WARMUP
    )?;
    writeln!(
        out,
        "- The download load is one `canvas download` of {} files, restarted \
         whenever it finishes, so a transfer is in flight for every run in the \
         group. It is run once to completion before the measurements, so a \
         download that fails cannot be mistaken for an idle server.",
        DOWNLOAD_FILES.len()
    )?;

    writeln!(out, "\n## Limitations\n")?;
    writeln!(
        out,
        "- **Cold start is a lower bound.** Dropping the operating-system page \
         cache needs root. Each cold run instead gets a fresh copy of the cache \
         files and a new process, so the process, the SQLite connection, and \
         the SQLite page cache are cold, but the file bytes can still be in the \
         operating-system cache."
    )?;
    writeln!(
        out,
        "- **The concurrent download uses a debug build.** SPEC §11 refuses a \
         transfer over plain `http` in a release build, and `wiremock` serves \
         no TLS. The measured `todo` runs use the release binary; only the \
         download that loads the server alongside them is a debug build with \
         `CANVAS_TEST_ALLOW_HTTP=1`."
    )?;
    if shape.extra_pages > 0 {
        writeln!(
            out,
            "- **Only the first page of each endpoint is served.** A recorded \
             `Link` header points at the host it was recorded from, so replaying \
             it would send the client off-origin; the header is dropped. This \
             set holds {} page(s) beyond the first, and those were not served, \
             so the workload is smaller than the set.",
            shape.extra_pages
        )?;
    }
    writeln!(
        out,
        "- **The numbers are local.** They measure this machine with a local \
         mock server and no network latency. They bound the client's own work, \
         not a session against a real Canvas instance."
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_document_describes_the_set_it_actually_served() {
        let options = Options {
            fixture: "recorded-3".to_owned(),
            runs: 3,
            mcp: false,
            no_fail: false,
            watch: false,
            doc: None,
        };
        let shape = SetShape {
            courses: 3,
            extra_pages: 2,
        };
        let empty = Measurements {
            targets: Vec::new(),
            tick: None,
        };
        let doc = document(&options, &shape, &empty, None, true).unwrap();
        assert!(doc.contains("Courses in set | 3"), "{doc}");
        assert!(doc.contains("fixture set: 3 courses"), "{doc}");
        assert!(doc.contains("not comparable with a 5-course run"), "{doc}");
        assert!(doc.contains("Only the first page"), "{doc}");
    }

    #[test]
    fn the_shape_is_read_from_the_set() {
        let make = |path: &str, page: Option<u32>, body: Value| {
            (
                path.to_owned(),
                Recorded {
                    method: "GET".into(),
                    path: path.into(),
                    query: vec![],
                    status: 200,
                    headers: BTreeMap::new(),
                    body,
                    page,
                },
            )
        };
        let entries = vec![
            make("/api/v1/courses", Some(1), json!([{"id": 1}, {"id": 2}])),
            make("/api/v1/courses", Some(2), json!([{"id": 3}])),
            make("/api/v1/users/self", None, json!({"id": 9})),
        ];
        let shape = SetShape::of(&entries);
        // The count comes from the first page, not from every page summed.
        assert_eq!(shape.courses, 2);
        assert_eq!(shape.extra_pages, 1);
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let samples = [10.0, 20.0, 30.0, 40.0];
        assert!((percentile(&samples, 0.50) - 20.0).abs() < f64::EPSILON);
        assert!((percentile(&samples, 0.95) - 40.0).abs() < f64::EPSILON);
        assert!((percentile(&[7.0], 0.95) - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_shift_moves_timestamps_and_leaves_everything_else_alone() {
        let body = json!({
            "due_at": "2026-01-05T12:00:00Z",
            "name": "Assignment 1",
            "id": 5,
            "points_possible": 20.0,
            "nested": [{"start_date": "2026-01-05T12:00:00Z"}]
        });
        let out = shift_days(&body, 10);
        assert_eq!(out["due_at"], json!("2026-01-15T12:00:00Z"));
        assert_eq!(
            out["nested"][0]["start_date"],
            json!("2026-01-15T12:00:00Z")
        );
        assert_eq!(out["name"], json!("Assignment 1"));
        assert_eq!(out["id"], json!(5));
        assert_eq!(out["points_possible"], json!(20.0));
        // A zero shift is the identity.
        assert_eq!(shift_days(&body, 0), body);
    }

    #[test]
    fn a_missed_target_is_reported_for_either_percentile() {
        let make = |p50: f64, p95: f64| Measured {
            label: "cached todo, first output",
            load: Load::Idle,
            p50,
            p95,
            target_p50: Some(50.0),
            target_p95: 150.0,
        };
        assert!(!make(10.0, 20.0).missed());
        assert!(make(60.0, 20.0).missed());
        assert!(make(10.0, 200.0).missed());
    }

    #[test]
    fn the_document_records_the_run_and_both_limitations() {
        let options = Options {
            fixture: DEFAULT_SET.to_owned(),
            runs: 3,
            mcp: false,
            no_fail: false,
            watch: false,
            doc: None,
        };
        let measurements = Measurements {
            targets: vec![Measured {
                label: "cached todo, full run",
                load: Load::Download,
                p50: 12.0,
                p95: 18.0,
                target_p50: None,
                target_p95: 250.0,
            }],
            tick: None,
        };
        let shape = SetShape {
            courses: 5,
            extra_pages: 0,
        };
        let doc = document(&options, &shape, &measurements, None, true).unwrap();
        assert!(doc.contains("# Benchmarks"));
        assert!(doc.contains("bench-5"));
        assert!(doc.contains("Courses in set | 5"));
        assert!(doc.contains("fixture set: 5 courses"), "{doc}");
        assert!(!doc.contains("Only the first page"), "{doc}");
        assert!(doc.contains("Runs per metric | 3"));
        assert!(doc.contains("Cold start is a lower bound"));
        assert!(doc.contains("debug build"));
        assert!(doc.contains("cached todo, full run | download | 12.0 | 18.0"));
    }

    #[test]
    fn the_watch_section_appears_only_when_watch_was_measured() {
        let options = Options {
            fixture: DEFAULT_SET.to_owned(),
            runs: 3,
            mcp: false,
            no_fail: false,
            watch: true,
            doc: None,
        };
        let measurements = Measurements {
            targets: vec![Measured {
                label: "cached todo, full run",
                load: Load::Watch,
                p50: 14.0,
                p95: 22.0,
                target_p50: None,
                target_p95: 250.0,
            }],
            tick: Some(Tick {
                p50: 90.0,
                p95: 130.0,
                events: 2,
                requests: 6,
            }),
        };
        let shape = SetShape {
            courses: 5,
            extra_pages: 0,
        };
        let doc = document(&options, &shape, &measurements, None, true).unwrap();
        assert!(doc.contains("## Watch"), "{doc}");
        assert!(
            doc.contains("watch tick, full refresh | 90.0 | 130.0"),
            "{doc}"
        );
        assert!(doc.contains("6 API request(s) and streamed 2"), "{doc}");
        assert!(
            doc.contains("cached todo, full run | watch | 14.0 | 22.0"),
            "{doc}"
        );

        let quiet = Options {
            watch: false,
            ..options
        };
        let without = Measurements {
            targets: Vec::new(),
            tick: None,
        };
        let doc = document(&quiet, &shape, &without, None, true).unwrap();
        assert!(doc.contains("Not measured in this run."), "{doc}");
        assert!(!doc.contains("watch tick, full refresh"), "{doc}");
    }
}

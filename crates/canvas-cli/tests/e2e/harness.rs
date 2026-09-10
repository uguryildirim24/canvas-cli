//! Shared environment, Canvas fixture server, and snapshot helper.
//!
//! Every test builds its own [`E2e`]: a private config root, a private data
//! root, the file credential store, and the frozen presentation environment
//! SPEC §16 row 3 requires (`COLUMNS=100`, `--color never`, `TZ` fixed,
//! `CANVAS_NOW` frozen).

#![allow(dead_code)]

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use canvas_core::test_scratch::Scratch;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The instant every snapshot is taken at.
pub const NOW: &str = "2026-09-09T17:05:12Z";
/// Fixed zone, so `ts+local` fields and human dates are stable.
pub const TZ: &str = "America/New_York";
/// The token the fixture server accepts.
pub const TOKEN: &str = "e2e-fixture-token";
/// The user id in `user.json`.
pub const USER_ID: i64 = 1;
/// The course id in `course.json`.
pub const COURSE_ID: i64 = 100;
/// The assignment id in `assignment.json`.
pub const ASSIGNMENT_ID: i64 = 9;
/// Mount priority for a per-test route override (lower wins in `wiremock`).
const OVERRIDE_PRIORITY: u8 = 1;

/// One `crates/canvas-api/tests/fixtures/` document.
macro_rules! fixture {
    ($name:literal) => {
        serde_json::from_str::<Value>(include_str!(concat!(
            "../../../canvas-api/tests/fixtures/",
            $name
        )))
        .expect(concat!($name, " is valid JSON"))
    };
}

/// Every Canvas fixture the suite serves, parsed once per call.
pub struct Fixtures;

impl Fixtures {
    pub fn user() -> Value {
        fixture!("user.json")
    }
    pub fn course() -> Value {
        fixture!("course.json")
    }
    pub fn term() -> Value {
        fixture!("term.json")
    }
    pub fn assignment() -> Value {
        fixture!("assignment.json")
    }
    pub fn submission() -> Value {
        fixture!("submission.json")
    }
    pub fn announcement() -> Value {
        fixture!("announcement.json")
    }
    pub fn calendar_event() -> Value {
        fixture!("calendar.json")
    }
    pub fn module() -> Value {
        fixture!("module.json")
    }
    pub fn module_item() -> Value {
        fixture!("module_item.json")
    }
    pub fn file() -> Value {
        fixture!("file.json")
    }
    pub fn folder() -> Value {
        fixture!("folder.json")
    }
    pub fn planner() -> Value {
        fixture!("planner.json")
    }
    pub fn missing() -> Value {
        fixture!("missing.json")
    }
    pub fn enrollment() -> Value {
        fixture!("enrollment.json")
    }
    pub fn assignment_group() -> Value {
        fixture!("assignment_group.json")
    }
    pub fn grading_period() -> Value {
        fixture!("grading_period.json")
    }
}

/// A `wiremock` Canvas built from the shipped fixtures (Appendix B).
///
/// The shipped routes all carry the default priority. A test that needs a
/// different answer for one route calls an `override_*` method, which mounts at
/// a higher priority; `wiremock` otherwise answers with the first match in
/// mount order, so a plain second mount would never be reached.
pub struct CanvasServer {
    pub server: MockServer,
}

impl CanvasServer {
    /// Start a server with every read endpoint the v1 commands use.
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let this = Self { server };
        this.mount_identity().await;
        this.mount_courses().await;
        this.mount_assignments().await;
        this.mount_planner().await;
        this.mount_grades().await;
        this.mount_files_and_modules().await;
        this.mount_announcements_and_calendar().await;
        this.mount_submit().await;
        this
    }

    pub fn uri(&self) -> String {
        self.server.uri().trim_end_matches('/').to_owned()
    }

    async fn json(&self, route: &str, body: Value) {
        Mock::given(method("GET"))
            .and(path(route.to_owned()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    async fn mount_identity(&self) {
        self.json("/api/v1/users/self", Fixtures::user()).await;
    }

    async fn mount_courses(&self) {
        let mut course = Fixtures::course();
        course["term"] = Fixtures::term();
        course["html_url"] = json!(format!("{}/courses/{COURSE_ID}", self.uri()));
        course["teachers"] = json!([{ "id": "55", "display_name": "Grace Hopper" }]);
        self.json("/api/v1/courses", json!([course.clone()])).await;
        self.json(&format!("/api/v1/courses/{COURSE_ID}"), course)
            .await;
    }

    async fn mount_assignments(&self) {
        let mut assignment = Fixtures::assignment();
        assignment["html_url"] = json!(format!(
            "{}/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}",
            self.uri()
        ));
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/assignments"),
            json!([assignment.clone()]),
        )
        .await;
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}"),
            assignment,
        )
        .await;
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions/self"),
            Fixtures::submission(),
        )
        .await;
    }

    async fn mount_planner(&self) {
        let mut planner = Fixtures::planner();
        planner["plannable"]["due_at"] = json!("2026-09-20T03:59:00Z");
        self.json("/api/v1/planner/items", json!([planner])).await;
        self.json(
            "/api/v1/users/self/missing_submissions",
            json!([Fixtures::missing()]),
        )
        .await;
    }

    async fn mount_grades(&self) {
        self.json(
            "/api/v1/users/self/enrollments",
            json!([Fixtures::enrollment()]),
        )
        .await;
        let mut group = Fixtures::assignment_group();
        group["assignments"] = json!([Fixtures::assignment()]);
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/assignment_groups"),
            json!([group]),
        )
        .await;
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/grading_periods"),
            json!({ "grading_periods": [Fixtures::grading_period()] }),
        )
        .await;
    }

    async fn mount_files_and_modules(&self) {
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/folders"),
            json!([Fixtures::folder()]),
        )
        .await;
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/files"),
            json!([Fixtures::file()]),
        )
        .await;
        let module = Fixtures::module();
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/modules"),
            json!([module.clone()]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/courses/\d+/modules/\d+/items$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([Fixtures::module_item()])),
            )
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/files/\d+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Fixtures::file()))
            .mount(&self.server)
            .await;
    }

    async fn mount_announcements_and_calendar(&self) {
        let mut announcement = Fixtures::announcement();
        announcement["html_url"] = json!(format!(
            "{}/courses/{COURSE_ID}/discussion_topics/40",
            self.uri()
        ));
        self.json("/api/v1/announcements", json!([announcement.clone()]))
            .await;
        self.json(
            &format!("/api/v1/courses/{COURSE_ID}/discussion_topics/40"),
            announcement,
        )
        .await;
        self.json(
            "/api/v1/calendar_events",
            json!([Fixtures::calendar_event()]),
        )
        .await;
    }

    /// Accept a text or URL submission `POST` and answer with the attempt.
    ///
    /// The answer echoes the body that was sent: `submit` records the digest of
    /// the body Canvas reports back, and `submission verify` later compares it
    /// with what it sent, so a canned response would make every verify report
    /// `unavailable`.
    async fn mount_submit(&self) {
        Mock::given(method("POST"))
            .and(path_regex(
                r"^/api/v1/courses/\d+/assignments/\d+/submissions$",
            ))
            .respond_with(EchoSubmission)
            .mount(&self.server)
            .await;
    }

    /// Let assignment 9 take a text submission with no attempts used.
    ///
    /// The shipped fixture is upload-only with one attempt already spent, so a
    /// `submit` test would otherwise never reach the `POST`.
    pub async fn allow_text_submission(&self) {
        let mut assignment = Fixtures::assignment();
        assignment["submission_types"] = json!(["online_text_entry"]);
        assignment["allowed_extensions"] = json!([]);
        assignment["submission"] = json!({ "attempt": 0 });
        assignment["html_url"] = json!(format!(
            "{}/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}",
            self.uri()
        ));
        self.override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}"),
            200,
            assignment,
        )
        .await;
    }

    /// Answer the readback route with the attempt the CLI already posted.
    ///
    /// `submission verify` reads the submission again; without this the shipped
    /// upload fixture would answer, and no history entry would match.
    pub async fn echo_posted_submission(&self) {
        let requests = self.server.received_requests().await.expect("recording");
        let posted = requests
            .iter()
            .rev()
            .find(|r| r.method == wiremock::http::Method::POST)
            .expect("the CLI posted a submission");
        let sent: Value = serde_json::from_slice(&posted.body).expect("the POST body is JSON");
        self.override_get(
            &format!("/api/v1/courses/{COURSE_ID}/assignments/{ASSIGNMENT_ID}/submissions/self"),
            200,
            posted_attempt(&sent["submission"]),
        )
        .await;
    }

    /// Replace the submission `POST` for the rest of this test.
    pub async fn override_post(&self, route: &str, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(route.to_owned()))
            .respond_with(template)
            .with_priority(OVERRIDE_PRIORITY)
            .mount(&self.server)
            .await;
    }

    /// Replace one route for the rest of this test.
    pub async fn override_get(&self, route: &str, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(route.to_owned()))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .with_priority(OVERRIDE_PRIORITY)
            .mount(&self.server)
            .await;
    }

    /// Replace one route with a status and no decodable body.
    pub async fn override_status(&self, route: &str, status: u16) {
        Mock::given(method("GET"))
            .and(path(route.to_owned()))
            .respond_with(ResponseTemplate::new(status))
            .with_priority(OVERRIDE_PRIORITY)
            .mount(&self.server)
            .await;
    }

    /// Answer a route only when a query parameter is present.
    pub async fn override_with_query(
        &self,
        route: &str,
        key: &'static str,
        value: &'static str,
        status: u16,
        body: Value,
    ) {
        Mock::given(method("GET"))
            .and(path(route.to_owned()))
            .and(query_param(key, value))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .with_priority(OVERRIDE_PRIORITY)
            .mount(&self.server)
            .await;
    }
}

/// Answers a submission `POST` with the attempt it describes.
struct EchoSubmission;

impl wiremock::Respond for EchoSubmission {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let sent: Value = serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({}));
        ResponseTemplate::new(200).set_body_json(posted_attempt(&sent["submission"]))
    }
}

/// The submission Canvas would report after accepting `submission`.
fn posted_attempt(submission: &Value) -> Value {
    let entry = json!({
        "id": 77,
        "attempt": 1,
        "submitted_at": NOW,
        "workflow_state": "submitted",
        "submission_type": submission["submission_type"],
        "body": submission["body"],
        "url": submission["url"],
        "attachments": [],
    });
    json!({
        "id": 77,
        "assignment_id": ASSIGNMENT_ID,
        "user_id": USER_ID,
        "attempt": 1,
        "submitted_at": NOW,
        "workflow_state": "submitted",
        "late": false,
        "missing": false,
        "submission_type": submission["submission_type"],
        "body": submission["body"],
        "url": submission["url"],
        "attachments": [],
        "submission_comments": [],
        "submission_history": [entry],
    })
}

/// One invocation's captured output.
#[derive(Debug, Clone)]
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// The single JSON document a `--json` invocation must print (§7).
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not one JSON document ({e}): stdout={:?} stderr={:?}",
                self.stdout, self.stderr
            )
        })
    }

    pub fn assert_code(&self, expected: i32) -> &Self {
        assert_eq!(
            self.code, expected,
            "exit {} != {expected}\nstdout={}\nstderr={}",
            self.code, self.stdout, self.stderr
        );
        self
    }
}

/// An isolated CLI environment for one test.
pub struct E2e {
    scratch: Scratch,
    masks: RefCell<Vec<(String, String)>>,
    config_dir: PathBuf,
    data_dir: PathBuf,
    origin: Option<String>,
    pub identity: Option<IdentityDocument>,
}

impl E2e {
    /// A bare environment: no identity, no server.
    pub fn new() -> Self {
        let scratch = Scratch::new("canvas-e2e");
        let config_dir = scratch.join("config");
        let data_dir = scratch.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        Self {
            scratch,
            masks: RefCell::new(Vec::new()),
            config_dir,
            data_dir,
            origin: None,
            identity: None,
        }
    }

    /// An environment with an identity for `origin` and a validated token.
    pub fn with_identity(origin: &str) -> Self {
        let mut env = Self::new();
        env.install_identity(origin, USER_ID, true);
        env
    }

    /// An environment bound to a running fixture server.
    pub fn with_server(server: &CanvasServer) -> Self {
        Self::with_identity(&server.uri())
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn scratch_path(&self) -> &Path {
        self.scratch.as_ref()
    }

    pub fn identity_key(&self) -> String {
        self.identity
            .as_ref()
            .expect("identity installed")
            .key
            .to_string()
    }

    pub fn paths(&self) -> Paths {
        Paths::for_identity(&self.data_dir, &self.identity.as_ref().unwrap().key)
    }

    /// Create the identity directory, optionally recording a validated token.
    pub fn install_identity(&mut self, origin: &str, user_id: i64, validated: bool) {
        let doc = IdentityDocument::new(origin, user_id, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(&self.data_dir, &doc.key);
        std::fs::create_dir_all(&paths.identity_dir).unwrap();
        std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        if validated {
            let key = doc.key.to_string();
            open.store
                .call_blocking(move |conns| {
                    conns.state.execute(
                        "INSERT INTO credential (identity_key, token_sha256, validated_at, active_source)
                         VALUES (?1, ?2, '2026-01-01T00:00:00Z', 'file')",
                        rusqlite::params![key, token_hash()],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        drop(open);
        self.origin = Some(origin.to_owned());
        self.identity = Some(doc);
    }

    /// Open the identity store for direct seeding.
    pub fn open(&self) -> OpenIdentity {
        let doc = self.identity.as_ref().expect("identity installed");
        OpenIdentity::open(&Paths::for_identity(&self.data_dir, &doc.key), doc).unwrap()
    }

    /// Write `config.toml` verbatim.
    pub fn write_config(&self, contents: &str) {
        std::fs::write(self.config_dir.join("config.toml"), contents).unwrap();
    }

    /// Point `default_profile` at the installed identity.
    pub fn set_default_profile(&self, name: &str) {
        let doc = self.identity.as_ref().expect("identity installed");
        self.write_config(&format!(
            "default_profile = \"{name}\"\n\n[profiles.{name}]\norigin = \"{}\"\nuser_id = {}\nkey = \"{}\"\n",
            doc.origin,
            doc.user_id,
            doc.key.as_str(),
        ));
    }

    /// A `canvas` command with the frozen presentation environment.
    ///
    /// Both config variables are set: `CANVAS_CONFIG_DIR` for the paths module
    /// and `XDG_CONFIG_HOME` so the base strategy cannot reach the real home.
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_canvas"));
        command
            .env("CANVAS_CONFIG_DIR", &self.config_dir)
            .env("CANVAS_DATA_DIR", &self.data_dir)
            .env("CANVAS_DATA_ROOT", &self.data_dir)
            .env("XDG_CONFIG_HOME", self.scratch.join("xdg-config"))
            .env("XDG_DATA_HOME", self.scratch.join("xdg-data"))
            .env("XDG_CACHE_HOME", self.scratch.join("xdg-cache"))
            .env("HOME", self.scratch_path())
            .env("CANVAS_NOW", NOW)
            .env("TZ", TZ)
            .env("COLUMNS", "100")
            .env("NO_COLOR", "1")
            .env("CANVAS_TEST_FORCE_FILE", "1")
            .env("CANVAS_TEST_ALLOW_HTTP", "1")
            .env("CANVAS_TEST_NO_LAUNCH", "1")
            .env("EDITOR", "/usr/bin/true")
            .env_remove("CANVAS_TOKEN")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE")
            .env_remove("CANVAS_IDENTITY_KEY")
            .env_remove("CANVAS_TEST_KEYRING_ERROR")
            .env_remove("CANVAS_TEST_CRASH_AFTER")
            .env_remove("CLICOLOR_FORCE");
        if let Some(doc) = &self.identity {
            command.env("CANVAS_IDENTITY_KEY", doc.key.as_str());
        }
        command
    }

    /// Run `canvas` with the token the fixture server accepts.
    pub fn run(&self, args: &[&str]) -> Run {
        self.run_with(args, |command| {
            command.env("CANVAS_TOKEN", TOKEN);
        })
    }

    /// Run `canvas` without a token (class A and B paths).
    pub fn run_local(&self, args: &[&str]) -> Run {
        self.run_with(args, |_| {})
    }

    /// Run `canvas` after applying `configure` to the command.
    pub fn run_with(&self, args: &[&str], configure: impl FnOnce(&mut Command)) -> Run {
        let mut command = self.command();
        command.args(args).args(["--color", "never"]);
        configure(&mut command);
        let output = command.output().expect("canvas runs");
        Run {
            code: output.status.code().expect("canvas exits with a code"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Run without appending `--color never` (raw-output commands).
    pub fn run_raw(&self, args: &[&str]) -> Run {
        let mut command = self.command();
        command.args(args).env("CANVAS_TOKEN", TOKEN);
        let output = command.output().expect("canvas runs");
        Run {
            code: output.status.code().expect("canvas exits with a code"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Run `canvas` with extra environment variables.
    pub fn run_env(&self, args: &[&str], vars: &[(&str, &str)]) -> Run {
        let owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        self.run_with(args, move |command| {
            for (key, value) in owned {
                command.env(key, value);
            }
        })
    }

    /// Run `canvas` with `input` on stdin and the fixture token in the env.
    pub fn run_stdin(&self, args: &[&str], input: &str) -> Run {
        self.run_stdin_with(args, input, |command| {
            command.env("CANVAS_TOKEN", TOKEN);
        })
    }

    /// Run `canvas` with `input` on stdin and no token (the login path).
    pub fn run_stdin_local(&self, args: &[&str], input: &str) -> Run {
        self.run_stdin_with(args, input, |_| {})
    }

    fn run_stdin_with(
        &self,
        args: &[&str],
        input: &str,
        configure: impl FnOnce(&mut Command),
    ) -> Run {
        let mut command = self.command();
        command
            .args(args)
            .args(["--color", "never"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure(&mut command);
        let mut child = command.spawn().expect("canvas starts");
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(input.as_bytes())
            .expect("stdin accepts the body");
        let output = child.wait_with_output().expect("canvas exits");
        Run {
            code: output.status.code().expect("canvas exits with a code"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Write `body` into the scratch directory and return its path.
    pub fn write_file(&self, name: &str, body: &[u8]) -> PathBuf {
        let path = self.scratch.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Replace every environment-specific string with a stable placeholder.
    /// Mask an origin this environment did not install itself.
    ///
    /// `auth login` creates the identity inside the child process, so the
    /// fixture server's port only becomes snapshottable once the test names it.
    pub fn track_origin(&self, origin: &str) {
        self.mask(origin, "<origin>");
        if let Some(hostless) = origin.strip_prefix("http://") {
            self.mask(hostless, "<host>");
        }
    }

    /// Mask the identity key `auth login` just created.
    pub fn track_logged_in_identity(&self) {
        let listed = self.run_local(&["identity", "list", "--json"]);
        listed.assert_code(0);
        for entry in listed.json()["result"]["identities"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(key) = entry["key"].as_str() {
                self.mask(key, "<identity>");
            }
        }
    }

    /// Replace `value` with `placeholder` in every later snapshot.
    ///
    /// Receipt, journal and attempt identifiers are generated per run, so the
    /// test that creates one records it here before it is snapshotted.
    pub fn mask(&self, value: &str, placeholder: &str) {
        self.masks
            .borrow_mut()
            .push((value.to_owned(), placeholder.to_owned()));
    }

    pub fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (value, placeholder) in self.masks.borrow().iter() {
            out = out.replace(value, placeholder);
        }
        if let Some(origin) = &self.origin {
            out = out.replace(origin, "<origin>");
            if let Some(hostless) = origin.strip_prefix("http://") {
                out = out.replace(hostless, "<host>");
            }
        }
        if let Some(doc) = &self.identity {
            out = out.replace(doc.key.as_str(), "<identity>");
        }
        for path in [self.scratch_path(), &self.config_dir, &self.data_dir] {
            if let Some(text) = path.to_str() {
                out = out.replace(text, "<scratch>");
            }
        }
        out = mask_number_after(&out, "size=");
        out = mask_number_after(&out, "seconds=");
        out = out
            .replace(TOKEN, "<token>")
            .replace(env!("CANVAS_BUILD_TARGET"), "<target>")
            .replace(env!("CARGO_PKG_VERSION"), "<version>");
        match option_env!("CANVAS_COMMIT") {
            Some(commit) => out.replace(commit, "<commit>"),
            None => out,
        }
    }

    /// Snapshot stdout, stderr and the exit code of one invocation.
    pub fn snapshot(&self, name: &str, run: &Run) {
        let body = format!(
            "exit: {}\n--- stdout ---\n{}--- stderr ---\n{}",
            run.code,
            ensure_newline(&self.scrub(&run.stdout)),
            ensure_newline(&self.scrub(&run.stderr)),
        );
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!(name, body);
        });
    }

    /// Snapshot a `--json` invocation's envelope, with volatile fields scrubbed.
    pub fn snapshot_json(&self, name: &str, run: &Run) -> Value {
        let mut value = run.json();
        scrub_json(&mut value, self);
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
            insta::assert_json_snapshot!(name, value);
        });
        run.json()
    }
}

/// Replace the digits that follow every `prefix` with `<n>`.
///
/// The identity listing prints the `SQLite` file size, which moves with page
/// allocation; `doctor` prints the clock skew against the fixture server, which
/// moves with the wall clock.
fn mask_number_after(text: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let (head, tail) = rest.split_at(at + prefix.len());
        out.push_str(head);
        let body = tail.strip_prefix('-').unwrap_or(tail);
        let sign = tail.len() - body.len();
        let digits =
            sign + body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == sign {
            rest = tail;
        } else {
            out.push_str("<n>");
            rest = &tail[digits..];
        }
    }
    out.push_str(rest);
    out
}

fn ensure_newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}\n")
    }
}

/// Keys whose value is generated per run and can never be snapshotted.
///
/// `journal_id` and `receipt_id` are UUIDs. `created_at` and `updated_at` are
/// journal row timestamps: `canvas-core` stamps them from the wall clock, so
/// `CANVAS_NOW` does not reach them.
const VOLATILE_KEYS: [(&str, &str); 4] = [
    ("journal_id", "<id>"),
    ("receipt_id", "<id>"),
    ("created_at", "<clock>"),
    ("updated_at", "<clock>"),
];

/// Numeric keys whose value is a filesystem measurement, not a command result.
const VOLATILE_NUMBER_KEYS: [&str; 1] = ["size_bytes"];

/// Replace ids and paths that change between runs.
///
/// A value the test masked by name keeps that mask; anything else under a
/// volatile key becomes `<id>`.
fn scrub_json(value: &mut Value, env: &E2e) {
    match value {
        Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                let volatile = VOLATILE_KEYS.iter().find(|(name, _)| *name == key.as_str());
                match (entry.as_str(), volatile) {
                    (Some(text), Some((_, placeholder))) => {
                        let scrubbed = env.scrub(text);
                        *entry = if scrubbed == text {
                            json!(placeholder)
                        } else {
                            json!(scrubbed)
                        };
                    }
                    _ if VOLATILE_NUMBER_KEYS.contains(&key.as_str()) && entry.is_number() => {
                        *entry = json!("<n>");
                    }
                    _ => scrub_json(entry, env),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scrub_json(item, env);
            }
        }
        Value::String(text) => *text = env.scrub(text),
        _ => {}
    }
}

/// SHA-256 of the fixture token, as the credential row records it.
pub fn token_hash() -> String {
    format!("{:x}", Sha256::digest(TOKEN.as_bytes()))
}

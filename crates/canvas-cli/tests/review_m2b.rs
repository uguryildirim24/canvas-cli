//! End-to-end M2-b regressions. Each invocation has isolated identity/config data.
use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
    submit::MAX_COMMENT_CHARS,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, process::Command};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const TOKEN: &str = "review-secret-token";
struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}
impl Fixture {
    fn new(origin: &str, validated: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new(origin, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        if validated {
            let key = doc.key.to_string();
            open.store.call_blocking(move |conns| {
                conns.state.execute("INSERT INTO credential (identity_key,token_sha256,validated_at) VALUES (?1,?2,'2026-01-01T00:00:00Z')", rusqlite::params![key, format!("{:x}",Sha256::digest(TOKEN.as_bytes()))])?;
                Ok(())
            }).unwrap();
        }
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
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["exit"], code);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
        v
    }
}
impl Fixture {
    fn store(&self) -> (Paths, OpenIdentity) {
        let paths = Paths::for_identity(self.dir.path().join("data"), &self.doc.key);
        let open = OpenIdentity::open(&paths, &self.doc).unwrap();
        (paths, open)
    }
    async fn human(&self, args: &[&str], code: i32) -> String {
        let mut cmd = self.command();
        cmd.args(args).args(["--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
async fn mock_assignment(server: &MockServer, status: u16) {
    Mock::given(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({"id":2,"name":"HW","submission_types":["online_text_entry","online_upload","online_url"],"can_submit":true,"due_at":"2020-01-01T00:00:00Z","submission":{"attempt":0}}))).mount(server).await;
}
async fn mock_history(server: &MockServer, body: Value) {
    Mock::given(path("/api/v1/courses/1/assignments/2/submissions/self")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"attempt":1,"submission_history":[{"id":55,"attempt":1,"submitted_at":"2026-09-09T17:05:12Z","body":body,"attachments":[]}]}))).mount(server).await;
}
/// Every M2-b registry fixture describes the fields its live command emits.
fn assert_fixtures_match_live(snapshots: &[Value], receipt: &Value) {
    let live = |name: &str| -> Value {
        snapshots
            .iter()
            .find(|s| s["command"] == name)
            .expect("captured command")["json"]["result"]
            .clone()
    };
    for (schema, fixture, result) in [
        (
            "submit@1",
            include_str!("../src/output/schemas/submit.json"),
            live("submit"),
        ),
        (
            "verify@1",
            include_str!("../src/output/schemas/verify.json"),
            live("verify"),
        ),
        (
            "reconcile@1",
            include_str!("../src/output/schemas/reconcile.json"),
            live("reconcile"),
        ),
        (
            "receipts@1",
            include_str!("../src/output/schemas/receipts.json"),
            live("list"),
        ),
        (
            "receipt@1",
            include_str!("../src/output/schemas/receipt.json"),
            receipt.clone(),
        ),
    ] {
        let fixture: Value = serde_json::from_str(fixture).unwrap();
        assert_eq!(keys(&fixture), keys(&result), "{schema} result fields");
        assert_eq!(
            keys(&fixture["posted"]),
            keys(&result["posted"]),
            "{schema} Posted fields"
        );
        assert_eq!(
            keys(&fixture["journals"][0]),
            keys(&result["journals"][0]),
            "{schema} Journal fields"
        );
    }
}

/// Field names of a JSON object, for fixture and live-output agreement.
fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn normalize(value: &mut Value, jid: &str, rid: &str, root: &str) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key == "bytes" {
                    assert!(v.as_u64().is_some_and(|n| n > 0));
                    *v = json!("<byte-count>");
                } else if matches!(
                    key.as_str(),
                    "created_at" | "updated_at" | "acknowledged_at"
                ) && !v.is_null()
                {
                    *v = json!("<time>");
                } else if key == "plan_id" && !v.is_null() {
                    *v = json!("<plan>");
                } else if key == "approval" && !v.is_null() {
                    // The audit is real; its digest and timestamp vary per run.
                    v["at"] = json!("<time>");
                    v["plan_sha256"] = json!("<digest>");
                } else if key == "origin" {
                    *v = json!("<origin>");
                } else if key == "key" {
                    *v = json!("<identity>");
                } else {
                    normalize(v, jid, rid, root);
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                normalize(v, jid, rid, root);
            }
        }
        Value::String(s) => {
            *s = s
                .replace(jid, "<journal>")
                .replace(rid, "<receipt>")
                .replace(root, "<root>");
        }
        _ => {}
    }
}
#[tokio::test]
async fn live_commands_have_complete_json_and_human_snapshots() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    mock_assignment(&server, 200).await;
    Mock::given(method("POST")).and(path("/api/v1/courses/1/assignments/2/submissions")).respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":55,"attempt":1,"submitted_at":"2026-09-09T17:05:12Z","body":"<p>hello</p>"}))).mount(&server).await;
    mock_history(&server, json!("<p>hello</p>")).await;
    let submitted = f
        .run(
            &[
                "submit",
                "1",
                "2",
                "--text",
                input.to_str().unwrap(),
                "--yes",
            ],
            0,
        )
        .await;
    assert_eq!(submitted["result"]["posted"]["attempt"], 1);
    assert!(
        submitted["result"]["posted"]["submitted_at_local"]
            .as_str()
            .unwrap()
            .contains("-04:00")
    );
    let jid = submitted["result"]["journal_id"].as_str().unwrap();
    let rid = submitted["result"]["receipt_id"].as_str().unwrap();
    let mut snapshots = vec![];
    snapshots.push(json!({"command":"submit","json":submitted}));
    for (name, args) in [
        ("list", vec!["receipts", "list"]),
        ("show", vec!["receipts", "show", rid]),
        ("export", vec!["receipts", "export", rid]),
        ("verify", vec!["submission", "verify", rid]),
        ("reconcile", vec!["submission", "reconcile", jid]),
    ] {
        let json = f.run(&args, 0).await;
        let human = f.human(&args, 0).await;
        snapshots.push(json!({"command":name,"json":json,"human":human}));
    }
    let raw = f
        .command()
        .args(["receipts", "export", rid, "--out", "-"])
        .output()
        .unwrap();
    assert!(raw.status.success());
    let receipt: Value = serde_json::from_slice(&raw.stdout).unwrap();
    assert_eq!(receipt["receipt_id"], rid);
    snapshots.push(json!({"command":"receipt@1 export","json":receipt}));

    assert_fixtures_match_live(&snapshots, &receipt);

    let mut snapshot = json!(snapshots);
    normalize(&mut snapshot, jid, rid, f.dir.path().to_str().unwrap());
    let snapshot: Value = serde_json::from_str(
        &snapshot
            .to_string()
            .replace(f.doc.key.as_str(), "<identity>"),
    )
    .unwrap();
    insta::assert_json_snapshot!("m2b_live_outputs", snapshot);
    // Class B resolves a URL and alias locally even with an unusable token.
    let (paths, open) = f.store();
    open.store
        .call_blocking(|c| {
            canvas_core::resolve::alias_set(&c.state, "chem", 1).unwrap();
            Ok(())
        })
        .unwrap();
    for course in ["chem".to_owned(), format!("{}/courses/1", server.uri())] {
        let output = f
            .command()
            .env("CANVAS_TOKEN", "bad\nheader")
            .args(["receipts", "list", "--course", &course, "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["requests"]["api"], 0);
    }
    assert!(paths.identity_dir.join("receipts").exists());
}
#[tokio::test]
async fn submit_validation_refusal_and_persistence_exits_are_real() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    mock_assignment(&server, 200).await;
    // One character over the limit, and ASCII. Linux refuses any single
    // `execve` argument of 128 KiB or more, so a multi-byte comment this long
    // never reaches the binary at all — the test would fail in the spawn
    // rather than on the validation it is here to check. That the limit counts
    // characters and not bytes is proved in `canvas-core`, where the string
    // does not have to survive a kernel argument list.
    let long = "x".repeat(MAX_COMMENT_CHARS + 1);
    let usage = f
        .run(
            &[
                "submit",
                "1",
                "2",
                "--text",
                input.to_str().unwrap(),
                "--comment",
                &long,
                "--yes",
            ],
            2,
        )
        .await;
    assert_eq!(usage["result"]["code"], "usage");
    assert_eq!(
        f.store()
            .1
            .store
            .call_blocking(|c| Ok(c.state.query_row(
                "SELECT COUNT(*) FROM submission_journal",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        0
    );
    server.reset().await;
    Mock::given(path("/api/v1/courses/1/assignments/2")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"submission_types":["online_text_entry"],"can_submit":false,"lock_explanation":"closed"}))).mount(&server).await;
    let refusal = f
        .run(&["submit", "1", "2", "--text", "/absent/input", "--yes"], 8)
        .await;
    assert_eq!(refusal["result"]["message"], "closed");
    server.reset().await;
    mock_assignment(&server, 200).await;
    let (paths, open) = f.store();
    open.store.call_blocking(|c|{c.state.execute_batch("CREATE TRIGGER refuse_journal BEFORE INSERT ON submission_journal BEGIN SELECT RAISE(ABORT,'injected persistence failure'); END;")?;Ok(())}).unwrap();
    drop(open);
    let failed = f
        .run(
            &[
                "submit",
                "1",
                "2",
                "--text",
                input.to_str().unwrap(),
                "--yes",
            ],
            13,
        )
        .await;
    assert_eq!(failed["schema"], "canvas-cli/error@1");
    assert!(
        fs::read_dir(paths.identity_dir.join("journals"))
            .unwrap()
            .count()
            > 0
    );
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.method == "POST")
    );
    let mut snapshots = json!([usage, refusal, failed]);
    normalize(
        &mut snapshots,
        "unused-journal",
        "unused-receipt",
        f.dir.path().to_str().unwrap(),
    );
    insta::assert_json_snapshot!("m2b_validation_exits", snapshots);
}
#[tokio::test]
async fn error_responses_stay_unknown_and_acknowledge_is_local() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    mock_assignment(&server, 200).await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"errors":["comment refused"]})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/1/assignments/2/submissions/self"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"attempt":0,"submission_history":[]})),
        )
        .mount(&server)
        .await;
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    let result = f
        .run(
            &[
                "submit",
                "1",
                "2",
                "--text",
                input.to_str().unwrap(),
                "--yes",
            ],
            9,
        )
        .await;
    assert_eq!(result["result"]["post_status"], 400);
    assert_eq!(result["result"]["response_kind"], "canvas-error");
    assert!(result["result"]["error"].is_string());
    let jid = result["result"]["journal_id"].as_str().unwrap();
    let mut snapshot = result.clone();
    normalize(
        &mut snapshot,
        jid,
        "unused-receipt",
        f.dir.path().to_str().unwrap(),
    );
    insta::assert_json_snapshot!("m2b_unknown_exit", snapshot);

    f.run(
        &["submission", "reconcile", jid, "--assume-not-submitted"],
        8,
    )
    .await;
    f.run(&["receipts", "export", jid], 8).await;
    let ack = f
        .run(&["receipts", "acknowledge", jid, "--offline"], 0)
        .await;
    assert_eq!(ack["requests"]["api"], 0);
    let shown = f.run(&["receipts", "show", jid, "--offline"], 0).await;
    assert_eq!(shown["result"]["journal"]["state"], "outcome_unknown");
    assert!(shown["result"]["journal"]["acknowledged_at"].is_string());
    assert!(
        !f.store()
            .1
            .store
            .call_blocking(|c| canvas_core::store::pending_for_assignment(&c.state, 2))
            .unwrap()
    );
}
#[tokio::test]
async fn verify_body_exit_codes_and_identity_validation() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    mock_assignment(&server, 200).await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"attempt":1,"body":"<p>hello</p>"})),
        )
        .mount(&server)
        .await;
    mock_history(&server, json!("<p>hello</p>")).await;
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    let result = f
        .run(
            &[
                "submit",
                "1",
                "2",
                "--text",
                input.to_str().unwrap(),
                "--yes",
            ],
            0,
        )
        .await;
    let rid = result["result"]["receipt_id"].as_str().unwrap();
    let mut outcomes = vec![];
    for (body, code) in [(json!("changed"), 10), (Value::Null, 12)] {
        server.reset().await;
        mock_history(&server, body).await;
        outcomes.push(f.run(&["submission", "verify", rid], code).await);
    }
    let mut snapshots = json!(outcomes);
    normalize(
        &mut snapshots,
        result["result"]["journal_id"].as_str().unwrap(),
        rid,
        f.dir.path().to_str().unwrap(),
    );
    insta::assert_json_snapshot!("m2b_verify_exits", snapshots);
    let (_, open) = f.store();
    open.store
        .call_blocking(|c| {
            c.state
                .execute("UPDATE credential SET token_sha256=NULL", [])?;
            Ok(())
        })
        .unwrap();
    drop(open);
    server.reset().await;
    Mock::given(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":999})))
        .expect(1)
        .mount(&server)
        .await;
    f.run(&["submission", "verify", rid], 3).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_no_and_ctrl_c_cancel_before_any_write() {
    const DRIVER: &str = r"
import os, pty, select, signal, sys, time
mode, program, *args = sys.argv[1:]
pid, master = pty.fork()
if pid == 0:
    os.execv(program, [program] + args)
data = bytearray()
sent = False
deadline = time.monotonic() + 15
while time.monotonic() < deadline:
    ready, _, _ = select.select([master], [], [], 0.2)
    if not ready:
        continue
    try:
        part = os.read(master, 65536)
    except OSError:
        break
    if not part:
        break
    data.extend(part)
    if not sent and b'Submit? [y/N]' in data:
        sent = True
        if mode == 'interrupt':
            os.kill(pid, signal.SIGINT)
        else:
            os.write(master, b'n\n')
else:
    os.kill(pid, signal.SIGKILL)
_, status = os.waitpid(pid, 0)
sys.stdout.buffer.write(data)
sys.exit(os.waitstatus_to_exitcode(status))
";
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri(), true);
    mock_assignment(&server, 200).await;
    let input = f.dir.path().join("text");
    fs::write(&input, b"hello").unwrap();
    for mode in ["no", "interrupt"] {
        let base = f.command();
        let mut cmd = Command::new("python3");
        for (key, value) in base.get_envs() {
            if let Some(value) = value {
                cmd.env(key, value);
            } else {
                cmd.env_remove(key);
            }
        }
        cmd.args([
            "-c",
            DRIVER,
            mode,
            env!("CARGO_BIN_EXE_canvas"),
            "submit",
            "1",
            "2",
            "--text",
            input.to_str().unwrap(),
            "--json",
        ]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(11),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("submission cancelled"));
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .find_map(|line| line.find("{\"schema\"").map(|at| &line[at..]))
            .unwrap();
        let mut value: Value = serde_json::from_str(line).unwrap();
        normalize(
            &mut value,
            "unused-journal",
            "unused-receipt",
            f.dir.path().to_str().unwrap(),
        );
        insta::assert_json_snapshot!("m2b_cancelled_exit", value);
    }
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.method == "POST")
    );
}

#[tokio::test]
async fn preflight_network_failure_has_no_journal() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let f = Fixture::new(&format!("http://{address}"), true);
    let mut result = f
        .run(&["submit", "1", "2", "--text", "unread-input", "--yes"], 4)
        .await;
    normalize(
        &mut result,
        "unused-journal",
        "unused-receipt",
        f.dir.path().to_str().unwrap(),
    );
    insta::assert_json_snapshot!("m2b_network_exit", result);
    assert_eq!(
        f.store()
            .1
            .store
            .call_blocking(|c| Ok(c.state.query_row(
                "SELECT COUNT(*) FROM submission_journal",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        0
    );
}

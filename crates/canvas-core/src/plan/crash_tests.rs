//! Real subprocesses at the approval-link boundary and during approval waits.
//!
//! The helper below is re-invoked as a child process, so the kills are real
//! kills and the two concurrent executes are two operating-system processes,
//! not two tasks sharing one connection pool.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::identity::{IdentityDocument, Paths};
use crate::journal::{AdmissionLock, LockError, get_journal};
use crate::store::{OpenIdentity, Store};

use super::execute::{Linked, link};
use super::record::PlanState;
use super::tests::{setup, test_client};
use super::{ApprovalChannel, approve, execute, issue_handle, prepare, require};

/// Publish a handshake file atomically, so the parent never reads a half file.
fn publish(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, contents).unwrap();
    std::fs::rename(&temporary, path).unwrap();
}

fn open(root: &Path) -> (Paths, Store) {
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(root, &doc.key);
    let stored = IdentityDocument::read(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &stored).unwrap().store;
    (paths, store)
}

#[test]
fn helper() {
    let Ok(root) = std::env::var("CANVAS_PLAN_ROOT") else {
        return;
    };
    let (paths, store) = open(Path::new(&root));
    let ready = std::env::var("CANVAS_PLAN_READY").unwrap();
    let mode = std::env::var("CANVAS_PLAN_MODE").unwrap_or_default();

    if mode == "probe" {
        let answer = match AdmissionLock::try_acquire(&paths.identity_dir, 2) {
            Ok(_lock) => "free",
            Err(LockError::InProgress) => "held",
            Err(e) => panic!("{e}"),
        };
        publish(ready, answer);
        return;
    }

    let plan_id = std::env::var("CANVAS_PLAN_ID").unwrap();
    if mode == "link" {
        // The local half of execute. `CANVAS_JOURNAL_PHASE` parks this process
        // inside or just after the transaction so the parent can kill it.
        let plan = require(&store, &plan_id).unwrap();
        let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
        match link(&store, &paths.identity_dir, &admission, &plan).unwrap() {
            Linked::Created(journal_id, _owner) => publish(ready, &journal_id),
            Linked::Existing(journal_id) => publish(ready, &journal_id),
        }
        return;
    }

    assert_eq!(mode, "execute", "unknown helper mode {mode}");
    let key = IdentityDocument::read(&paths.identity_json()).unwrap().key;
    let client = test_client(&std::env::var("CANVAS_PLAN_ORIGIN").unwrap());
    // Every racer starts together.
    publish(&ready, "waiting");
    while !Path::new(&root).join("start").exists() {
        std::thread::sleep(Duration::from_millis(2));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let answer = runtime.block_on(execute(
        &client,
        &store,
        &paths.identity_dir,
        key.as_str(),
        &plan_id,
        Timestamp::now(),
    ));
    match answer {
        Ok(super::Admission::Created { journal_id, .. }) => {
            publish(ready, &format!("created:{journal_id}"));
        }
        Ok(super::Admission::Existing { journal_id }) => {
            publish(ready, &format!("existing:{journal_id}"));
        }
        Err(e) => publish(ready, &format!("error:{e}")),
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(root: &Path, ready: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "plan::crash_tests::helper", "--nocapture"])
        .env("CANVAS_PLAN_ROOT", root)
        .env("CANVAS_PLAN_READY", ready)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn wait_for(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && !text.is_empty()
        {
            return text;
        }
        assert!(Instant::now() < deadline, "child never published {path:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

async fn mount_assignment(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/1/assignments/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 2, "name": "HW",
            "submission_types": ["online_text_entry"],
            "can_submit": true, "submission": { "attempt": 0 }, "allowed_attempts": -1
        })))
        .mount(server)
        .await;
}

/// Prepare and approve one plan, and return its id.
async fn approved(store: &Store, paths: &Paths, doc: &IdentityDocument, origin: &str) -> String {
    let client = test_client(origin);
    let now = Timestamp::now();
    let prepared = prepare(
        &client,
        store,
        &super::PrepareRequest {
            identity_dir: &paths.identity_dir,
            identity_key: doc.key.as_str(),
            consumer: None,
            course_id: 1,
            assignment_id: 2,
            course_code: None,
            kind: crate::submit::InputKind::OnlineTextEntry,
        },
        move || {
            let bytes = b"hello".to_vec();
            crate::submit::freeze_text(&crate::submit::TextSource::Bytes(&bytes), None)
        },
        now,
    )
    .await
    .unwrap();
    let plan_id = prepared.plan.plan_id.clone();
    let handle = issue_handle(store, &plan_id, None).unwrap();
    approve(store, &plan_id, &handle, ApprovalChannel::Tty, None, now).unwrap();
    plan_id
}

#[tokio::test]
async fn a_kill_at_the_approval_link_transaction_leaves_no_half_state() {
    // `inserted` parks inside the transaction; `published` parks just after the
    // commit. Nothing else is possible in between.
    for phase in ["inserted", "published"] {
        let (dir, paths, open_identity, doc) = setup();
        let server = MockServer::start().await;
        mount_assignment(&server).await;
        let plan_id = approved(&open_identity.store, &paths, &doc, &server.uri()).await;

        let ready = dir.path().join("ready");
        let mut child = Process(
            command(dir.path(), &ready)
                .env("CANVAS_PLAN_MODE", "link")
                .env("CANVAS_PLAN_ID", &plan_id)
                .env("CANVAS_JOURNAL_PHASE", phase)
                .env("CANVAS_JOURNAL_READY", &ready)
                .spawn()
                .unwrap(),
        );
        let journal_id = wait_for(&ready);
        child.0.kill().unwrap();
        child.0.wait().unwrap();

        let plan = require(&open_identity.store, &plan_id).unwrap();
        let journal = get_journal(&open_identity.store, &journal_id).unwrap();
        if phase == "inserted" {
            assert!(journal.is_none(), "an uncommitted journal survived");
            assert_eq!(plan.state, PlanState::Approved);
            assert_eq!(plan.journal_id, None);
        } else {
            let journal = journal.expect("a committed journal is missing");
            assert_eq!(journal.plan_id.as_deref(), Some(plan_id.as_str()));
            assert!(journal.approval_json.is_some());
            assert_eq!(plan.state, PlanState::Executed);
            assert_eq!(plan.journal_id.as_deref(), Some(journal_id.as_str()));
        }
        let journals: i64 = open_identity
            .store
            .call_blocking(|c| {
                Ok(c.state
                    .query_row("SELECT COUNT(*) FROM submission_journal", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(journals, i64::from(phase == "published"), "{phase}");
    }
}

#[tokio::test]
async fn concurrent_executes_and_a_replay_create_exactly_one_journal() {
    let (dir, paths, open_identity, doc) = setup();
    let server = MockServer::start().await;
    mount_assignment(&server).await;
    let plan_id = approved(&open_identity.store, &paths, &doc, &server.uri()).await;

    let a_ready = dir.path().join("a");
    let b_ready = dir.path().join("b");
    let mut racers = Vec::new();
    for ready in [&a_ready, &b_ready] {
        racers.push(Process(
            command(dir.path(), ready)
                .env("CANVAS_PLAN_MODE", "execute")
                .env("CANVAS_PLAN_ID", &plan_id)
                .env("CANVAS_PLAN_ORIGIN", server.uri())
                .spawn()
                .unwrap(),
        ));
    }
    assert_eq!(wait_for(&a_ready), "waiting");
    assert_eq!(wait_for(&b_ready), "waiting");
    publish(dir.path().join("start"), "go");

    let deadline = Instant::now() + Duration::from_secs(30);
    let (a, b) = loop {
        let a = std::fs::read_to_string(&a_ready).unwrap();
        let b = std::fs::read_to_string(&b_ready).unwrap();
        if a != "waiting" && b != "waiting" {
            break (a, b);
        }
        assert!(Instant::now() < deadline, "a={a}, b={b}");
        std::thread::sleep(Duration::from_millis(10));
    };
    for racer in &mut racers {
        assert!(racer.0.wait().unwrap().success());
    }

    // One created the journal; the other found the same one.
    let created: Vec<&str> = [a.as_str(), b.as_str()]
        .into_iter()
        .filter_map(|answer| answer.strip_prefix("created:"))
        .collect();
    assert_eq!(created.len(), 1, "a={a}, b={b}");
    let journal_id = created[0];
    let existing: Vec<&str> = [a.as_str(), b.as_str()]
        .into_iter()
        .filter_map(|answer| answer.strip_prefix("existing:"))
        .collect();
    assert_eq!(existing, vec![journal_id], "a={a}, b={b}");

    // A replayed approval, in a third process, adds nothing.
    let c_ready = dir.path().join("c");
    publish(dir.path().join("start"), "go");
    assert!(
        command(dir.path(), &c_ready)
            .env("CANVAS_PLAN_MODE", "execute")
            .env("CANVAS_PLAN_ID", &plan_id)
            .env("CANVAS_PLAN_ORIGIN", server.uri())
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(wait_for(&c_ready), format!("existing:{journal_id}"));

    let journals: i64 = open_identity
        .store
        .call_blocking(|c| {
            Ok(c.state
                .query_row("SELECT COUNT(*) FROM submission_journal", [], |r| r.get(0))?)
        })
        .unwrap();
    assert_eq!(journals, 1, "one plan admits exactly one journal");
    assert_eq!(
        require(&open_identity.store, &plan_id)
            .unwrap()
            .journal_id
            .as_deref(),
        Some(journal_id)
    );
}

#[tokio::test]
async fn no_lock_is_held_while_a_plan_waits_for_approval() {
    let (dir, paths, open_identity, doc) = setup();
    let server = MockServer::start().await;
    mount_assignment(&server).await;
    let client = test_client(&server.uri());

    let prepared = prepare(
        &client,
        &open_identity.store,
        &super::PrepareRequest {
            identity_dir: &paths.identity_dir,
            identity_key: doc.key.as_str(),
            consumer: None,
            course_id: 1,
            assignment_id: 2,
            course_code: None,
            kind: crate::submit::InputKind::OnlineTextEntry,
        },
        move || {
            let bytes = b"hello".to_vec();
            crate::submit::freeze_text(&crate::submit::TextSource::Bytes(&bytes), None)
        },
        Timestamp::now(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.plan.state, PlanState::Prepared);

    // The plan is waiting for a human; a second process finds admission free.
    let free = dir.path().join("free");
    assert!(
        command(dir.path(), &free)
            .env("CANVAS_PLAN_MODE", "probe")
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(wait_for(&free), "free");

    // The probe is real: a held lock reads as held.
    let held = dir.path().join("held");
    let admission = AdmissionLock::try_acquire(&paths.identity_dir, 2).unwrap();
    assert!(
        command(dir.path(), &held)
            .env("CANVAS_PLAN_MODE", "probe")
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(wait_for(&held), "held");
    drop(admission);
}

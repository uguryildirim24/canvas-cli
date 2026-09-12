//! Crash checkpoints for the operation journal (M8-b).
//!
//! The same mechanism `journal::crash_tests` uses: a test process arms a
//! checkpoint name, and the code under test aborts the process the moment it
//! reaches it. A second process then reads the database and the locks, and
//! checks that owner-absent recovery gives the interrupted journal the only
//! honest answer it can have.

use std::sync::OnceLock;

/// The checkpoint this process is armed to die at, from the environment.
fn armed() -> Option<&'static str> {
    static ARMED: OnceLock<Option<String>> = OnceLock::new();
    ARMED
        .get_or_init(|| std::env::var("CANVAS_OPERATION_CRASH_AT").ok())
        .as_deref()
}

/// Abort immediately when this process is armed to die here.
///
/// `abort` and not `exit`: no destructor runs, so the owner lock is released
/// by the operating system exactly as it would be after a kill.
pub(crate) fn checkpoint(name: &str, detail: &str) {
    if armed() == Some(name) {
        eprintln!("crash at {name} {detail}");
        std::process::abort();
    }
}

#[cfg(test)]
mod kills {
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use jiff::Timestamp;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::identity::{IdentityDocument, Paths};
    use crate::store::{OpenIdentity, Store};

    use super::super::tests::{
        approved_quiz, approved_reply, mount_quiz, mount_quiz_answers, mount_quiz_complete,
        mount_topic, open_topic, test_client,
    };
    use super::super::{Admitted, NotPostedEvidence, OpState, execute, get, post, recover_active};

    /// Publish a handshake file atomically, so the parent never reads a half file.
    fn publish(path: impl AsRef<Path>, contents: &str) {
        let path = path.as_ref();
        let temporary = path.with_extension("partial");
        std::fs::write(&temporary, contents).unwrap();
        std::fs::rename(&temporary, path).unwrap();
    }

    fn open(root: &Path) -> (Paths, Store, IdentityDocument) {
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(root, &doc.key);
        let stored = IdentityDocument::read(&paths.identity_json()).unwrap();
        let store = OpenIdentity::open(&paths, &stored).unwrap().store;
        (paths, store, stored)
    }

    /// The child half of every kill below.
    ///
    /// It is this same test binary, re-invoked with `CANVAS_OPERATION_ROOT`
    /// set, so the kills are real kills of a real process: the owner lock is
    /// released by the operating system, exactly as after a crash.
    #[test]
    fn helper() {
        let Ok(root) = std::env::var("CANVAS_OPERATION_ROOT") else {
            return;
        };
        let (paths, store, doc) = open(Path::new(&root));
        let ready = std::env::var("CANVAS_OPERATION_READY").unwrap();
        let plan_id = std::env::var("CANVAS_OPERATION_PLAN").unwrap();
        let origin = std::env::var("CANVAS_OPERATION_ORIGIN").unwrap();
        let client = test_client(&origin);

        // Every racer starts together, so two executes really do race.
        publish(&ready, "waiting");
        while !Path::new(&root).join("start").exists() {
            std::thread::sleep(Duration::from_millis(2));
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let answer: Result<String, super::super::OperationError> = runtime.block_on(async {
            let admitted = execute(
                &client,
                &store,
                &paths.identity_dir,
                doc.key.as_str(),
                &plan_id,
                Timestamp::now(),
            )
            .await?;
            match admitted {
                Admitted::Existing { journal_id } => Ok(format!("existing:{journal_id}")),
                Admitted::Created { journal_id, owner } => {
                    // A crash checkpoint inside `post` aborts this process
                    // before anything is published.
                    post(&client, &store, &owner, &journal_id).await?;
                    Ok(format!("created:{journal_id}"))
                }
            }
        });
        match answer {
            Ok(text) => publish(ready, &text),
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

    fn command(root: &Path, ready: &Path, plan_id: &str, origin: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "operations::crash_tests::kills::helper",
                "--nocapture",
            ])
            .env("CANVAS_OPERATION_ROOT", root)
            .env("CANVAS_OPERATION_READY", ready)
            .env("CANVAS_OPERATION_PLAN", plan_id)
            .env("CANVAS_OPERATION_ORIGIN", origin)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
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

    async fn mount_all(server: &MockServer) {
        mount_topic(server, open_topic()).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/courses/5/discussion_topics/55/entries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 5003, "user_id": 7,
                "created_at": "2026-09-10T14:02:11Z",
                "message": "<p>My reply.</p>"
            })))
            .mount(server)
            .await;
    }

    /// A kill at every transition boundary, and the recovery each one implies.
    ///
    /// The table is the whole of owner-absent recovery for an operation:
    /// before the `POST` nothing was sent, so the journal is `refused` with
    /// `never_sent`; from the moment the request may have left, the only
    /// honest state is `outcome_unknown`.
    #[tokio::test]
    async fn a_kill_at_every_transition_recovers_to_the_state_it_implies() {
        for (checkpoint, expected, evidence) in [
            ("owner_acquired", None, None),
            ("inserted", None, None),
            (
                "published",
                Some(OpState::Refused),
                Some(NotPostedEvidence::NeverSent),
            ),
            ("posting", Some(OpState::OutcomeUnknown), None),
            ("response_received", Some(OpState::OutcomeUnknown), None),
        ] {
            let dir = tempfile::TempDir::new().unwrap();
            let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
            let paths = Paths::for_identity(dir.path(), &doc.key);
            std::fs::create_dir_all(&paths.identity_dir).unwrap();
            std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
            doc.write(&paths.identity_json()).unwrap();
            let open = OpenIdentity::open(&paths, &doc).unwrap();

            let server = MockServer::start().await;
            mount_all(&server).await;
            let client = test_client(&server.uri());
            let prepared = approved_reply(&client, &open.store, &paths, &doc).await;

            let ready = dir.path().join("ready");
            let mut child = Process(
                command(dir.path(), &ready, &prepared.plan.plan_id, &server.uri())
                    .env("CANVAS_OPERATION_CRASH_AT", checkpoint)
                    .spawn()
                    .unwrap(),
            );
            wait_for(&ready);
            std::fs::write(dir.path().join("start"), "go").unwrap();
            let status = child.0.wait().unwrap();
            assert!(!status.success(), "{checkpoint}: the child did not die");

            // The parent recovers what the dead process left.
            let recovered =
                recover_active(&open.store, &paths.identity_dir, &prepared.operation.target)
                    .unwrap();
            match expected {
                // The row was never published, so there is nothing to recover
                // and no journal exists.
                None => {
                    assert!(recovered.is_empty(), "{checkpoint}: {recovered:?}");
                    assert!(
                        super::super::for_plan(&open.store, &prepared.plan.plan_id)
                            .unwrap()
                            .is_none(),
                        "{checkpoint}: a journal exists"
                    );
                }
                Some(state) => {
                    assert_eq!(recovered.len(), 1, "{checkpoint}: {recovered:?}");
                    assert_eq!(recovered[0].1, state, "{checkpoint}");
                    let row = get(&open.store, &recovered[0].0).unwrap().unwrap();
                    assert_eq!(row.not_posted_evidence, evidence, "{checkpoint}");
                }
            }
        }
    }

    /// A kill between the quiz's two requests recovers to unknown.
    ///
    /// The answers POST answered 200 before the process died, so the journal
    /// names the session Canvas recorded them on — but the completion was
    /// never observed, and only reconcile can say whether it landed.
    #[tokio::test]
    async fn a_kill_between_the_quiz_posts_recovers_unknown() {
        for checkpoint in ["quiz_answers_received", "quiz_complete_received"] {
            let dir = tempfile::TempDir::new().unwrap();
            let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
            let paths = Paths::for_identity(dir.path(), &doc.key);
            std::fs::create_dir_all(&paths.identity_dir).unwrap();
            std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
            doc.write(&paths.identity_json()).unwrap();
            let open = OpenIdentity::open(&paths, &doc).unwrap();

            let server = MockServer::start().await;
            mount_quiz(&server).await;
            mount_quiz_answers(&server).await;
            mount_quiz_complete(&server).await;
            let client = test_client(&server.uri());
            let prepared = approved_quiz(&client, &open.store, &paths, &doc).await;

            let ready = dir.path().join("ready");
            let mut child = Process(
                command(dir.path(), &ready, &prepared.plan.plan_id, &server.uri())
                    .env("CANVAS_OPERATION_CRASH_AT", checkpoint)
                    .spawn()
                    .unwrap(),
            );
            wait_for(&ready);
            std::fs::write(dir.path().join("start"), "go").unwrap();
            let status = child.0.wait().unwrap();
            assert!(!status.success(), "{checkpoint}: the child did not die");

            let recovered =
                recover_active(&open.store, &paths.identity_dir, &prepared.operation.target)
                    .unwrap();
            assert_eq!(recovered.len(), 1, "{checkpoint}: {recovered:?}");
            assert_eq!(recovered[0].1, OpState::OutcomeUnknown, "{checkpoint}");
        }
    }

    /// Two processes execute the same approved plan at the same instant.
    ///
    /// One admits a journal, and the other gets that same journal. There is
    /// never a second journal, and Canvas is never asked twice.
    #[tokio::test]
    async fn two_racing_executes_produce_one_journal() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path(), &doc.key);
        std::fs::create_dir_all(&paths.identity_dir).unwrap();
        std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();

        let server = MockServer::start().await;
        mount_all(&server).await;
        let client = test_client(&server.uri());
        let prepared = approved_reply(&client, &open.store, &paths, &doc).await;
        drop(open);

        let first_ready = dir.path().join("ready-1");
        let second_ready = dir.path().join("ready-2");
        let mut first = Process(
            command(
                dir.path(),
                &first_ready,
                &prepared.plan.plan_id,
                &server.uri(),
            )
            .spawn()
            .unwrap(),
        );
        let mut second = Process(
            command(
                dir.path(),
                &second_ready,
                &prepared.plan.plan_id,
                &server.uri(),
            )
            .spawn()
            .unwrap(),
        );
        wait_for(&first_ready);
        wait_for(&second_ready);
        std::fs::write(dir.path().join("start"), "go").unwrap();
        first.0.wait().unwrap();
        second.0.wait().unwrap();

        let answers = [wait_for(&first_ready), wait_for(&second_ready)];
        let ids: Vec<&str> = answers
            .iter()
            .filter_map(|answer| {
                answer
                    .strip_prefix("created:")
                    .or_else(|| answer.strip_prefix("existing:"))
            })
            .collect();
        assert_eq!(ids.len(), 2, "{answers:?}");
        assert_eq!(ids[0], ids[1], "two journals for one plan: {answers:?}");

        let posts = server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.method.as_str() == "POST")
            .count();
        assert_eq!(posts, 1, "the plan was posted more than once");
    }
}

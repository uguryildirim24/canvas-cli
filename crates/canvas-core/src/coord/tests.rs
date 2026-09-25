//! Coordinator tests: slot caps, owner death, single-flight names, interest.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use canvas_api::{Governor, GovernorConfig, GovernorSnapshot, Lane, Seams};

use super::{FilePermits, InterestKind, refresh_lock_name};
use crate::identity::{IdentityDocument, Paths};
use crate::store::{OpenIdentity, Store};
use crate::test_scratch::Scratch;

fn identity(scratch: &Scratch) -> (Paths, IdentityDocument) {
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(scratch.as_ref(), &doc.key);
    std::fs::create_dir_all(&paths.identity_dir).unwrap();
    std::fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    (paths, doc)
}

fn open(scratch: &Scratch) -> (Paths, Store) {
    let (paths, doc) = identity(scratch);
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    (paths, store)
}

// ------------------------------------------------------------ lock naming

#[test]
fn distinct_dataset_scopes_never_share_a_lock_file() {
    // The pairs below are the shapes that could fold together under a naive
    // encoding: separator confusion, case folding on a case-insensitive
    // filesystem, path traversal, and the `~` the encoder itself uses.
    let pairs = [
        ("assignments", "course:1"),
        ("assignments", "course:10"),
        ("assignments", "course:1:"),
        ("assignments-course", "1"),
        ("assignments", "Course:1"),
        ("assignments", "COURSE:1"),
        ("assignments", "course~3a1"),
        ("assignments", "../../etc/passwd"),
        ("assignments", ""),
        ("", "assignments-course:1"),
        ("announcements", "window:2026-09-01..2026-09-14:ctx:abcdef"),
        ("announcements", "window:2026-09-01..2026-09-14:ctx:ABCDEF"),
        (
            "calendar_events",
            "window:2026-09-01..2026-09-14:ctx:abcdef",
        ),
        ("missing", "all"),
        ("missing", "all "),
        ("missing", "all\n"),
        ("missing", "AL/L"),
    ];
    let mut seen: HashSet<String> = HashSet::new();
    for (dataset, scope) in pairs {
        let name = refresh_lock_name(dataset, scope);
        assert!(
            seen.insert(name.to_ascii_lowercase()),
            "{dataset}:{scope} collides with an earlier name: {name}"
        );
        assert!(
            name.starts_with("refresh-")
                && std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|e| e == "lock"),
            "{name}"
        );
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._~".contains(&b)),
            "{name} is not a canonical file name"
        );
        // The `refresh-` prefix and the byte set above leave no path
        // separator, no device name, and no bare `.` or `..` component.
        assert!(!name.contains('/') && !name.contains('\\'), "{name}");
    }
}

#[test]
fn a_long_scope_falls_back_to_a_digest_that_still_separates_it() {
    let long = "x".repeat(400);
    let a = refresh_lock_name("announcements", &long);
    let b = refresh_lock_name("announcements", &format!("{long}y"));
    assert_ne!(a, b);
    assert!(a.len() <= 80, "{a}");
}

// ------------------------------------------------------------- API slots

#[tokio::test]
async fn api_slots_bound_admission_and_are_released_on_drop() {
    let scratch = Scratch::new("coord-slots");
    let permits = FilePermits::new(&scratch.as_ref().join("locks"), 2, 2).unwrap();
    assert_eq!(permits.api_slots(), 2);
    let governor = Governor::with_seams(
        GovernorConfig {
            api_concurrency: 8,
            storage_concurrency: 2,
            full_remaining: 700.0,
            jitter: false,
        },
        &Seams {
            permits: Some(std::sync::Arc::new(permits)),
            state: None,
        },
    );
    let first = governor.admit(Lane::Api, "GET /a").await;
    let second = governor.admit(Lane::Api, "GET /a").await;
    // The lane is full: a third admission cannot complete.
    let third = tokio::time::timeout(
        Duration::from_millis(150),
        governor.admit(Lane::Api, "GET /a"),
    )
    .await;
    assert!(
        third.is_err(),
        "a third request was admitted over two slots"
    );
    drop(first);
    let third =
        tokio::time::timeout(Duration::from_secs(5), governor.admit(Lane::Api, "GET /a")).await;
    assert!(third.is_ok(), "the released slot was not reused");
    drop(second);
}

#[tokio::test]
async fn storage_permits_stay_per_process() {
    let scratch = Scratch::new("coord-storage");
    let locks = scratch.as_ref().join("locks");
    let one = FilePermits::new(&locks, 1, 1).unwrap();
    let two = FilePermits::new(&locks, 1, 1).unwrap();
    // Two `FilePermits` over the same directory are two processes for the API
    // lane, but each keeps its own storage cap.
    let a = Governor::with_seams(
        GovernorConfig {
            api_concurrency: 1,
            storage_concurrency: 1,
            full_remaining: 700.0,
            jitter: false,
        },
        &Seams {
            permits: Some(std::sync::Arc::new(one)),
            state: None,
        },
    );
    let b = Governor::with_seams(
        GovernorConfig {
            api_concurrency: 1,
            storage_concurrency: 1,
            full_remaining: 700.0,
            jitter: false,
        },
        &Seams {
            permits: Some(std::sync::Arc::new(two)),
            state: None,
        },
    );
    let held_a = a.admit(Lane::Storage, "transfer:download").await;
    let held_b = tokio::time::timeout(
        Duration::from_millis(200),
        b.admit(Lane::Storage, "transfer:download"),
    )
    .await;
    assert!(
        held_b.is_ok(),
        "storage transfers were capped across processes"
    );
    // The API lane is capped across them.
    let api_a = a.admit(Lane::Api, "GET /a").await;
    let api_b =
        tokio::time::timeout(Duration::from_millis(200), b.admit(Lane::Api, "GET /a")).await;
    assert!(api_b.is_err(), "one API slot admitted two processes");
    drop((held_a, api_a));
}

// ------------------------------------------------- shared governor row

#[tokio::test]
async fn the_governor_row_survives_a_process_and_keeps_a_low_estimate() {
    let scratch = Scratch::new("coord-governor");
    let (paths, store) = open(&scratch);
    let coord = store.coordinator().clone();
    let config = GovernorConfig {
        api_concurrency: 2,
        storage_concurrency: 2,
        full_remaining: 700.0,
        jitter: false,
    };
    let first = Governor::with_seams(config.clone(), &coord.seams());
    let permit = first.admit(Lane::Api, "GET /a").await;
    first.apply_observation_for_test(permit.issue(), 210.0, None);
    drop(permit);
    drop(first);
    drop(store);

    // A second process opens the same identity and reads the row rather than
    // starting from a full bucket.
    let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    let second = Governor::with_seams(config, &store.coordinator().seams());
    let permit = second.admit(Lane::Api, "GET /b").await;
    assert!(second.estimate() < 215.0, "{}", second.estimate());
    drop(permit);
}

#[test]
fn a_vanished_owner_leaves_a_row_that_the_silence_rule_resets() {
    let scratch = Scratch::new("coord-silence");
    let (_paths, store) = open(&scratch);
    let coord = store.coordinator().clone();
    let seams = coord.seams();
    let shared = seams.state.clone().unwrap();
    // The owner died in cooldown with 40 remaining, a minute and a second ago.
    let stale = GovernorSnapshot {
        estimate: 40.0,
        watermark: 31,
        cooldown_until: Some(canvas_api::governor::unix_millis() + 5_000),
        refill: 2.0,
        updated_at: canvas_api::governor::unix_millis() - 61_000,
    };
    shared.update(&mut |_| Some(stale));
    assert_eq!(shared.load(), Some(stale));

    let governor = Governor::with_seams(
        GovernorConfig {
            api_concurrency: 2,
            storage_concurrency: 2,
            full_remaining: 700.0,
            jitter: false,
        },
        &seams,
    );
    governor.adopt_shared_for_test();
    // §11 and nothing more: the reset is the header-silence reset.
    assert!(governor.estimate() > 690.0, "{}", governor.estimate());
    assert_eq!(governor.watermark(), 0);

    // The same row, written a second ago, is adopted instead.
    let fresh = GovernorSnapshot {
        updated_at: canvas_api::governor::unix_millis() - 1_000,
        ..stale
    };
    shared.update(&mut |_| Some(fresh));
    let governor = Governor::with_seams(
        GovernorConfig {
            api_concurrency: 2,
            storage_concurrency: 2,
            full_remaining: 700.0,
            jitter: false,
        },
        &seams,
    );
    governor.adopt_shared_for_test();
    assert!(governor.estimate() < 50.0, "{}", governor.estimate());
}

// ------------------------------------------------------------- interest

#[test]
fn interest_is_visible_while_it_is_held_and_gone_when_it_is_dropped() {
    let scratch = Scratch::new("coord-interest");
    let (_paths, store) = open(&scratch);
    let coord = store.coordinator();
    assert!(!coord.foreground_interest().unwrap());
    let held = coord
        .register_interest(InterestKind::Submit, 42)
        .unwrap()
        .expect("interest registered");
    assert_eq!(held.assignment_id(), 42);
    assert!(coord.foreground_interest().unwrap());
    drop(held);
    assert!(!coord.foreground_interest().unwrap());
}

#[test]
fn a_dead_registrant_cannot_starve_polling() {
    let scratch = Scratch::new("coord-interest-death");
    let (paths, store) = open(&scratch);
    let mut child = Process(
        helper_command(paths.data_root.as_path(), "interest")
            .spawn()
            .unwrap(),
    );
    wait_for(&paths.data_root.join("ready"));
    // While the registrant lives, polling must stand aside.
    assert!(store.coordinator().foreground_interest().unwrap());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    // Its descriptor went with it, so the row is stale and is removed.
    assert!(!store.coordinator().foreground_interest().unwrap());
    assert_eq!(
        store
            .call_blocking(|conns| Ok(conns.state.query_row(
                "SELECT COUNT(*) FROM interest",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        0
    );
}

#[test]
fn a_dead_owner_releases_its_api_slot() {
    let scratch = Scratch::new("coord-slot-death");
    let (paths, _store) = open(&scratch);
    let locks = paths.identity_dir.join("locks");
    let mut child = Process(
        helper_command(paths.data_root.as_path(), "slot")
            .spawn()
            .unwrap(),
    );
    wait_for(&paths.data_root.join("ready"));
    let permits = FilePermits::new(&locks, 1, 1).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let governor = Governor::with_seams(
            GovernorConfig {
                api_concurrency: 1,
                storage_concurrency: 1,
                full_remaining: 700.0,
                jitter: false,
            },
            &Seams {
                permits: Some(std::sync::Arc::new(permits)),
                state: None,
            },
        );
        let blocked = tokio::time::timeout(
            Duration::from_millis(200),
            governor.admit(Lane::Api, "GET /a"),
        )
        .await;
        assert!(blocked.is_err(), "the child's slot was not exclusive");
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let taken =
            tokio::time::timeout(Duration::from_secs(5), governor.admit(Lane::Api, "GET /a")).await;
        assert!(taken.is_ok(), "a dead owner did not free its slot");
    });
}

// -------------------------------------------------------- helper process

/// The child half of the two cross-process tests.
///
/// It returns at once unless the parent named a mode, exactly as the M2-a
/// crash helper does, so the test binary can re-invoke itself.
#[test]
fn helper() {
    let Ok(root) = std::env::var("CANVAS_COORD_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let doc = IdentityDocument::new("https://canvas.example", 7, "2026-01-01T00:00:00Z");
    let paths = Paths::for_identity(&root, &doc.key);
    let doc = IdentityDocument::read(&paths.identity_json()).unwrap();
    let store = OpenIdentity::open(&paths, &doc).unwrap().store;
    let mode = std::env::var("CANVAS_COORD_MODE").unwrap_or_default();
    let _held: Box<dyn std::any::Any> = match mode.as_str() {
        "interest" => Box::new(
            store
                .coordinator()
                .register_interest(InterestKind::Submit, 42)
                .unwrap()
                .expect("child registers interest"),
        ),
        "slot" => {
            let permits = FilePermits::new(&paths.identity_dir.join("locks"), 1, 1).unwrap();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let slot = runtime.block_on(async move {
                let governor = Governor::with_seams(
                    GovernorConfig {
                        api_concurrency: 1,
                        storage_concurrency: 1,
                        full_remaining: 700.0,
                        jitter: false,
                    },
                    &Seams {
                        permits: Some(std::sync::Arc::new(permits)),
                        state: None,
                    },
                );
                governor.admit(Lane::Api, "GET /child").await
            });
            Box::new((slot, runtime))
        }
        other => panic!("unknown helper mode {other}"),
    };
    publish(&root.join("ready"), "ready");
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn helper_command(root: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "coord::tests::helper", "--nocapture"])
        .env("CANVAS_COORD_ROOT", root)
        .env("CANVAS_COORD_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

/// Publish a handshake file atomically, so the parent never reads it empty.
fn publish(path: &Path, contents: &str) {
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, contents).unwrap();
    std::fs::rename(&temporary, path).unwrap();
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

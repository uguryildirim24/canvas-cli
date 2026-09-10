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

//! Unique, self-cleaning temporary directories for tests.
//!
//! Test binaries run one test per process and several processes at once, and
//! they all share one system temp directory. A name built from the wall clock
//! alone is not unique there: `SystemTime::now()` advances in 1 µs steps on
//! macOS, and a process-local counter restarts at zero in every process, so
//! two processes that reach the helper in the same microsecond pick the same
//! path and `create_dir_all` hands both of them the same directory.
//!
//! [`Scratch`] adds the process id, so the name is unique by construction, and
//! removes the tree on drop instead of leaving it in the temp directory.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A temporary directory that is removed when it goes out of scope.
///
/// Dereferences to the `root` directory the test works in. Siblings of `root`
/// (for example the `root.identity` a destination helper derives) live inside
/// the same private base directory and are removed with it.
pub struct Scratch {
    base: PathBuf,
    root: PathBuf,
}

impl Scratch {
    /// Create `<temp>/<prefix>-<pid>-<micros>-<n>/root` and return it.
    pub fn new(prefix: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after the unix epoch")
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        Self { base, root }
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

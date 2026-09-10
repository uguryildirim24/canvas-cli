use std::path::Path;
use std::process::Command;

fn main() {
    println!(
        "cargo:rustc-env=CANVAS_BUILD_TARGET={}",
        std::env::var("TARGET").expect("Cargo sets TARGET")
    );
    // `version@1` reports `commit`. Prefer an explicit value (release builds
    // and source tarballs), then git, then leave it unset so it stays null.
    println!("cargo:rerun-if-env-changed=CANVAS_COMMIT");
    if let Some(commit) = env_commit().or_else(git_commit) {
        println!("cargo:rustc-env=CANVAS_COMMIT={commit}");
    }
}

fn env_commit() -> Option<String> {
    let value = std::env::var("CANVAS_COMMIT").ok()?;
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn git_commit() -> Option<String> {
    let commit = git(&["rev-parse", "--short=12", "HEAD"])?;
    watch_head();
    Some(commit)
}

/// Tell Cargo which files move when the stamped commit moves.
///
/// `HEAD` on its own is not enough: a commit on the current branch rewrites
/// the branch ref and leaves `HEAD` untouched, so the build script would not
/// rerun and the binary would keep reporting the commit before it. `git
/// rev-parse --git-path` also puts each file where this checkout actually
/// keeps it, which differs in a linked worktree: `HEAD` lives in the
/// worktree's own git directory, the branch ref in the common one.
///
/// Needs git 2.31 for `--path-format`. On anything older nothing is watched,
/// and Cargo falls back to rerunning whenever a file in the package changes.
fn watch_head() {
    watch(git_path("HEAD"));
    // A detached `HEAD` holds the commit itself and needs nothing more.
    if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        watch(git_path(&reference));
        // A branch whose ref has been packed has no loose file of its own.
        watch(git_path("packed-refs"));
    }
}

fn watch(path: Option<String>) {
    let Some(path) = path else { return };
    if Path::new(&path).exists() {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn git_path(name: &str) -> Option<String> {
    git(&["rev-parse", "--path-format=absolute", "--git-path", name])
}

/// Run `git` and return its trimmed stdout, or `None` when it is unusable.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

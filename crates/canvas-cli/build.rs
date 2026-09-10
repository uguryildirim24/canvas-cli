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
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    if commit.is_empty() {
        return None;
    }
    // Rebuild when HEAD moves, but only when a git dir is actually reachable.
    if let Some(dir) = git_dir() {
        println!("cargo:rerun-if-changed={dir}/HEAD");
    }
    Some(commit)
}

fn git_dir() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!dir.is_empty()).then_some(dir)
}

//! `dist-assets` must be reproducible: two runs, byte-identical output.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Every file under `dir`, keyed by its path relative to `dir`.
fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .map(|e| e.expect("dir entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let key = path
                    .strip_prefix(root)
                    .expect("entry is under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(key, std::fs::read(&path).expect("read asset"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn generate(out: &Path) {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["dist-assets", "--out"])
        .arg(out)
        .output()
        .expect("run xtask");
    assert!(
        result.status.success(),
        "dist-assets exited with {}: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn two_runs_produce_identical_assets() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    generate(first.path());
    generate(second.path());

    let a = read_tree(first.path());
    let b = read_tree(second.path());
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "two runs wrote different file sets"
    );
    for (name, bytes) in &a {
        assert_eq!(
            bytes,
            b.get(name).expect("same file set"),
            "{name} differs between runs"
        );
    }
}

#[test]
fn a_rerun_over_an_existing_directory_matches_a_fresh_one() {
    let reused = TempDir::new().unwrap();
    let fresh = TempDir::new().unwrap();
    generate(reused.path());
    // A stale page from a command that no longer exists must not survive.
    std::fs::write(reused.path().join("man").join("canvas-gone.1"), b"stale").unwrap();
    generate(reused.path());
    generate(fresh.path());
    assert_eq!(read_tree(reused.path()), read_tree(fresh.path()));
}

/// `--out` is an arbitrary path, so a run clears its own assets and nothing
/// else: pointed at a populated directory it must not take a user's files.
#[test]
fn a_rerun_leaves_files_it_did_not_write_alone() {
    let dir = TempDir::new().unwrap();
    generate(dir.path());
    let strangers = [
        dir.path().join("man").join("gzip.1.gz"),
        dir.path().join("completions").join("git"),
    ];
    for path in &strangers {
        std::fs::write(path, b"not ours").unwrap();
    }
    generate(dir.path());
    for path in &strangers {
        assert_eq!(
            std::fs::read(path).ok().as_deref(),
            Some(b"not ours".as_slice()),
            "{} was deleted or rewritten",
            path.display()
        );
    }
}

#[test]
fn assets_cover_every_command_and_every_shell() {
    let dir = TempDir::new().unwrap();
    generate(dir.path());
    let tree = read_tree(dir.path());

    for shell in canvas_cli::dist::SHELLS {
        let name = canvas_cli::dist::completion_file_name(*shell);
        let key = format!("completions/{name}");
        let script = tree
            .get(&key)
            .unwrap_or_else(|| panic!("missing {key}; wrote {:?}", tree.keys()));
        assert!(!script.is_empty(), "{key} is empty");
    }

    // One page for `canvas`, one for every command and subcommand.
    let mut expected = vec!["man/canvas.1".to_owned()];
    let mut root = canvas_cli::dist::command();
    root.build();
    collect_pages(&root, "canvas", &mut expected);
    assert!(expected.len() > 40, "expected the whole §5 surface");
    for page in expected {
        let content = tree
            .get(&page)
            .unwrap_or_else(|| panic!("missing {page}; wrote {:?}", tree.keys()));
        let text = String::from_utf8_lossy(content);
        assert!(text.contains(".TH "), "{page} has no man page header");
        assert!(text.contains(".SH NAME"), "{page} has no NAME section");
    }
    assert!(
        !tree.keys().any(|k| k.contains("help")),
        "clap's implicit help subcommand got a man page"
    );
}

fn collect_pages(cmd: &clap::Command, path: &str, out: &mut Vec<String>) {
    for sub in cmd
        .get_subcommands()
        .filter(|s| s.get_name() != "help" && !s.is_hide_set())
    {
        let child = format!("{path}-{}", sub.get_name());
        out.push(format!("man/{child}.1"));
        collect_pages(sub, &child, out);
    }
}

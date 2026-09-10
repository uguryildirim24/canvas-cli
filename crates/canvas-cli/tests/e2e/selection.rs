//! The SPEC §16 row 3 list, item by item.
//!
//! Most of the eleven named items were already asserted by the package that
//! introduced the behaviour; `docs/testing.md` maps every item to the test that
//! covers it, and [`every_row_3_item_names_a_test_that_exists`] keeps that map
//! honest. The two items with no end-to-end test yet are asserted here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use canvas_core::identity::IdentityKey;

use crate::harness::{CanvasServer, E2e};

/// Item 11: class B, an unbound env pair, and no default profile.
///
/// The pair names an origin the CLI has never validated, so there is no binding
/// to select an identity through and no profile to fall back on. SPEC §8 makes
/// that exit 3 with the hint that binds it.
#[tokio::test]
async fn class_b_command_with_an_unbound_env_pair_and_no_default_profile() {
    let server = CanvasServer::start().await;
    let env = E2e::new();
    env.track_origin(&server.uri());
    assert!(
        !env.config_dir().join("config.toml").exists(),
        "no config file, so no default profile"
    );

    // `alias list` is class B: identity-bound and local, never a network call.
    let run = env.run_env(
        &["alias", "list", "--json"],
        &[("CANVAS_HOST", &server.uri()), ("CANVAS_TOKEN", "unbound")],
    );
    run.assert_code(3);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/error@1");
    assert_eq!(value["result"]["code"], "auth");
    assert_eq!(
        value["requests"]["api"], 0,
        "a class-B command never opens a connection"
    );
    assert!(
        !env.data_dir().join("env-bindings.toml").exists(),
        "an unbound pair leaves no binding behind"
    );
    env.snapshot_json("class_b_unbound_env_pair", &run);

    // The same command with a default profile runs.
    let bound = E2e::with_server(&server);
    bound.set_default_profile("default");
    bound.run_local(&["alias", "list"]).assert_code(0);
}

/// Item 13: an IPv6 origin's identity key is a legal Windows path component.
///
/// `identity.rs::ipv6_origin_key` asserts the slug the key generator produces.
/// This adds the rule that slug exists for: the key names a directory, so it
/// must survive Windows' path grammar as well as POSIX's, on every platform
/// that can write a data root another platform will read.
#[tokio::test]
async fn ipv6_origin_identity_key_obeys_windows_path_rules() {
    const ORIGIN: &str = "https://[2001:db8::1]:8443";
    let key = IdentityKey::compute(ORIGIN, 7);
    let text = key.as_str();

    assert!(
        !text.contains(':'),
        "a colon makes the rest of a Windows path an alternate data stream: {text}"
    );
    let forbidden: BTreeSet<char> = r#"<>:"/\|?*"#.chars().collect();
    assert!(
        !text.chars().any(|c| forbidden.contains(&c)),
        "reserved Windows characters in {text}"
    );
    assert!(
        !text.chars().any(|c| (c as u32) < 0x20),
        "control characters in {text}"
    );
    assert!(
        !text.ends_with('.') && !text.ends_with(' '),
        "Windows silently trims a trailing dot or space: {text}"
    );
    assert!(
        !is_windows_device_name(text),
        "reserved Windows device name: {text}"
    );

    // The key really is usable as a directory, and the CLI reads it back.
    let mut env = E2e::new();
    env.install_identity(ORIGIN, 7, true);
    assert_eq!(env.identity_key(), text, "the CLI computes the same key");
    let listed = env.run_local(&["identity", "list", "--json"]);
    listed.assert_code(0);
    let value = listed.json();
    assert_eq!(value["result"]["identities"][0]["key"], text);
    assert_eq!(value["result"]["identities"][0]["origin"], ORIGIN);

    env.run_local(&["identity", "remove", text, "--yes"])
        .assert_code(0);
    let empty = env.run_local(&["identity", "list", "--json"]);
    empty.assert_code(0);
    assert_eq!(
        empty.json()["result"]["identities"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "the identity directory was removed"
    );
}

/// The same key, created as a real directory by the Windows path layer.
///
/// The rules test above is portable and runs everywhere, which is what keeps a
/// macOS or Linux developer from writing a key Windows cannot store. This one
/// is the proof on the platform that enforces them: Windows resolves device
/// names and trims trailing dots and spaces in the file system itself, so only
/// Windows can show that the component survives a create and a read back.
#[cfg(windows)]
#[test]
fn ipv6_origin_identity_key_is_a_real_windows_directory() {
    const ORIGIN: &str = "https://[2001:db8::1]:8443";
    let key = IdentityKey::compute(ORIGIN, 7);
    let scratch = canvas_core::test_scratch::Scratch::new("e2e-ipv6-key");
    let dir = scratch.as_ref().join(key.as_str());
    std::fs::create_dir_all(&dir).expect("Windows accepts the key as a path component");
    std::fs::write(dir.join("identity.json"), b"{}").expect("a file inside it");
    let found: Vec<String> = std::fs::read_dir(scratch.as_ref())
        .expect("the scratch root")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        found,
        vec![key.as_str().to_owned()],
        "Windows stored the component under the name it was given"
    );
}

/// True when the first path component names a Windows character device.
///
/// Windows resolves `CON`, `NUL`, `COM1`… as devices whatever the directory or
/// the extension, so a path component that starts with one cannot be created.
fn is_windows_device_name(text: &str) -> bool {
    let stem = text.split('.').next().unwrap_or(text).to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) {
        return true;
    }
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|rest| rest.len() == 1 && rest.starts_with(|c: char| c.is_ascii_digit()))
    };
    numbered("COM") || numbered("LPT")
}

/// `docs/testing.md` names a real test for every §16 row 3 item.
///
/// The table is the deliverable; this keeps it from rotting into a list of
/// functions that no longer exist.
#[test]
fn every_row_3_item_names_a_test_that_exists() {
    let doc = std::fs::read_to_string(repo_path("docs/testing.md")).expect("docs/testing.md");
    let table = doc
        .split_once("<!-- spec-16-row-3 -->")
        .expect("the coverage table is delimited")
        .1
        .split_once("<!-- /spec-16-row-3 -->")
        .expect("the coverage table is closed")
        .0;

    let mut items = 0;
    for row in table.lines().filter(|line| line.starts_with('|')) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        // Skip the header and the `---` separator.
        if cells.len() < 4 || cells[1].starts_with("---") || cells[1] == "Item" {
            continue;
        }
        items += 1;
        for reference in cells[2].split(',') {
            let reference = reference.trim().trim_matches('`');
            let (file, function) = reference
                .split_once("::")
                .unwrap_or_else(|| panic!("`{reference}` is not `file.rs::function`"));
            let path = repo_path(&format!("crates/canvas-cli/tests/{file}"));
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                source.contains(&format!("fn {function}(")),
                "{reference} names no test in {}",
                path.display()
            );
        }
    }
    assert_eq!(
        items, 11,
        "SPEC §16 row 3 names eleven items after the first two"
    );
}

/// A path inside the repository, from this crate's manifest directory.
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

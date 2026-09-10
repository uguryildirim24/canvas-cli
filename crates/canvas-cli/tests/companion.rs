//! The browser companion as a shipped artifact.
//!
//! What the broker does with the companion's messages is tested against a
//! live host in `tests/bridge.rs`, and the companion's own classifiers are
//! tested by `npm test` under `extension/`. This file pins the two things
//! neither of those can see: that the package ships, and that its manifest
//! asks for nothing more than the design allows.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Value {
    let raw = std::fs::read_to_string(root().join("extension/manifest.json"))
        .expect("the companion manifest ships");
    serde_json::from_str(&raw).expect("the manifest is JSON")
}

/// The release archives carry the companion, and the README points at it.
#[test]
fn the_companion_is_shipped_and_referenced() {
    let dist = std::fs::read_to_string(root().join("dist-workspace.toml")).expect("dist config");
    assert!(
        dist.contains("\"extension\""),
        "the release archives do not carry the companion"
    );
    let readme = std::fs::read_to_string(root().join("README.md")).expect("README");
    assert!(
        readme.contains("extension/"),
        "the README does not point at the companion"
    );
}

/// The companion asks for one tab on a gesture and a pipe, and nothing else.
///
/// `scripting` is the one permission beyond the two REPORT §3.3 names: Chrome
/// requires it for `chrome.scripting.executeScript` even under `activeTab`,
/// and it grants no host access of its own. `docs/companion.md` records the
/// deviation. Everything below would widen the reach and is refused here.
#[test]
fn the_manifest_asks_for_nothing_it_does_not_need() {
    let manifest = manifest();
    assert_eq!(manifest["manifest_version"], 3);
    let permissions: Vec<&str> = manifest["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().unwrap_or_default())
        .collect();
    assert_eq!(permissions, ["activeTab", "nativeMessaging", "scripting"]);
    for forbidden in [
        "cookies",
        "webRequest",
        "webRequestBlocking",
        "declarativeNetRequest",
        "history",
        "tabs",
        "storage",
        "downloads",
        "debugger",
        "<all_urls>",
    ] {
        assert!(
            !permissions.contains(&forbidden),
            "the companion asks for {forbidden}"
        );
    }
    // No standing access to any site: `activeTab` is granted per gesture.
    assert!(
        manifest.get("host_permissions").is_none(),
        "the companion holds standing host permissions"
    );
    assert!(
        manifest.get("content_scripts").is_none(),
        "the companion injects without a gesture"
    );
    // Nothing is fetched from anywhere but the page the person attached.
    assert!(manifest.get("externally_connectable").is_none());
    // The person's own act is the only way in: a toolbar click or a shortcut.
    assert!(manifest["action"].is_object(), "no toolbar action");
    assert!(
        manifest["commands"]["attach"].is_object(),
        "no keyboard shortcut"
    );
}

/// The package has no dependency at all, so nothing is installed to run it.
#[test]
fn the_companion_carries_no_dependency() {
    let raw = std::fs::read_to_string(root().join("extension/package.json"))
        .expect("the companion package ships");
    let package: Value = serde_json::from_str(&raw).expect("json");
    for field in ["dependencies", "devDependencies", "peerDependencies"] {
        assert!(
            package.get(field).is_none(),
            "the companion declares {field}"
        );
    }
    assert!(
        package["scripts"]["test"].is_string(),
        "the companion has no test script"
    );
}

/// Every file the background worker injects ships in the package.
#[test]
fn every_injected_file_exists() {
    let background = std::fs::read_to_string(root().join("extension/src/background.js"))
        .expect("the background worker ships");
    let mut found = 0;
    for span in background.split('"') {
        if span.starts_with("src/") && Path::new(span).extension().is_some_and(|e| e == "js") {
            assert!(
                root().join("extension").join(span).is_file(),
                "{span} is injected but does not ship"
            );
            found += 1;
        }
    }
    assert!(found >= 5, "the injected file list is {found} files long");
}

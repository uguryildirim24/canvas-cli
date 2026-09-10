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
/// `scripting` is one of two permissions beyond the two REPORT §3.3 names:
/// Chrome requires it for `chrome.scripting.executeScript` even under
/// `activeTab`, and it grants no host access of its own. `sidePanel` is the
/// other, added in M7-b: it opens the extension's own surface and reaches no
/// page at all. `docs/companion.md` records both. Everything below would
/// widen the reach and is refused here.
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
    assert_eq!(
        permissions,
        ["activeTab", "nativeMessaging", "scripting", "sidePanel"]
    );
    for forbidden in [
        "cookies",
        "webRequest",
        "webRequestBlocking",
        "declarativeNetRequest",
        "webNavigation",
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
    // The panel is a page of this extension, served from the package.
    let panel = manifest["side_panel"]["default_path"]
        .as_str()
        .expect("no side panel");
    assert!(
        root().join("extension").join(panel).is_file(),
        "{panel} is the panel but does not ship"
    );
}

/// The panel page loads only files that ship, and builds nothing from a
/// string.
///
/// A note is written by an agent that reads web pages. It reaches the panel
/// as Markdown source and reaches the document through `textContent`; an
/// `innerHTML` anywhere in this surface would undo every bound around it.
#[test]
fn the_panel_builds_no_markup_from_a_string() {
    let panel_dir = root().join("extension/src");
    let html = std::fs::read_to_string(panel_dir.join("panel.html")).expect("the panel ships");
    for span in html.split('"') {
        if Path::new(span).extension().is_some_and(|e| e == "js") {
            assert!(
                panel_dir.join(span).is_file(),
                "{span} is loaded by the panel but does not ship"
            );
        }
    }
    // Every file the panel page loads, taken from the page itself. A fixed
    // list here would go stale the moment `panel.html` gained a script, and
    // the file it gained is exactly the one nobody would think to add.
    let mut scanned = 0;
    for span in html.split('"') {
        if Path::new(span).extension().is_none_or(|e| e != "js") {
            continue;
        }
        let source = std::fs::read_to_string(panel_dir.join(span)).expect(span);
        for forbidden in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
            "new Function",
            "srcdoc",
        ] {
            assert!(!source.contains(forbidden), "{span} uses {forbidden}");
        }
        scanned += 1;
    }
    assert!(
        scanned >= 5,
        "the panel loads {scanned} scripts; the scan found too few to be reading the real page"
    );
    // The one `javascript:` in this surface is the comment naming what the
    // link policy blocks. A panel that built such a URL would not say so.
    let markdown = std::fs::read_to_string(panel_dir.join("markdown.js")).expect("markdown.js");
    assert!(
        markdown.contains("url.protocol !== \"https:\""),
        "the link policy no longer decides by protocol"
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

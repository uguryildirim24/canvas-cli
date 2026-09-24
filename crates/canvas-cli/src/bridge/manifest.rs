//! The Chrome native-messaging host manifest and where it is installed.
//!
//! `bridge install` writes one file into the browser's per-user
//! `NativeMessagingHosts` directory. It never opens a browser profile, a
//! cookie store, or a preferences file: the manifest is the whole of what the
//! CLI puts into the browser's world, and the extension is loaded by the
//! person, not by this command.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The native-messaging host name, which Chrome passes to the binary.
pub const HOST_NAME: &str = "com.canvas_cli.bridge";

/// The prefix Chrome uses for an extension's origin.
pub const EXTENSION_SCHEME: &str = "chrome-extension://";

pub use crate::cli::Browser;

/// The per-user `NativeMessagingHosts` directory of one browser.
///
/// macOS puts it under `Library/Application Support`; every other Unix under
/// the browser's own config directory. Windows keeps native host
/// registrations in the registry instead of a well-known directory, so there
/// is no path to return and `bridge install` says so.
#[must_use]
pub fn native_messaging_dir(browser: Browser, home: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let vendor = match browser {
            Browser::Chrome => "Google/Chrome",
            Browser::Chromium => "Chromium",
            Browser::Edge => "Microsoft Edge",
        };
        Some(
            home.join("Library")
                .join("Application Support")
                .join(vendor)
                .join("NativeMessagingHosts"),
        )
    } else if cfg!(windows) {
        None
    } else {
        let vendor = match browser {
            Browser::Chrome => "google-chrome",
            Browser::Chromium => "chromium",
            Browser::Edge => "microsoft-edge",
        };
        Some(
            home.join(".config")
                .join(vendor)
                .join("NativeMessagingHosts"),
        )
    }
}

/// The manifest file inside that directory.
#[must_use]
pub fn manifest_path(browser: Browser, home: &Path) -> Option<PathBuf> {
    native_messaging_dir(browser, home).map(|dir| dir.join(format!("{HOST_NAME}.json")))
}

/// The native-messaging host manifest Chrome reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostManifest {
    pub name: String,
    pub description: String,
    /// The absolute path of the `canvas` binary.
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// Exactly the extension ids that may connect.
    pub allowed_origins: Vec<String>,
}

impl HostManifest {
    /// Build the manifest for one extension id and one binary.
    pub fn new(extension_id: &str, binary: &Path) -> Result<Self, ManifestError> {
        if !canvas_core::bridge::state::is_extension_id(extension_id) {
            return Err(ManifestError::ExtensionId);
        }
        if !binary.is_absolute() {
            return Err(ManifestError::RelativePath);
        }
        Ok(Self {
            name: HOST_NAME.to_owned(),
            description: "canvas-cli companion broker".to_owned(),
            path: binary.display().to_string(),
            kind: "stdio".to_owned(),
            allowed_origins: vec![format!("{EXTENSION_SCHEME}{extension_id}/")],
        })
    }

    /// The one extension id this manifest admits.
    #[must_use]
    pub fn extension_id(&self) -> Option<&str> {
        self.allowed_origins
            .first()
            .and_then(|origin| origin.strip_prefix(EXTENSION_SCHEME))
            .map(|rest| rest.trim_end_matches('/'))
    }

    /// Serialize the file Chrome reads.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Why a manifest could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("an extension id is 32 characters from a to p")]
    ExtensionId,
    #[error("the manifest must name an absolute path to the canvas binary")]
    RelativePath,
}

/// The caller origin Chrome passes as the first argument.
///
/// Chrome runs the host as `<path> chrome-extension://<id>/ [--parent-window=N]`.
/// The origin is checked against the configured extension id before a single
/// message is read.
#[must_use]
pub fn origin_extension_id(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix(EXTENSION_SCHEME)?;
    let id = rest.trim_end_matches('/');
    canvas_core::bridge::state::is_extension_id(id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn the_manifest_is_the_document_chrome_reads() {
        let manifest = HostManifest::new(ID, Path::new("/usr/local/bin/canvas")).expect("build");
        let value: serde_json::Value =
            serde_json::from_str(&manifest.to_json()).expect("valid json");
        assert_eq!(value["name"], HOST_NAME);
        assert_eq!(value["type"], "stdio");
        assert_eq!(value["path"], "/usr/local/bin/canvas");
        assert_eq!(
            value["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{ID}/")])
        );
        assert_eq!(manifest.extension_id(), Some(ID));
    }

    /// M7-a acceptance: an exact extension id, and an absolute binary.
    #[test]
    fn a_manifest_refuses_a_wrong_id_or_a_relative_path() {
        for bad in [
            "",
            "short",
            "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert_eq!(
                HostManifest::new(bad, Path::new("/usr/local/bin/canvas")).unwrap_err(),
                ManifestError::ExtensionId,
                "{bad}"
            );
        }
        assert_eq!(
            HostManifest::new(ID, Path::new("canvas")).unwrap_err(),
            ManifestError::RelativePath
        );
    }

    #[test]
    fn the_caller_origin_yields_exactly_one_id() {
        assert_eq!(
            origin_extension_id(&format!("chrome-extension://{ID}/")),
            Some(ID)
        );
        assert_eq!(
            origin_extension_id(&format!("chrome-extension://{ID}")),
            Some(ID)
        );
        for bad in [
            "chrome-extension://short/",
            "https://evil.test/",
            "chrome-extension://ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP/",
            "",
        ] {
            assert_eq!(origin_extension_id(bad), None, "{bad}");
        }
        // The pre-parse shim in `main` recognizes the same prefix.
        assert!(crate::cli::is_native_messaging_caller(
            "chrome-extension://x/"
        ));
        assert!(!crate::cli::is_native_messaging_caller("bridge"));
    }

    #[test]
    fn every_browser_names_a_per_user_directory_on_this_platform() {
        let home = Path::new("/home/rolf");
        for browser in [Browser::Chrome, Browser::Chromium, Browser::Edge] {
            let path = manifest_path(browser, home);
            if cfg!(windows) {
                assert_eq!(path, None, "{}", browser.as_str());
            } else {
                let path = path.unwrap_or_else(|| panic!("{}", browser.as_str()));
                assert!(path.starts_with(home), "{}", path.display());
                assert!(path.ends_with(format!("{HOST_NAME}.json")));
                assert!(
                    path.to_string_lossy().contains("NativeMessagingHosts"),
                    "{}",
                    path.display()
                );
            }
        }
    }

    /// The three browsers must not share one directory: installing for Chrome
    /// must not write into Chromium's profile tree.
    #[test]
    fn the_browsers_do_not_share_a_directory() {
        let home = Path::new("/home/rolf");
        let paths: Vec<_> = [Browser::Chrome, Browser::Chromium, Browser::Edge]
            .into_iter()
            .filter_map(|b| manifest_path(b, home))
            .collect();
        if !paths.is_empty() {
            let mut unique = paths.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), paths.len());
        }
    }
}

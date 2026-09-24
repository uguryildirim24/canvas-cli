//! `canvas bridge install|host|status|detach` (`bridge@1`).
//!
//! `install`, `status`, and `detach` are ordinary class-B commands with
//! `--json` per §7. `host` is not: Chrome starts it and its stdout is the
//! native-messaging channel, so it has its own transport framing (the design note
//! §3.2) and lives in [`crate::bridge::host`].
//!
//! `install` writes one file: the native-messaging host manifest, into the
//! browser's per-user `NativeMessagingHosts` directory. It never opens a
//! browser profile, a cookie store, or a preferences file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use canvas_core::bridge::Endpoint;
use canvas_core::bridge::ipc::{Body, Op, Reason};
use serde_json::json;

use super::Globals;
use super::emit::{base_envelope, emit_error, session_error};
use super::handled::Handled;
use crate::bridge::client;
use crate::bridge::manifest::{self, HOST_NAME, HostManifest};
use crate::bridge::owner;
use crate::cli::Browser;
use crate::output::SCHEMA_BRIDGE;

/// The subcommands of `canvas bridge`.
#[derive(Debug, Clone)]
pub enum BridgeCmd {
    Install {
        extension_id: Option<String>,
        browser: Option<Browser>,
    },
    Status,
    Detach {
        attachment_id: Option<String>,
    },
}

/// Run one `canvas bridge` subcommand for the CLI.
pub async fn run(globals: &Globals, command: BridgeCmd) -> ExitCode {
    handle(globals, command).await.emit(globals.json)
}

/// Run one `canvas bridge` subcommand.
pub async fn handle(globals: &Globals, command: BridgeCmd) -> Handled {
    match command {
        BridgeCmd::Install {
            extension_id,
            browser,
        } => install(globals, extension_id, browser.unwrap_or(Browser::Chrome)),
        BridgeCmd::Status => status(globals),
        BridgeCmd::Detach { attachment_id } => detach(globals, attachment_id, None),
    }
}

// ------------------------------------------------------------------ install

fn install(globals: &Globals, extension_id: Option<String>, browser: Browser) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let paths = match crate::paths::CliPaths::resolve() {
        Ok(paths) => paths,
        Err(e) => {
            return emit_error(
                "local",
                &e.to_string(),
                13,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let configured = crate::config::Config::load_file(&paths)
        .ok()
        .and_then(|config| config.bridge.extension_id);
    let Some(id) = extension_id.clone().or(configured) else {
        return emit_error(
            "usage",
            "bridge install needs an extension id: load `extension/` unpacked in \
             chrome://extensions, then pass --extension-id <ID>",
            2,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };
    let Some(binary) = crate::bridge::host::binary_path() else {
        return emit_error(
            "local",
            "cannot resolve the absolute path of the canvas binary",
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };
    let manifest = match HostManifest::new(&id, &binary) {
        Ok(manifest) => manifest,
        Err(e) => {
            return emit_error(
                "usage",
                &e.to_string(),
                2,
                session.profile.clone(),
                Some(session.identity_ref()),
            );
        }
    };
    let Some(home) = home_dir() else {
        return emit_error(
            "local",
            "cannot locate the home directory",
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };
    let Some(path) = manifest::manifest_path(browser, &home) else {
        return emit_error(
            "refused",
            "this platform registers native messaging hosts in the registry, \
             not in a NativeMessagingHosts directory; see docs/companion.md",
            8,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    };
    if let Err(e) = write_manifest(&path, &manifest) {
        return emit_error(
            "local",
            &format!("cannot write {}: {e}", path.display()),
            13,
            session.profile.clone(),
            Some(session.identity_ref()),
        );
    }
    // Remember the id, so `bridge host` can refuse every other extension.
    if extension_id.is_some()
        && let Ok(mut config) = crate::config::Config::load_file(&paths)
    {
        config.bridge.extension_id = Some(id.clone());
        let _ = config.save(&paths);
    }

    let result = json!({
        "browser": browser.as_str(),
        "host_name": HOST_NAME,
        "extension_id": id,
        "binary": binary.display().to_string(),
        "manifest_path": path.display().to_string(),
        "written": true,
        "steps": load_steps(),
    });
    let envelope = base_envelope(SCHEMA_BRIDGE, &session, result);
    Handled::new(envelope, |envelope| {
        let mut out = io::stdout();
        writeln!(
            out,
            "wrote {}",
            envelope.result["manifest_path"].as_str().unwrap_or("")
        )?;
        for step in envelope.result["steps"].as_array().into_iter().flatten() {
            writeln!(out, "  - {}", step.as_str().unwrap_or(""))?;
        }
        Ok(())
    })
}

/// The unpacked-extension steps a person follows once, in Chrome.
fn load_steps() -> Vec<String> {
    vec![
        "Open chrome://extensions and turn on Developer mode.".to_owned(),
        "Choose \"Load unpacked\" and select the `extension/` directory that ships \
         with canvas-cli."
            .to_owned(),
        "Copy the extension id Chrome shows. If it differs from the one in the \
         manifest, run `canvas bridge install --extension-id <ID>` again."
            .to_owned(),
        "Open your Canvas tab and click the canvas-cli toolbar button, or press \
         its keyboard shortcut, to attach it."
            .to_owned(),
        "Check it with `canvas bridge status`, then read `canvas here --json`.".to_owned(),
    ]
}

/// Write the manifest atomically, at mode `0600`.
fn write_manifest(path: &Path, manifest: &HostManifest) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let _ = fs::remove_file(&temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(manifest.to_json().as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&temp, path)
}

// ------------------------------------------------------------------- status

fn status(globals: &Globals) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);
    let manifest = installed_manifest();
    let live = owner::live_owner(&endpoint);
    let attachments = if live.is_some() {
        client::call(&endpoint, Op::AttachmentsList)
            .ok()
            .and_then(|body| match body {
                Body::Attachments { attachments } => serde_json::to_value(attachments).ok(),
                _ => None,
            })
            .unwrap_or_else(|| json!([]))
    } else {
        json!([])
    };
    let result = json!({
        "endpoint": endpoint.address(),
        "manifest": manifest,
        "owner": {
            "live": live.is_some(),
            "pid": live.as_ref().map(|owner| owner.pid),
            "started_at": live.as_ref().map(|owner| owner.started_at.clone()),
        },
        "attachments": attachments,
    });
    let envelope = base_envelope(SCHEMA_BRIDGE, &session, result);
    Handled::new(envelope, |envelope| {
        writeln!(io::stdout(), "{}", human_status(&envelope.result))
    })
}

/// The manifest this machine has installed, for whichever browser has one.
fn installed_manifest() -> serde_json::Value {
    let home = home_dir();
    let mut fallback = json!({
        "browser": Browser::Chrome.as_str(),
        "path": null,
        "present": false,
        "host_name": HOST_NAME,
        "extension_id": null,
    });
    let Some(home) = home else {
        return fallback;
    };
    for browser in [Browser::Chrome, Browser::Chromium, Browser::Edge] {
        let Some(path) = manifest::manifest_path(browser, &home) else {
            continue;
        };
        let entry = |present: bool, id: Option<String>| {
            json!({
                "browser": browser.as_str(),
                "path": path.display().to_string(),
                "present": present,
                "host_name": HOST_NAME,
                "extension_id": id,
            })
        };
        if browser == Browser::Chrome {
            fallback = entry(false, None);
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let id = serde_json::from_str::<HostManifest>(&raw)
            .ok()
            .and_then(|manifest| manifest.extension_id().map(str::to_owned));
        return entry(true, id);
    }
    fallback
}

fn human_status(result: &serde_json::Value) -> String {
    let mut lines = vec![format!(
        "endpoint {}",
        result["endpoint"].as_str().unwrap_or("")
    )];
    let manifest = &result["manifest"];
    lines.push(format!(
        "manifest {} for {} ({})",
        if manifest["present"] == json!(true) {
            "installed"
        } else {
            "absent"
        },
        manifest["browser"].as_str().unwrap_or(""),
        manifest["extension_id"]
            .as_str()
            .unwrap_or("no extension id")
    ));
    lines.push(if result["owner"]["live"] == json!(true) {
        format!(
            "host live, pid {}",
            result["owner"]["pid"].as_u64().unwrap_or(0)
        )
    } else {
        "host absent".to_owned()
    });
    let attachments = result["attachments"].as_array().map_or(0, Vec::len);
    lines.push(match attachments {
        0 => "no attachment".to_owned(),
        _ => format!(
            "attachment {} on {}",
            result["attachments"][0]["state"].as_str().unwrap_or(""),
            result["attachments"][0]["origin"].as_str().unwrap_or("")
        ),
    });
    lines.join("\n")
}

// ------------------------------------------------------------------- detach

fn detach(globals: &Globals, attachment_id: Option<String>, consumer: Option<String>) -> Handled {
    let session = match globals.open_local_session() {
        Ok(session) => session,
        Err(e) => return session_error(e, globals.profile.clone()),
    };
    let endpoint = Endpoint::for_identity(&session.paths.data_root, &session.identity.key);
    match client::call(
        &endpoint,
        Op::Detach {
            attachment_id: attachment_id.clone(),
            // `None` is the person: the CLI ends the attachment itself.
            consumer: consumer.clone(),
        },
    ) {
        Ok(Body::Detached { detached }) => {
            let result = json!({
                "detached": detached,
                "attachment_id": attachment_id,
                "reason": null,
            });
            let envelope = base_envelope(SCHEMA_BRIDGE, &session, result);
            Handled::new(envelope, |_| writeln!(io::stdout(), "detached"))
        }
        Ok(_) => refused(&session, attachment_id, Reason::Protocol),
        Err(reason) => refused(&session, attachment_id, reason),
    }
}

fn refused(
    session: &crate::session::Session,
    attachment_id: Option<String>,
    reason: Reason,
) -> Handled {
    let result = json!({
        "detached": false,
        "attachment_id": attachment_id,
        "reason": reason.as_str(),
    });
    let mut envelope = base_envelope(SCHEMA_BRIDGE, session, result);
    envelope.outcome = crate::output::Outcome::Refused;
    envelope.exit = 8;
    let message = match reason {
        Reason::BridgeUnavailable => "no canvas bridge host is running".to_owned(),
        Reason::NotAttached => "nothing is attached".to_owned(),
        other => format!("refused: {other}"),
    };
    Handled::new(envelope, move |_| writeln!(io::stderr(), "{message}"))
}

/// The user's home directory, honouring the test override the CLI already
/// uses for its own paths.
fn home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CANVAS_BRIDGE_HOME") {
        return Some(PathBuf::from(home));
    }
    etcetera::home_dir().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn the_manifest_is_written_atomically_and_privately() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("hosts").join("com.canvas_cli.bridge.json");
        let manifest = HostManifest::new(ID, Path::new("/usr/local/bin/canvas")).expect("build");
        write_manifest(&path, &manifest).expect("write");
        let raw = fs::read_to_string(&path).expect("read");
        let parsed: HostManifest = serde_json::from_str(&raw).expect("parse");
        assert_eq!(parsed.extension_id(), Some(ID));
        assert_eq!(parsed.path, "/usr/local/bin/canvas");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A second install replaces it without leaving a temp file behind.
        write_manifest(&path, &manifest).expect("rewrite");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "a temp file survived the install");
    }

    #[test]
    fn the_install_steps_name_the_unpacked_extension_flow() {
        let steps = load_steps().join("\n");
        assert!(steps.contains("chrome://extensions"));
        assert!(steps.contains("Load unpacked"));
        assert!(steps.contains("extension/"));
        assert!(steps.contains("bridge status"));
    }

    #[test]
    fn the_human_status_says_what_is_installed_and_what_is_live() {
        let quiet = json!({
            "endpoint": "/data/bridge/key.sock",
            "manifest": { "browser": "chrome", "present": false, "extension_id": null },
            "owner": { "live": false, "pid": null, "started_at": null },
            "attachments": [],
        });
        let text = human_status(&quiet);
        assert!(text.contains("manifest absent"), "{text}");
        assert!(text.contains("host absent"), "{text}");
        assert!(text.contains("no attachment"), "{text}");

        let busy = json!({
            "endpoint": "/data/bridge/key.sock",
            "manifest": { "browser": "chrome", "present": true, "extension_id": ID },
            "owner": { "live": true, "pid": 42, "started_at": "2026-09-10T10:00:00Z" },
            "attachments": [ { "state": "attached", "origin": "https://school.test" } ],
        });
        let text = human_status(&busy);
        assert!(text.contains("manifest installed"), "{text}");
        assert!(text.contains("host live, pid 42"), "{text}");
        assert!(text.contains("attachment attached"), "{text}");
    }
}

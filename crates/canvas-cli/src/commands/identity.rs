//! `identity list|remove` commands.

use serde_json::json;

use canvas_core::identity::{
    remove_identity, IdentityKey, IdentityLock, RemovalCallbacks,
};

use crate::cli::{Globals, IdentityCommand};
use crate::config::Config;
use crate::credentials;
use crate::exit::CliError;
use crate::output::{Envelope, IdentityRef, print_human};
use crate::paths::CliPaths;
use crate::selection::{self, dir_size};
use crate::token;

/// Dispatch identity subcommands.
pub async fn run(globals: &Globals, command: IdentityCommand) -> Result<(), CliError> {
    let paths = CliPaths::resolve()?;
    match command {
        IdentityCommand::List => list(globals, &paths),
        IdentityCommand::Remove { identity_key, yes } => {
            remove(globals, &paths, &identity_key, yes)
        }
    }
}

fn list(globals: &Globals, paths: &CliPaths) -> Result<(), CliError> {
    let config = Config::load(paths)?;
    let identities = selection::list_identities(paths)?;
    let mut items = Vec::new();
    for doc in &identities {
        let profiles: Vec<String> = config
            .profiles
            .iter()
            .filter(|(_, p)| p.key == doc.key.as_str())
            .map(|(n, _)| n.clone())
            .collect();
        let core = selection::paths_for_key(paths, &doc.key);
        let size_bytes = dir_size(&core.identity_dir);
        let name = config
            .profiles
            .values()
            .find(|p| p.key == doc.key.as_str())
            .and_then(|p| p.name.clone());
        let journals_pending = count_pending_journals(paths, doc).unwrap_or(0);
        items.push(json!({
            "key": doc.key.as_str(),
            "origin": doc.origin,
            "user_id": doc.user_id.to_string(),
            "name": name,
            "profiles": profiles,
            "size_bytes": size_bytes,
            "journals_pending": journals_pending,
        }));
    }
    let result = json!({ "identities": items });
    if globals.json {
        Envelope::new("canvas-cli/identity@1", None, None)
            .with_result(result)
            .print_json();
    } else if identities.is_empty() {
        print_human(["no identities"]);
    } else {
        for doc in &identities {
            let profiles: Vec<_> = config
                .profiles
                .iter()
                .filter(|(_, p)| p.key == doc.key.as_str())
                .map(|(n, _)| n.as_str())
                .collect();
            print_human([format!(
                "{}  {} / {}  profiles=[{}]",
                doc.key.as_str(),
                doc.origin,
                doc.user_id,
                profiles.join(", ")
            )]);
        }
    }
    Ok(())
}

fn count_pending_journals(
    paths: &CliPaths,
    doc: &canvas_core::identity::IdentityDocument,
) -> Result<i64, CliError> {
    let core = selection::paths_for_key(paths, &doc.key);
    if !core.state_db.exists() {
        return Ok(0);
    }
    let open = canvas_core::store::OpenIdentity::open(&core, doc)?;
    open.store
        .call_blocking(|conns| {
            let n: i64 = conns.state.query_row(
                "SELECT COUNT(*) FROM submission_journal
                 WHERE state IN ('planned','uploading','uploaded','posting')
                    OR (state = 'outcome_unknown' AND acknowledged_at IS NULL)",
                [],
                |r| r.get(0),
            )?;
            Ok(n)
        })
        .map_err(CliError::from)
}

fn remove(
    globals: &Globals,
    paths: &CliPaths,
    identity_key: &str,
    yes: bool,
) -> Result<(), CliError> {
    let key = IdentityKey::parse(identity_key)?;
    let core = selection::paths_for_key(paths, &key);
    if !core.identity_json().exists() {
        return Err(CliError::auth(format!(
            "identity `{identity_key}` not found"
        )));
    }
    if !yes {
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            return Err(CliError::usage(
                "identity remove requires confirmation; pass --yes",
            ));
        }
        eprint!("Remove identity {identity_key} and all local data? [y/N] ");
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| CliError::local(e.to_string()))?;
        if !matches!(line.trim(), "y" | "Y" | "yes" | "YES") {
            return Err(CliError::new(
                crate::exit::ExitKind::Cancelled,
                "cancelled",
            ));
        }
    }

    let lock = IdentityLock::acquire_exclusive(&core)?;
    let mut profiles_removed = Vec::new();
    let mut default_cleared = false;
    let paths_clone = paths.clone();
    let key_clone = key.clone();
    let mut delete_credentials = || {
        credentials::delete_all_for_identity(&paths_clone, &key_clone)
            .map_err(|e| canvas_core::identity::IdentityError::Mismatch {
                reason: e.to_string(),
            })
    };
    let mut remove_profiles = || {
        let mut config = Config::load(&paths_clone).map_err(|e| {
            canvas_core::identity::IdentityError::Mismatch {
                reason: e.to_string(),
            }
        })?;
        let before_default = config.default_profile.clone();
        config.profiles.retain(|name, profile| {
            if profile.key == key_clone.as_str() {
                profiles_removed.push(name.clone());
                false
            } else {
                true
            }
        });
        if before_default
            .as_ref()
            .is_some_and(|d| profiles_removed.iter().any(|p| p == d))
        {
            config.default_profile = None;
            default_cleared = true;
        }
        config.save(&paths_clone).map_err(|e| {
            canvas_core::identity::IdentityError::Mismatch {
                reason: e.to_string(),
            }
        })?;
        Ok(())
    };
    let mut callbacks = RemovalCallbacks {
        delete_credentials: &mut delete_credentials,
        remove_profiles: &mut remove_profiles,
    };
    remove_identity(&core, lock, &mut callbacks)?;

    let result = json!({
        "removed": true,
        "key": key.as_str(),
        "profiles_removed": profiles_removed,
        "default_profile_cleared": default_cleared,
    });
    if globals.json {
        Envelope::new("canvas-cli/identity@1", None, None)
            .with_result(result)
            .print_json();
    } else {
        print_human([format!("removed identity {}", key.as_str())]);
    }
    let _ = token::user_agent();
    let _ = IdentityRef::new("", 0, "");
    Ok(())
}

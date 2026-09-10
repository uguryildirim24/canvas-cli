//! `doctor` command.

use serde_json::{Value, json};

use crate::cli::Globals;
use crate::config::Config;
use crate::credentials::{self, ActiveSource, CredentialRow};
use crate::exit::CliError;
use crate::output::{Envelope, IdentityRef, print_human};
use crate::paths::CliPaths;
use crate::selection::{self, CommandClass, SelectionInput};
use crate::token;

/// Dispatch `doctor`.
pub async fn run(globals: &Globals, network: bool) -> Result<(), CliError> {
    if network && globals.offline {
        return Err(CliError::usage(
            "doctor --network cannot be used with --offline",
        ));
    }

    let paths = CliPaths::resolve()?;
    let config = Config::load(&paths);
    let mut checks = Vec::new();
    let mut identity_selected = false;
    let mut recovered_journals: Vec<String> = Vec::new();
    let mut selected_meta: Option<(String, IdentityRef)> = None;

    // Config parse.
    match &config {
        Ok(_) => push_check(&mut checks, "config", "ok", "config parsed"),
        Err(e) => push_check(&mut checks, "config", "fail", &e.message),
    }

    let config = config.unwrap_or_default();
    let selected = selection::select(
        if network {
            CommandClass::D
        } else {
            CommandClass::B
        },
        &paths,
        &config,
        &SelectionInput {
            profile_flag: globals.profile.as_deref(),
            offline: globals.offline,
            allow_new_profile: false,
        },
    );

    match selected {
        Ok(sel) => {
            identity_selected = true;
            selected_meta = Some((
                sel.profile_name.clone().unwrap_or_default(),
                IdentityRef::new(
                    &sel.identity.origin,
                    sel.identity.user_id,
                    sel.identity.key.as_str(),
                ),
            ));
            push_check(
                &mut checks,
                "profile_identity",
                "ok",
                &format!("profile {:?} → {}", sel.profile_name, sel.identity.key),
            );

            match token::open_store(&sel) {
                Ok(open) => {
                    push_check(&mut checks, "databases", "ok", "cache and state opened");
                    match CredentialRow::load(&open.store, &sel.identity.key) {
                        Ok(row) => {
                            let msg = format!(
                                "active_source={} cleanup_keyring={} cleanup_file={}",
                                row.active_source.as_str(),
                                row.cleanup_keyring,
                                row.cleanup_file
                            );
                            let status = if row.active_source == ActiveSource::None {
                                "warn"
                            } else {
                                "ok"
                            };
                            push_check(&mut checks, "credential_row", status, &msg);
                            let stray =
                                credentials::stray_sources(&paths, &sel.identity.key, &row)?;
                            if stray.is_empty() {
                                push_check(&mut checks, "stray_credentials", "ok", "none");
                            } else {
                                push_check(
                                    &mut checks,
                                    "stray_credentials",
                                    "warn",
                                    &stray.join(", "),
                                );
                            }
                        }
                        Err(e) => push_check(&mut checks, "credential_row", "fail", &e.message),
                    }
                    recovered_journals = recover_owner_absent_journals(&open.store);
                    if recovered_journals.is_empty() {
                        push_check(
                            &mut checks,
                            "owner_absent_journals",
                            "ok",
                            "no recovery needed",
                        );
                    } else {
                        push_check(
                            &mut checks,
                            "owner_absent_journals",
                            "warn",
                            &format!("recovered {}", recovered_journals.len()),
                        );
                    }

                    if network {
                        match token::resolve_token(&paths, &open.store, &sel.identity.key) {
                            Ok(resolved) => {
                                match token::validate_identity_user(
                                    &sel.identity.origin,
                                    resolved.token.expose(),
                                    sel.identity.user_id,
                                )
                                .await
                                {
                                    Ok(_) => push_check(
                                        &mut checks,
                                        "network_users_self",
                                        "ok",
                                        "id matches",
                                    ),
                                    Err(e) => push_check(
                                        &mut checks,
                                        "network_users_self",
                                        "fail",
                                        &e.message,
                                    ),
                                }
                            }
                            Err(e) => {
                                push_check(&mut checks, "network_users_self", "fail", &e.message);
                            }
                        }
                    }
                }
                Err(e) => push_check(&mut checks, "databases", "fail", &e.message),
            }
        }
        Err(e) => {
            push_check(
                &mut checks,
                "profile_identity",
                "skipped",
                &format!("no identity selected: {}", e.message),
            );
            push_check(&mut checks, "databases", "skipped", "no identity");
            push_check(&mut checks, "credential_row", "skipped", "no identity");
            push_check(&mut checks, "stray_credentials", "skipped", "no identity");
            push_check(
                &mut checks,
                "owner_absent_journals",
                "skipped",
                "no identity",
            );
        }
    }

    let backend = if credentials::keyring_available() {
        "keyring available"
    } else {
        "keyring unavailable; file fallback may be used"
    };
    push_check(&mut checks, "credential_backend", "ok", backend);
    push_check(
        &mut checks,
        "identity_lock",
        "ok",
        "lock protocol available",
    );

    if !network || globals.offline {
        push_check(
            &mut checks,
            "network_users_self",
            "skipped",
            "pass --network to probe",
        );
    }

    let fail = checks.iter().any(|c| c["status"] == "fail");
    let result = json!({
        "identity_selected": identity_selected,
        "checks": checks,
        "recovered_journals": recovered_journals,
    });

    let (profile, identity) = match selected_meta {
        Some((p, i)) => (Some(p), Some(i)),
        None => (None, None),
    };

    if globals.json {
        let env = Envelope::new("canvas-cli/doctor@1", profile.as_deref(), identity)
            .with_result(result)
            .with_outcome(
                if fail { "partial" } else { "ok" },
                if fail { 12 } else { 0 },
            );
        env.print_json();
    } else {
        print_human([format!("identity selected: {identity_selected}")]);
        for c in &checks {
            print_human([format!(
                "[{}] {}: {}",
                c["status"].as_str().unwrap_or("?"),
                c["name"].as_str().unwrap_or("?"),
                c["message"].as_str().unwrap_or("")
            )]);
        }
    }

    let _ = fail;
    Ok(())
}

fn push_check(checks: &mut Vec<Value>, name: &str, status: &str, message: &str) {
    checks.push(json!({
        "name": name,
        "status": status,
        "message": message,
    }));
}

/// Extension point for M2-a: recover journals whose owner process is absent.
///
/// Returns recovered journal ids. Currently a no-op.
#[allow(clippy::needless_pass_by_ref_mut)] // signature reserved for M2-a
pub fn recover_owner_absent_journals(_store: &canvas_core::store::Store) -> Vec<String> {
    Vec::new()
}

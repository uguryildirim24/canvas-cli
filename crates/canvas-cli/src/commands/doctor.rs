//! Local diagnostics and explicit online identity validation.
use crate::cli::Globals;
use crate::config::Config;
use crate::credentials::{self, ActiveSource, CredentialRow};
use crate::exit::{CliError, ExitKind};
use crate::output::{Envelope, IdentityRef, print_human};
use crate::paths::CliPaths;
use crate::selection::{self, CommandClass, SelectionInput};
use crate::token;
use serde_json::{Value, json};

pub async fn run(globals: &Globals, network: bool) -> Result<(), CliError> {
    // §8's class-D offline rule is checked before any I/O.
    if network && globals.offline {
        return Err(CliError::usage(
            "doctor --network cannot be used with --offline",
        ));
    }
    let paths = CliPaths::resolve()?;
    let mut checks = Vec::new();
    let parsed = Config::load_with_flags(&paths, globals);
    push_check(
        &mut checks,
        "config",
        if parsed.is_ok() { "ok" } else { "fail" },
        if parsed.is_ok() {
            "config parsed"
        } else {
            "config parse failed"
        },
    );
    let config = parsed.unwrap_or_default();
    let selected = selection::select_online(
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
    )
    .await;
    let mut envelope = Envelope::new("canvas-cli/doctor@1", None, None);
    let mut recovered_journals = Vec::new();
    let mut identity_selected = false;
    let mut network_headers = None;
    match selected {
        Ok(mut sel) => {
            identity_selected = true;
            envelope.profile = sel.profile_name.clone();
            envelope.identity = Some(IdentityRef::new(
                &sel.identity.origin,
                sel.identity.user_id,
                sel.identity.key.as_str(),
            ));
            push_check(
                &mut checks,
                "profile_identity",
                "ok",
                &format!("{} / {}", sel.identity.origin, sel.identity.user_id),
            );
            match token::open_store(&sel) {
                Ok(open) => {
                    push_check(
                        &mut checks,
                        "identity_lock",
                        "ok",
                        "shared identity lock held; generation verified",
                    );
                    let integrity = open.store.call_blocking(|conns| {
                        let mut reports = Vec::new();
                        for (name, conn) in [("cache", &conns.cache), ("state", &conns.state)] {
                            let mut stmt = conn.prepare("PRAGMA integrity_check")?;
                            let messages = stmt
                                .query_map([], |r| r.get::<_, String>(0))?
                                .collect::<Result<Vec<_>, _>>()?;
                            let version: i64 =
                                conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                            reports.push((name, messages == ["ok"], version));
                        }
                        Ok(reports)
                    });
                    match integrity {
                        Ok(reports) => {
                            let ok = reports.iter().all(|(_, ok, _)| *ok);
                            let message = reports
                                .iter()
                                .map(|(name, _, version)| format!("{name} schema={version}"))
                                .collect::<Vec<_>>()
                                .join(", ");
                            push_check(
                                &mut checks,
                                "databases",
                                if ok { "ok" } else { "fail" },
                                &message,
                            );
                        }
                        Err(_) => push_check(
                            &mut checks,
                            "databases",
                            "fail",
                            "database integrity check failed",
                        ),
                    }
                    let row = CredentialRow::load(&open.store, &sel.identity.key)
                        .map_err(|e| e.for_selected(&sel))?;
                    push_check(
                        &mut checks,
                        "credential_row",
                        if row.active_source == ActiveSource::None
                            || row.cleanup_keyring
                            || row.cleanup_file
                        {
                            "warn"
                        } else {
                            "ok"
                        },
                        &format!(
                            "active_source={} cleanup_keyring={} cleanup_file={}",
                            row.active_source.as_str(),
                            row.cleanup_keyring,
                            row.cleanup_file
                        ),
                    );
                    match credentials::stray_sources(&paths, &sel.identity.key, &row) {
                        Ok(stray) => push_check(
                            &mut checks,
                            "stray_credentials",
                            if stray.is_empty() { "ok" } else { "warn" },
                            &if stray.is_empty() {
                                "none".to_owned()
                            } else {
                                stray.join(", ")
                            },
                        ),
                        Err(e) => return Err(e.for_selected(&sel)),
                    }
                    recovered_journals = recover_owner_absent_journals(&open.store);
                    push_check(
                        &mut checks,
                        "owner_absent_journals",
                        "skipped",
                        "recovery hook reserved for M2-a",
                    );
                    if network {
                        let (user, headers, telemetry) =
                            if let Some(validation) = sel.validation.take() {
                                validation
                            } else {
                                let resolved =
                                    token::resolve_token(&paths, &open.store, &sel.identity.key)
                                        .map_err(|e| e.for_selected(&sel))?;
                                eprintln!("using token from {}", resolved.source.as_str());
                                token::validate_users_self_details(
                                    &sel.identity.origin,
                                    resolved.token.expose(),
                                    &config.network,
                                )
                                .await
                                .map_err(|e| e.for_selected(&sel))?
                            };
                        if user.id != sel.identity.user_id {
                            return Err(CliError::auth("identity mismatch")
                                .for_selected(&sel)
                                .with_requests(telemetry));
                        }
                        envelope.requests.api = telemetry.api;
                        envelope.requests.storage = telemetry.storage;
                        envelope.requests.cost = telemetry.cost;
                        network_headers = Some(headers);
                        push_check(&mut checks, "network_users_self", "ok", "id matches");
                    }
                }
                Err(e) => {
                    push_check(&mut checks, "databases", "fail", &e.message);
                    push_check(
                        &mut checks,
                        "identity_lock",
                        "fail",
                        "identity/store open failed",
                    );
                }
            }
        }
        Err(e) => {
            // Fallback only for absence of a selectable identity, not malformed inputs,
            // rejected online tokens, or corrupt identity documents.
            if e.kind != ExitKind::Auth || (network && !e.message.contains("no profile selected")) {
                return Err(e);
            }
            push_check(&mut checks, "profile_identity", "skipped", &e.message);
        }
    }
    let available = credentials::keyring_available();
    push_check(
        &mut checks,
        "credential_backend",
        if available { "ok" } else { "warn" },
        if available {
            "keyring available"
        } else {
            "keyring unavailable"
        },
    );
    for name in [
        "identity_lock",
        "databases",
        "credential_row",
        "stray_credentials",
        "owner_absent_journals",
    ] {
        if !checks.iter().any(|c| c["name"] == name) {
            push_check(&mut checks, name, "skipped", "no open identity");
        }
    }
    if let Some(headers) = network_headers {
        let remaining = headers
            .get("x-rate-limit-remaining")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);
        push_check(
            &mut checks,
            "network_rate_limit",
            if remaining.is_some() { "ok" } else { "warn" },
            &remaining.map_or_else(
                || "header absent or invalid".into(),
                |n| format!("remaining={n}"),
            ),
        );
        let date = headers
            .get("date")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| jiff::fmt::rfc2822::parse(h).ok());
        let skew = date.map(|d| jiff::Timestamp::now().as_second() - d.timestamp().as_second());
        push_check(
            &mut checks,
            "network_clock_skew",
            if skew.is_some() { "ok" } else { "warn" },
            &skew.map_or_else(
                || "Date header absent or invalid".into(),
                |n| format!("clock skew seconds={n}"),
            ),
        );
    } else {
        for name in [
            "network_users_self",
            "network_rate_limit",
            "network_clock_skew",
        ] {
            push_check(&mut checks, name, "skipped", "network checks not run");
        }
    }
    // Appendix D requires fixed check order regardless of availability.
    let order = [
        "config",
        "profile_identity",
        "credential_row",
        "stray_credentials",
        "databases",
        "credential_backend",
        "identity_lock",
        "owner_absent_journals",
        "network_users_self",
        "network_rate_limit",
        "network_clock_skew",
    ];
    checks.sort_by_key(|c| {
        order
            .iter()
            .position(|name| c["name"] == *name)
            .unwrap_or(order.len())
    });
    let failed = checks.iter().any(|c| c["status"] == "fail");
    if globals.json {
        envelope
            .with_result(
                json!({"identity_selected": identity_selected, "checks": checks,
            "recovered_journals": recovered_journals}),
            )
            .with_outcome(
                if failed { "partial" } else { "ok" },
                if failed { 12 } else { 0 },
            )
            .print_json();
    } else {
        print_human([format!("identity selected: {identity_selected}")]);
        for check in &checks {
            print_human([format!(
                "[{}] {}: {}",
                check["status"].as_str().unwrap_or("?"),
                check["name"].as_str().unwrap_or("?"),
                check["message"].as_str().unwrap_or("")
            )]);
        }
    }
    if failed {
        Err(CliError::new(ExitKind::Partial, ""))
    } else {
        Ok(())
    }
}

fn push_check(checks: &mut Vec<Value>, name: &str, status: &str, message: &str) {
    checks.push(json!({"name": name, "status": status, "message": message}));
}

/// Extension point for M2-a; the core journal module has no recovery API yet.
pub fn recover_owner_absent_journals(store: &canvas_core::store::Store) -> Vec<String> {
    canvas_core::journal::recover_owner_absent(store)
}

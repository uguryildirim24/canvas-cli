//! `auth login|status|logout|token` commands.

use std::io::{self, IsTerminal, Read, Write};

use serde_json::json;

use canvas_core::identity::{IdentityKey, Paths as CorePaths};
use canvas_core::store::OpenIdentity;

use crate::cli::{AuthCommand, Globals};
use crate::config::{Config, Profile};
use crate::credentials::{self, ActiveSource, CredentialRow};
use crate::exit::{CliError, ExitKind};
use crate::origin::canonicalize_origin;
use crate::output::{Envelope, IdentityRef, print_human};
use crate::paths::CliPaths;
use crate::selection::{self, CommandClass, SelectionInput};
use crate::token::{self, TokenSource};

/// Dispatch auth subcommands.
pub async fn run(globals: &Globals, command: AuthCommand) -> Result<(), CliError> {
    let paths = CliPaths::resolve()?;
    match command {
        AuthCommand::Login {
            host,
            token_stdin,
            replace,
        } => login(globals, &paths, host, token_stdin, replace).await,
        AuthCommand::Status => status(globals, &paths).await,
        AuthCommand::Logout => logout(globals, &paths).await,
        AuthCommand::Token { reveal } => token_cmd(globals, &paths, reveal).await,
    }
}

async fn login(
    globals: &Globals,
    paths: &CliPaths,
    host: Option<String>,
    token_stdin: bool,
    replace: bool,
) -> Result<(), CliError> {
    if globals.offline {
        return Err(CliError::usage(
            "auth login requires the network; refuse --offline",
        ));
    }

    let config = Config::load(paths)?;
    let profile_input = globals
        .profile
        .clone()
        .or_else(|| std::env::var("CANVAS_PROFILE").ok());
    let profile_name = profile_input.clone().unwrap_or_else(|| "default".into());
    let env_host = std::env::var("CANVAS_HOST").ok().filter(|v| !v.is_empty());
    if profile_input.is_some() && env_host.is_some() {
        eprintln!("warning: CANVAS_HOST is ignored when a profile is selected");
    }
    let host_raw = match host
        .or_else(|| {
            if profile_input.is_some() {
                config.profiles.get(&profile_name).map(|p| p.origin.clone())
            } else {
                env_host
            }
        })
        .or_else(|| config.profiles.get(&profile_name).map(|p| p.origin.clone()))
    {
        Some(h) => h,
        None => prompt_line("Canvas host: ")?,
    };
    let origin = canonicalize_origin(&host_raw)?;

    if !token_stdin
        && std::env::var("CANVAS_TOKEN")
            .ok()
            .is_none_or(|t| t.is_empty())
        && io::stdin().is_terminal()
    {
        let settings = format!("{origin}/profile/settings");
        eprintln!("Create a personal access token at {settings}");
        if matches!(
            prompt_line("Open token settings in your browser? [y/N] ")?.as_str(),
            "y" | "Y" | "yes"
        ) {
            open::that(&settings).map_err(|_| CliError::local("could not open token settings"))?;
        }
    }
    let (token, token_source_label) = if token_stdin {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::usage(format!("failed to read token from stdin: {e}")))?;
        (buf.trim().to_owned(), "stdin")
    } else if let Ok(t) = std::env::var("CANVAS_TOKEN") {
        if t.is_empty() {
            (prompt_secret("Canvas token: ")?, "prompt")
        } else {
            (t, "CANVAS_TOKEN")
        }
    } else {
        (prompt_secret("Canvas token: ")?, "prompt")
    };
    if token.is_empty() {
        return Err(CliError::usage("token must not be empty"));
    }
    eprintln!("using token from {token_source_label}");

    let user = token::validate_users_self(&origin, &token).await?;
    let key = IdentityKey::compute(&origin, user.id);

    if let Some(existing) = config.profiles.get(&profile_name) {
        if existing.key != key.as_str() && !replace {
            return Err(CliError::auth(format!(
                "profile `{profile_name}` points at a different identity; pass --replace to rebind"
            )));
        }
        // Token for a different user than an existing identity under this profile path:
        // still refuse identity mismatch when the profile's recorded user differs and the
        // operator did not intend a replace of the label alone.
        if existing.user_id != user.id && existing.origin == origin && !replace {
            return Err(CliError::auth("identity mismatch"));
        }
    }

    let (identity, _initialization_lock) = selection::initialize_identity(paths, &origin, user.id)?;
    let core_paths = CorePaths::for_identity(&paths.data_dir, &key);
    let open = OpenIdentity::open(&core_paths, &identity)?;
    let _config_lock = crate::config::ConfigLock::acquire(paths)?;
    let mut config = Config::load_file(paths)?;
    if config
        .profiles
        .get(&profile_name)
        .is_some_and(|p| p.key != key.as_str())
        && !replace
    {
        return Err(CliError::auth(
            "profile points at a different identity; pass --replace to rebind",
        ));
    }

    let validated_at = jiff::Timestamp::now().to_string();
    let source = credentials::activate(paths, &open.store, &key, &token, &validated_at)?;

    config.profiles.insert(
        profile_name.clone(),
        Profile {
            origin: origin.clone(),
            user_id: user.id,
            key: key.as_str().to_owned(),
            name: user.name.clone(),
            time_zone: user.time_zone.clone(),
        },
    );
    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.clone());
    }
    config.save(paths)?;

    // Bind env pair when both env vars were the inputs.
    if std::env::var_os("CANVAS_HOST").is_some() && std::env::var_os("CANVAS_TOKEN").is_some() {
        if profile_input.is_none() {
            let env_origin =
                canonicalize_origin(&std::env::var("CANVAS_HOST").unwrap_or_default())?;
            let env_token = std::env::var("CANVAS_TOKEN").unwrap_or_default();
            if env_origin == origin && env_token == token {
                selection::write_env_binding(paths, &origin, &token, &key)?;
            }
        }
    }

    let backend = credentials::backend_label(source).unwrap_or("unknown");
    let identity_ref = IdentityRef::new(&origin, user.id, key.as_str());
    let result = json!({
        "profile": profile_name,
        "identity": {
            "origin": origin,
            "user_id": user.id.to_string(),
            "key": key.as_str(),
        },
        "backend": backend,
        "removed": [],
    });

    if globals.json {
        Envelope::new(
            "canvas-cli/auth_login@1",
            Some(&profile_name),
            Some(identity_ref),
        )
        .with_result(result)
        .with_api_requests(1)
        .print_json();
    } else {
        let name = user.name.as_deref().unwrap_or("(unnamed)");
        print_human([
            format!("logged in as {name} ({})", user.id),
            format!("profile: {profile_name}"),
            format!("identity: {origin} / {}", user.id),
            format!("credential backend: {backend}"),
        ]);
    }
    Ok(())
}

async fn status(globals: &Globals, paths: &CliPaths) -> Result<(), CliError> {
    let config = Config::load(paths)?;
    let selected = selection::select(
        CommandClass::B,
        paths,
        &config,
        &SelectionInput {
            profile_flag: globals.profile.as_deref(),
            offline: globals.offline,
            allow_new_profile: false,
        },
    )?;
    let open = token::open_store(&selected)?;
    let row = CredentialRow::load(&open.store, &selected.identity.key)?;
    let mut token_source: Option<&str> = None;
    if std::env::var("CANVAS_TOKEN")
        .ok()
        .is_some_and(|v| !v.is_empty())
    {
        token_source = Some("env");
    } else {
        match row.active_source {
            ActiveSource::Keyring => token_source = Some("keyring"),
            ActiveSource::File => token_source = Some("file"),
            ActiveSource::None => {}
        }
    }
    let stray = credentials::stray_sources(paths, &selected.identity.key, &row)?;
    let mut pending = Vec::new();
    if row.cleanup_keyring {
        pending.push("keyring");
    }
    if row.cleanup_file {
        pending.push("file");
    }
    let backend = if credentials::keyring_available() {
        "keyring"
    } else {
        "file"
    };

    let identity_ref = IdentityRef::new(
        &selected.identity.origin,
        selected.identity.user_id,
        selected.identity.key.as_str(),
    );
    let result = json!({
        "profile": selected.profile_name,
        "identity": {
            "origin": selected.identity.origin,
            "user_id": selected.identity.user_id.to_string(),
            "key": selected.identity.key.as_str(),
        },
        "token_source": token_source,
        "stray_sources": stray,
        "backend": backend,
        "validated_at": row.validated_at,
        "pending_cleanup": pending,
    });

    if globals.json {
        Envelope::new(
            "canvas-cli/auth_status@1",
            selected.profile_name.as_deref(),
            Some(identity_ref),
        )
        .with_result(result)
        .print_json();
    } else {
        let mut lines = vec![
            format!(
                "profile: {}",
                selected.profile_name.as_deref().unwrap_or("(none)")
            ),
            format!(
                "identity: {} / {}",
                selected.identity.origin, selected.identity.user_id
            ),
            format!("token source: {}", token_source.unwrap_or("none")),
            format!("backend: {backend}"),
        ];
        if let Some(v) = &row.validated_at {
            lines.push(format!("validated at: {v}"));
        }
        if !stray.is_empty() {
            lines.push(format!("stray sources: {}", stray.join(", ")));
        }
        if !pending.is_empty() {
            lines.push(format!("pending cleanup: {}", pending.join(", ")));
        }
        print_human(lines);
    }
    Ok(())
}

async fn logout(globals: &Globals, paths: &CliPaths) -> Result<(), CliError> {
    let config = Config::load(paths)?;
    let selected = selection::select(
        CommandClass::B,
        paths,
        &config,
        &SelectionInput {
            profile_flag: globals.profile.as_deref(),
            offline: globals.offline,
            allow_new_profile: false,
        },
    )?;
    let open = token::open_store(&selected)?;
    if std::env::var_os("CANVAS_TOKEN").is_some() {
        eprintln!("warning: CANVAS_TOKEN is still set in the environment");
    }
    let removed = credentials::logout(paths, &open.store, &selected.identity.key)?;
    let identity_ref = IdentityRef::new(
        &selected.identity.origin,
        selected.identity.user_id,
        selected.identity.key.as_str(),
    );
    let profile = selected
        .profile_name
        .clone()
        .unwrap_or_else(|| "default".into());
    let result = json!({
        "profile": profile,
        "identity": {
            "origin": selected.identity.origin,
            "user_id": selected.identity.user_id.to_string(),
            "key": selected.identity.key.as_str(),
        },
        "backend": null,
        "removed": removed,
    });
    if globals.json {
        Envelope::new(
            "canvas-cli/auth_logout@1",
            Some(&profile),
            Some(identity_ref),
        )
        .with_result(result)
        .print_json();
    } else {
        print_human([
            format!("logged out profile {profile}"),
            format!("removed: {}", removed.join(", ")),
        ]);
    }
    Ok(())
}

async fn token_cmd(globals: &Globals, paths: &CliPaths, reveal: bool) -> Result<(), CliError> {
    if reveal && globals.json {
        return Err(CliError::usage(
            "--json cannot be used with auth token --reveal",
        ));
    }
    let config = Config::load(paths)?;
    let selected = selection::select(
        CommandClass::B,
        paths,
        &config,
        &SelectionInput {
            profile_flag: globals.profile.as_deref(),
            offline: globals.offline,
            allow_new_profile: false,
        },
    )?;
    let open = token::open_store(&selected)?;
    let resolved = token::resolve_token(paths, &open.store, &selected.identity.key)?;
    if reveal {
        print!("{}", resolved.token.expose());
        if !resolved.token.expose().ends_with('\n') {
            println!();
        }
        return Ok(());
    }
    let identity_ref = IdentityRef::new(
        &selected.identity.origin,
        selected.identity.user_id,
        selected.identity.key.as_str(),
    );
    let result = json!({
        "token_source": resolved.source.as_str(),
        "sha256": credentials::token_sha256(resolved.token.expose()),
    });
    if globals.json {
        Envelope::new(
            "canvas-cli/auth_status@1",
            selected.profile_name.as_deref(),
            Some(identity_ref),
        )
        .with_result(result)
        .print_json();
    } else {
        print_human([
            format!("token source: {}", resolved.source.as_str()),
            format!(
                "sha256: {}",
                credentials::token_sha256(resolved.token.expose())
            ),
        ]);
    }
    let _ = TokenSource::Env;
    let _ = ExitKind::Ok;
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String, CliError> {
    if !io::stdin().is_terminal() {
        return Err(CliError::usage(
            "host prompt requires a terminal; pass --host or CANVAS_HOST",
        ));
    }
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| CliError::usage(format!("failed to read host: {e}")))?;
    Ok(line.trim().to_owned())
}

fn prompt_secret(prompt: &str) -> Result<String, CliError> {
    if !io::stdin().is_terminal() {
        return Err(CliError::usage(
            "token prompt requires a terminal; pass --token-stdin or CANVAS_TOKEN",
        ));
    }
    rpassword::prompt_password(prompt)
        .map(|s| s.trim().to_owned())
        .map_err(|_| CliError::local("failed to read hidden token from terminal"))
}

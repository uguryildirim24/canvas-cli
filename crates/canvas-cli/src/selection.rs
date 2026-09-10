//! Identity selection matrix and env bindings (§8).

#![allow(dead_code)] // CommandClass::C and bind helpers reserved for later commands.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use canvas_core::identity::{IdentityDocument, IdentityKey, IdentityLock, Paths as CorePaths};

use crate::config::{Config, Profile};
use crate::exit::CliError;
use crate::origin::canonicalize_origin;
use crate::paths::CliPaths;

/// Command class for selection and offline rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Identity-free local commands.
    A,
    /// Identity-bound local commands.
    B,
    /// Identity-bound cache-backed reads.
    C,
    /// Network-required commands.
    D,
}

/// Result of identity selection.
#[derive(Debug, Clone)]
pub struct Selected {
    pub profile_name: Option<String>,
    pub identity: IdentityDocument,
    pub core_paths: CorePaths,
    /// True when selected through the ephemeral `env` profile.
    pub from_env: bool,
    pub validation: Option<(
        canvas_api::models::User,
        reqwest::header::HeaderMap,
        canvas_api::Telemetry,
    )>,
}

/// Inputs that drive the selection matrix.
#[derive(Debug, Clone)]
pub struct SelectionInput<'a> {
    pub profile_flag: Option<&'a str>,
    pub offline: bool,
    /// When true, `auth login` may create a new profile name.
    pub allow_new_profile: bool,
}

/// Select an identity per the §8 matrix.
pub fn select(
    class: CommandClass,
    paths: &CliPaths,
    config: &Config,
    input: &SelectionInput<'_>,
) -> Result<Selected, CliError> {
    if class == CommandClass::A {
        return Err(CliError::usage(
            "internal: class A commands must not call select()",
        ));
    }

    let profile_env = std::env::var("CANVAS_PROFILE").ok();
    let profile_input = input.profile_flag.map(str::to_owned).or(profile_env);
    let host = std::env::var("CANVAS_HOST").ok().filter(|s| !s.is_empty());
    let token = std::env::var("CANVAS_TOKEN").ok().filter(|s| !s.is_empty());

    if let Some(name) = profile_input.as_deref() {
        if host.is_some() {
            eprintln!("warning: CANVAS_HOST is ignored when a profile is selected");
        }
        if let Some(profile) = config.profiles.get(name) {
            return selected_from_profile(paths, name, profile);
        }
        if input.allow_new_profile && class == CommandClass::D {
            return Err(CliError::auth(format!(
                "profile `{name}` does not exist yet; auth login will create it"
            )));
        }
        return Err(CliError::auth(format!("profile `{name}` does not exist")));
    }

    match (host.as_deref(), token.as_deref()) {
        (Some(_), Some(token)) => select_env_pair(
            class,
            paths,
            config,
            host.as_deref().unwrap(),
            token,
            input.offline,
        ),
        (None, Some(_token)) => {
            // Token alone applies to the selected or default profile.
            select_default(paths, config)
        }
        (Some(_), None) => Err(CliError::usage(
            "CANVAS_HOST alone is invalid; set CANVAS_TOKEN or use a profile",
        )),
        (None, None) => select_default(paths, config),
    }
}

fn select_default(paths: &CliPaths, config: &Config) -> Result<Selected, CliError> {
    let name = config.default_profile.as_deref().ok_or_else(|| {
        CliError::auth("no profile selected; run auth login or set --profile / CANVAS_PROFILE")
    })?;
    let profile = config.profiles.get(name).ok_or_else(|| {
        CliError::auth(format!(
            "default_profile `{name}` is missing from config; run auth login"
        ))
    })?;
    selected_from_profile(paths, name, profile)
}

fn selected_from_profile(
    paths: &CliPaths,
    name: &str,
    profile: &Profile,
) -> Result<Selected, CliError> {
    let key = IdentityKey::parse(&profile.key)?;
    let core_paths = CorePaths::for_identity(&paths.data_dir, &key);
    let identity = IdentityDocument::read(&core_paths.identity_json())?;
    core_paths.verify(&key)?;
    if identity.key != key
        || identity.origin != profile.origin
        || identity.user_id != profile.user_id
    {
        return Err(CliError::local("profile does not match identity.json"));
    }
    Ok(Selected {
        profile_name: Some(name.to_owned()),
        identity,
        core_paths,
        from_env: false,
        validation: None,
    })
}

fn select_env_pair(
    class: CommandClass,
    paths: &CliPaths,
    _config: &Config,
    host: &str,
    token: &str,
    offline: bool,
) -> Result<Selected, CliError> {
    let origin = canonicalize_origin(host)?;
    let binding_key = binding_hash(&origin, token);

    if let Some(key) = read_binding(paths, &binding_key)? {
        let core_paths = CorePaths::for_identity(&paths.data_dir, &key);
        match IdentityDocument::read(&core_paths.identity_json()) {
            Ok(identity)
                if identity.key == key
                    && identity.origin == origin
                    && IdentityLock::acquire_shared(&core_paths, &identity).is_ok() =>
            {
                return Ok(Selected {
                    profile_name: Some("env".into()),
                    identity,
                    core_paths,
                    from_env: true,
                    validation: None,
                });
            }
            _ => {
                remove_binding(paths, &binding_key)?;
            }
        }
    }

    match class {
        CommandClass::B => Err(CliError::auth(
            "run any online command or auth login to bind this token",
        )),
        CommandClass::C if offline => Err(CliError::auth(
            "run any online command or auth login to bind this token",
        )),
        CommandClass::C | CommandClass::D => Err(CliError::auth(
            "env identity is not bound; run auth login or an online command first",
        )),
        CommandClass::A => unreachable!(),
    }
}

/// After online `/users/self` for an unbound env pair, bind and select.
pub fn bind_env_identity(
    paths: &CliPaths,
    origin: &str,
    token: &str,
    user_id: i64,
) -> Result<Selected, CliError> {
    let key = IdentityKey::compute(origin, user_id);
    let core_paths = CorePaths::for_identity(&paths.data_dir, &key);
    let identity = IdentityDocument::read(&core_paths.identity_json()).map_err(|_| {
        CliError::auth("env token validates but no local identity exists; run auth login")
    })?;
    write_env_binding(paths, origin, token, &key)?;
    Ok(Selected {
        profile_name: Some("env".into()),
        identity,
        core_paths,
        from_env: true,
        validation: None,
    })
}

/// Record an env binding after online validation.
pub fn write_env_binding(
    paths: &CliPaths,
    origin: &str,
    token: &str,
    key: &IdentityKey,
) -> Result<(), CliError> {
    let hash = binding_hash(origin, token);
    with_bindings_lock(paths, |file| {
        file.bindings.insert(
            hash,
            BindingEntry {
                key: key.as_str().to_owned(),
            },
        );
        Ok(())
    })
}

fn read_binding(paths: &CliPaths, hash: &str) -> Result<Option<IdentityKey>, CliError> {
    let file = load_bindings(paths)?;
    match file
        .bindings
        .get(hash)
        .map(|entry| IdentityKey::parse(&entry.key))
    {
        Some(Ok(key)) => Ok(Some(key)),
        Some(Err(_)) => {
            remove_binding(paths, hash)?;
            Ok(None)
        }
        None => Ok(None),
    }
}

fn remove_binding(paths: &CliPaths, hash: &str) -> Result<(), CliError> {
    with_bindings_lock(paths, |file| {
        file.bindings.remove(hash);
        Ok(())
    })
}

fn binding_hash(origin: &str, token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin.as_bytes());
    hasher.update(b"\n");
    hasher.update(token.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BindingsFile {
    #[serde(default)]
    bindings: std::collections::BTreeMap<String, BindingEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BindingEntry {
    key: String,
}

fn load_bindings(paths: &CliPaths) -> Result<BindingsFile, CliError> {
    let path = paths.env_bindings_file();
    match fs::read_to_string(&path) {
        Ok(raw) if raw.trim().is_empty() => Ok(BindingsFile::default()),
        Ok(raw) => {
            toml::from_str(&raw).map_err(|e| CliError::local(format!("env-bindings.toml: {e}")))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BindingsFile::default()),
        Err(e) => Err(CliError::local(e.to_string())),
    }
}

fn with_bindings_lock(
    paths: &CliPaths,
    f: impl FnOnce(&mut BindingsFile) -> Result<(), CliError>,
) -> Result<(), CliError> {
    fs::create_dir_all(&paths.data_dir).map_err(|e| CliError::local(e.to_string()))?;
    let lock_path = paths.env_bindings_lock();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| CliError::local(e.to_string()))?;
    FileExt::lock_exclusive(&lock).map_err(|e| CliError::local(e.to_string()))?;

    let mut file = load_bindings(paths)?;
    f(&mut file)?;

    let path = paths.env_bindings_file();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".env-bindings.{}.tmp", uuid::Uuid::new_v4()));
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|e| CliError::local(e.to_string()))?;
            let raw = toml::to_string_pretty(&file).map_err(|e| CliError::local(e.to_string()))?;
            out.write_all(raw.as_bytes())
                .map_err(|e| CliError::local(e.to_string()))?;
            out.sync_all().map_err(|e| CliError::local(e.to_string()))?;
        }
        #[cfg(not(unix))]
        {
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| CliError::local(e.to_string()))?;
            let raw = toml::to_string_pretty(&file).map_err(|e| CliError::local(e.to_string()))?;
            out.write_all(raw.as_bytes())
                .map_err(|e| CliError::local(e.to_string()))?;
            out.sync_all().map_err(|e| CliError::local(e.to_string()))?;
        }
    }
    fs::rename(&tmp, &path).map_err(|e| CliError::local(e.to_string()))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| CliError::local(e.to_string()))?;
    Ok(())
}

/// Open identity paths for a known key without the selection matrix.
pub fn paths_for_key(paths: &CliPaths, key: &IdentityKey) -> CorePaths {
    CorePaths::for_identity(&paths.data_dir, key)
}

/// Scan the data root for `identity.json` documents.
pub fn list_identities(paths: &CliPaths) -> Result<Vec<IdentityDocument>, CliError> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(&paths.data_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(CliError::local(e.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| CliError::local(e.to_string()))?;
        let meta = entry
            .metadata()
            .map_err(|e| CliError::local(e.to_string()))?;
        if !meta.is_dir() {
            continue;
        }
        let json = entry.path().join("identity.json");
        if !json.exists() {
            continue;
        }
        match IdentityDocument::read(&json) {
            Ok(doc) => {
                let core = CorePaths::for_identity(&paths.data_dir, &doc.key);
                if core.identity_dir != entry.path() {
                    return Err(CliError::local("identity directory does not match key"));
                }
                let _lock = IdentityLock::acquire_shared(&core, &doc)?;
                out.push(doc);
            }
            Err(e) => eprintln!("warning: skipping {}: {e}", entry.path().display()),
        }
    }
    out.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
    Ok(out)
}

/// Directory size in bytes (best-effort).
pub fn dir_size(path: &Path) -> u64 {
    fn walk(p: &Path, acc: &mut u64) {
        let Ok(entries) = fs::read_dir(p) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk(&entry.path(), acc);
            } else {
                *acc += meta.len();
            }
        }
    }
    let mut n = 0u64;
    walk(path, &mut n);
    n
}

/// Read a file fully (helper for tests / doctor).
pub fn read_to_string(path: &Path) -> Result<String, CliError> {
    let mut f = File::open(path).map_err(|e| CliError::local(e.to_string()))?;
    let mut s = String::new();
    f.read_to_string(&mut s)
        .map_err(|e| CliError::local(e.to_string()))?;
    Ok(s)
}

/// Select an online identity, validating every class-D env pair before trusting it.
pub async fn select_online(
    class: CommandClass,
    paths: &CliPaths,
    config: &Config,
    input: &SelectionInput<'_>,
) -> Result<Selected, CliError> {
    if class == CommandClass::D && input.offline {
        return Err(CliError::usage("command requires the network"));
    }
    let explicit = input.profile_flag.is_some() || std::env::var_os("CANVAS_PROFILE").is_some();
    if !explicit && !input.offline && matches!(class, CommandClass::C | CommandClass::D) {
        if let (Ok(host), Ok(token)) = (std::env::var("CANVAS_HOST"), std::env::var("CANVAS_TOKEN"))
        {
            if !host.is_empty() && !token.is_empty() {
                let previous = match select(class, paths, config, input) {
                    Ok(selected) => Some(selected),
                    Err(error) if error.kind == crate::exit::ExitKind::Auth => None,
                    Err(error) => return Err(error),
                };
                if class == CommandClass::C {
                    if let Some(selected) = previous {
                        return Ok(selected);
                    }
                }
                let origin = canonicalize_origin(&host)?;
                let validation =
                    crate::token::validate_users_self_details(&origin, &token, &config.network)
                        .await?;
                let user = &validation.0;
                if let Some(selected) = &previous {
                    if selected.identity.user_id != user.id {
                        return Err(CliError::auth("identity mismatch")
                            .for_selected(selected)
                            .with_requests(validation.2));
                    }
                }
                let (identity, _lock) = initialize_identity(paths, &origin, user.id)?;
                write_env_binding(paths, &origin, &token, &identity.key)?;
                let core_paths = CorePaths::for_identity(&paths.data_dir, &identity.key);
                return Ok(Selected {
                    profile_name: Some("env".into()),
                    identity,
                    core_paths,
                    from_env: true,
                    validation: Some(validation),
                });
            }
        }
    }
    select(class, paths, config, input)
}

/// Initialize once under the credential lock and a shared identity lock.
/// Retaining the returned lock prevents removal until the caller opens its store.
pub fn initialize_identity(
    paths: &CliPaths,
    origin: &str,
    user_id: i64,
) -> Result<(IdentityDocument, File), CliError> {
    let candidate =
        IdentityDocument::new(origin, user_id, crate::output::now_timestamp().to_string());
    let core = CorePaths::for_identity(&paths.data_dir, &candidate.key);
    core.verify(&candidate.key)?;
    fs::create_dir_all(paths.data_dir.join("locks")).map_err(|e| CliError::local(e.to_string()))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&core.lock_path)
        .map_err(|e| CliError::local(e.to_string()))?;
    FileExt::lock_shared(&lock).map_err(|e| CliError::local(e.to_string()))?;
    let _credential_lock = crate::credentials::CredLock::acquire(paths, &candidate.key)?;
    core.verify(&candidate.key)?;
    let identity = if core.identity_json().exists() {
        IdentityDocument::read(&core.identity_json())?
    } else {
        candidate.write(&core.identity_json())?;
        candidate
    };
    if identity.origin != origin
        || identity.user_id != user_id
        || identity.key != IdentityKey::compute(origin, user_id)
    {
        return Err(CliError::local("identity mismatch"));
    }
    Ok((identity, lock))
}

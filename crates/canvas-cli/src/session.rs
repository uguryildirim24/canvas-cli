//! Minimal identity session open until M0-c lands.

use std::fs;
use std::path::{Path, PathBuf};

use canvas_api::{Client, Secret};
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use etcetera::{BaseStrategy, choose_base_strategy};
use reqwest::Url;
use serde::Deserialize;
use thiserror::Error;

const USER_AGENT: &str = concat!("canvas-cli/", env!("CARGO_PKG_VERSION"));

/// Open identity plus optional network client.
pub struct Session {
    pub profile: Option<String>,
    pub identity: IdentityDocument,
    pub paths: Paths,
    pub open: OpenIdentity,
    /// `None` when `--offline` or no `CANVAS_TOKEN`.
    pub client: Option<Client>,
}

/// Failures opening a session.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Missing profile / identity / token selection (exit 3).
    #[error("{0}")]
    Auth(String),
    /// Local persistence / path / identity mismatch (exit 13).
    #[error("{0}")]
    Local(String),
}

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    default_profile: Option<String>,
    #[serde(default)]
    profiles: std::collections::BTreeMap<String, ProfileEntry>,
    #[serde(default)]
    cache: CacheConfig,
}

#[derive(Debug, Deserialize, Default)]
struct ProfileEntry {
    key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CacheConfig {
    ttl_courses: Option<String>,
    ttl_grades: Option<String>,
}

impl Session {
    /// Open a session for class B/C/D commands.
    ///
    /// Identity selection (until M0-c):
    /// 1. `CANVAS_IDENTITY_KEY`
    /// 2. `--profile` → `config.toml` `[profiles.<name>].key`
    /// 3. `default_profile` from config
    /// 4. else exit 3
    pub fn open(profile: Option<&str>, offline: bool) -> Result<Self, SessionError> {
        let data_root = data_root()?;
        let config = read_config();
        let (profile_name, key) = resolve_identity_key(profile, &config)?;
        let identity_dir = data_root.join(&key);
        let identity_json = identity_dir.join("identity.json");
        let identity = IdentityDocument::read(&identity_json)
            .map_err(|e| SessionError::Auth(format!("cannot read identity {key}: {e}")))?;
        if identity.key.as_str() != key {
            return Err(SessionError::Local(
                "identity key does not match directory".into(),
            ));
        }
        let paths = Paths::for_identity(&data_root, &identity.key);
        let open = OpenIdentity::open(&paths, &identity)
            .map_err(|e| SessionError::Local(format!("cannot open identity store: {e}")))?;

        let client = if offline {
            None
        } else {
            match std::env::var("CANVAS_TOKEN") {
                Ok(token) if !token.is_empty() => {
                    let origin = Url::parse(&identity.origin).map_err(|e| {
                        SessionError::Local(format!("invalid identity origin: {e}"))
                    })?;
                    let client = Client::new(origin, Secret::new(token), USER_AGENT)
                        .map_err(|e| SessionError::Local(format!("cannot build client: {e}")))?;
                    Some(client)
                }
                _ => None,
            }
        };

        Ok(Self {
            profile: profile_name,
            identity,
            paths,
            open,
            client,
        })
    }

    /// Identity ref for envelopes.
    #[must_use]
    pub fn identity_ref(&self) -> crate::output::IdentityRef {
        crate::output::IdentityRef {
            origin: self.identity.origin.clone(),
            user_id: self.identity.user_id.to_string(),
            key: self.identity.key.to_string(),
        }
    }
}

/// Resolve the data root: `CANVAS_DATA_ROOT` or etcetera data dir / `canvas-cli`.
pub fn data_root() -> Result<PathBuf, SessionError> {
    if let Ok(root) = std::env::var("CANVAS_DATA_ROOT") {
        let path = PathBuf::from(root);
        if path.as_os_str().is_empty() {
            return Err(SessionError::Auth("CANVAS_DATA_ROOT is empty".into()));
        }
        return Ok(path);
    }
    let strategy = choose_base_strategy()
        .map_err(|e| SessionError::Local(format!("cannot locate data directory: {e}")))?;
    Ok(strategy.data_dir().join("canvas-cli"))
}

/// Config directory: etcetera config dir / `canvas-cli`.
fn config_dir() -> Option<PathBuf> {
    choose_base_strategy()
        .ok()
        .map(|s| s.config_dir().join("canvas-cli"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

fn read_config() -> ConfigFile {
    let Some(path) = config_path() else {
        return ConfigFile::default();
    };
    match fs::read_to_string(&path) {
        Ok(raw) => toml::from_str(&raw).unwrap_or_default(),
        Err(_) => ConfigFile::default(),
    }
}

fn resolve_identity_key(
    profile_flag: Option<&str>,
    config: &ConfigFile,
) -> Result<(Option<String>, String), SessionError> {
    if let Ok(key) = std::env::var("CANVAS_IDENTITY_KEY") {
        if key.is_empty() {
            return Err(SessionError::Auth("CANVAS_IDENTITY_KEY is empty".into()));
        }
        return Ok((profile_flag.map(str::to_owned), key));
    }

    if let Some(name) = profile_flag {
        let entry = config.profiles.get(name).ok_or_else(|| {
            SessionError::Auth(format!("profile {name:?} not found in config.toml"))
        })?;
        let key = entry
            .key
            .as_ref()
            .ok_or_else(|| SessionError::Auth(format!("profile {name:?} has no identity key")))?;
        return Ok((Some(name.to_owned()), key.clone()));
    }

    if let Some(name) = config.default_profile.as_deref() {
        let entry = config.profiles.get(name).ok_or_else(|| {
            SessionError::Auth(format!("default_profile {name:?} not found in config.toml"))
        })?;
        let key = entry.key.as_ref().ok_or_else(|| {
            SessionError::Auth(format!("default_profile {name:?} has no identity key"))
        })?;
        return Ok((Some(name.to_owned()), key.clone()));
    }

    Err(SessionError::Auth(
        "no identity selected; set CANVAS_IDENTITY_KEY, --profile, or default_profile".into(),
    ))
}

/// Default courses TTL (6h), optionally overridden by config.
#[must_use]
pub fn ttl_courses() -> jiff::Span {
    parse_ttl(read_config().cache.ttl_courses.as_deref(), 6, true)
}

/// Default grades TTL (10m), optionally overridden by config.
#[must_use]
pub fn ttl_grades() -> jiff::Span {
    parse_ttl(read_config().cache.ttl_grades.as_deref(), 10, false)
}

fn parse_ttl(raw: Option<&str>, default_amount: i64, hours: bool) -> jiff::Span {
    let Some(raw) = raw else {
        return if hours {
            jiff::Span::new().hours(default_amount)
        } else {
            jiff::Span::new().minutes(default_amount)
        };
    };
    if let Some(h) = raw.strip_suffix('h')
        && let Ok(n) = h.parse::<i64>()
    {
        return jiff::Span::new().hours(n);
    }
    if let Some(m) = raw.strip_suffix('m')
        && let Ok(n) = m.parse::<i64>()
    {
        return jiff::Span::new().minutes(n);
    }
    if hours {
        jiff::Span::new().hours(default_amount)
    } else {
        jiff::Span::new().minutes(default_amount)
    }
}

/// Ensure a path exists as a directory (tests / helpers).
#[allow(dead_code)]
pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

//! Minimal identity session open until M0-c lands.

use std::fs;
use std::path::{Path, PathBuf};

use canvas_api::{Client, Secret};
use canvas_core::identity::{IdentityDocument, Paths};
use canvas_core::store::OpenIdentity;
use etcetera::{BaseStrategy, choose_base_strategy};
use reqwest::Url;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use sha2::{Digest, Sha256};
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
    client_init_error: Option<canvas_api::Error>,
}

/// Failures opening a session.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Missing profile / identity / token selection (exit 3).
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    Usage(String),
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
    origin: Option<String>,
    user_id: Option<i64>,
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
        let config = read_config()?;
        let env_profile = std::env::var("CANVAS_PROFILE").ok();
        let profile = profile.or(env_profile.as_deref());
        if profile.is_none()
            && std::env::var_os("CANVAS_HOST").is_some()
            && std::env::var_os("CANVAS_TOKEN").is_none()
        {
            return Err(SessionError::Usage(
                "CANVAS_HOST requires CANVAS_TOKEN".into(),
            ));
        }
        // The M0-c bridge must never send an env token to a different default origin.
        let env_host = std::env::var("CANVAS_HOST").ok();
        let (profile_name, key) = resolve_identity_key(profile, &config)?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            || matches!(key.as_str(), "." | "..")
        {
            return Err(SessionError::Local("unsafe identity key".into()));
        }
        let identity_dir = data_root.join(&key);
        let identity_json = identity_dir.join("identity.json");
        let identity = IdentityDocument::read(&identity_json)
            .map_err(|e| SessionError::Local(format!("cannot read identity {key}: {e}")))?;
        if identity.key.as_str() != key {
            return Err(SessionError::Local(
                "identity key does not match directory".into(),
            ));
        }
        if let Some(entry) = profile_name
            .as_ref()
            .and_then(|name| config.profiles.get(name))
            && (entry.origin.as_ref().is_some_and(|v| v != &identity.origin)
                || entry.user_id.is_some_and(|v| v != identity.user_id))
        {
            return Err(SessionError::Local("profile and identity disagree".into()));
        }
        if profile.is_none()
            && let Some(host) = env_host
        {
            let raw = if host.contains("://") {
                host
            } else {
                format!("https://{host}")
            };
            let supplied =
                Url::parse(&raw).map_err(|_| SessionError::Auth("invalid CANVAS_HOST".into()))?;
            let origin = Url::parse(&identity.origin)
                .map_err(|_| SessionError::Local("invalid identity origin".into()))?;
            if supplied.origin() != origin.origin() {
                return Err(SessionError::Auth(
                    "environment token has no binding for this origin; run auth login".into(),
                ));
            }
        }
        let paths = Paths::for_identity(&data_root, &identity.key);
        let open = OpenIdentity::open(&paths, &identity)
            .map_err(|e| SessionError::Local(format!("cannot open identity store: {e}")))?;

        let mut client_init_error = None;
        let client = if offline {
            None
        } else {
            match std::env::var("CANVAS_TOKEN") {
                Ok(token) if !token.is_empty() => {
                    let origin = Url::parse(&identity.origin).map_err(|e| {
                        SessionError::Local(format!("invalid identity origin: {e}"))
                    })?;
                    match Client::new(origin, Secret::new(token), USER_AGENT) {
                        Ok(client) => Some(client),
                        Err(error) => {
                            client_init_error = Some(error);
                            None
                        }
                    }
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
            client_init_error,
        })
    }

    /// Validate a newly seen environment token before it can populate this identity's cache.
    /// Client construction failure, if any (for auth vs network exits).
    #[must_use]
    pub fn client_init_error(&self) -> Option<&canvas_api::Error> {
        self.client_init_error.as_ref()
    }

    pub async fn validate_network_token(&self) -> Result<(), canvas_core::sync::SyncError> {
        let Some(client) = &self.client else {
            return Err(
                if matches!(self.client_init_error, Some(canvas_api::Error::Network)) {
                    canvas_api::Error::Network
                } else {
                    canvas_api::Error::Unauthorized
                }
                .into(),
            );
        };
        let hash = format!("{:x}", Sha256::digest(client.token().expose().as_bytes()));
        let key = self.identity.key.to_string();
        let recorded = self.open.store.call({ let key = key.clone(); move |conns| {
            Ok(conns.state.query_row("SELECT token_sha256 FROM credential WHERE identity_key=?1 AND validated_at IS NOT NULL", [key], |r| r.get::<_,Option<String>>(0)).optional()?.flatten())
        }}).await?;
        if recorded.as_deref() == Some(&hash) {
            return Ok(());
        }
        let user: canvas_api::models::User = client.get("/api/v1/users/self").await?;
        if user.id != self.identity.user_id {
            return Err(canvas_api::Error::Unauthorized.into());
        }
        let at = crate::output::generated_at_now();
        self.open.store.call(move |conns| {
            let tx = conns.state.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute("INSERT INTO credential (identity_key,token_sha256,validated_at) VALUES (?1,?2,?3) ON CONFLICT(identity_key) DO UPDATE SET token_sha256=excluded.token_sha256,validated_at=excluded.validated_at", rusqlite::params![key, hash, at])?;
            tx.commit()?; Ok(())
        }).await?;
        Ok(())
    }

    pub fn requests(&self) -> crate::output::Requests {
        self.client
            .as_ref()
            .map_or_else(crate::output::Requests::default, |c| {
                let t = c.telemetry();
                crate::output::Requests {
                    api: t.api,
                    storage: t.storage,
                    cost: t.cost,
                }
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

fn read_config() -> Result<ConfigFile, SessionError> {
    let Some(path) = config_path() else {
        return Ok(ConfigFile::default());
    };
    match fs::read_to_string(&path) {
        Ok(raw) => {
            toml::from_str(&raw).map_err(|_| SessionError::Local("invalid config.toml".into()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(SessionError::Local(format!("cannot read config.toml: {e}"))),
    }
}

fn resolve_identity_key(
    profile_flag: Option<&str>,
    config: &ConfigFile,
) -> Result<(Option<String>, String), SessionError> {
    if profile_flag.is_none()
        && let Ok(key) = std::env::var("CANVAS_IDENTITY_KEY")
    {
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
    parse_ttl(
        read_config()
            .unwrap_or_default()
            .cache
            .ttl_courses
            .as_deref(),
        6,
        true,
    )
}

/// Default grades TTL (10m), optionally overridden by config.
#[must_use]
pub fn ttl_grades() -> jiff::Span {
    parse_ttl(
        read_config()
            .unwrap_or_default()
            .cache
            .ttl_grades
            .as_deref(),
        10,
        false,
    )
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
        && n >= 0
        && let Ok(span) = jiff::Span::new().try_hours(n)
    {
        return span;
    }
    if let Some(m) = raw.strip_suffix('m')
        && let Ok(n) = m.parse::<i64>()
        && n >= 0
        && let Ok(span) = jiff::Span::new().try_minutes(n)
    {
        return span;
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

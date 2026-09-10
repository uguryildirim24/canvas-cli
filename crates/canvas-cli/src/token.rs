//! Token resolution and `/users/self` validation (§8).

use canvas_api::models::User;
use canvas_api::{Client, Error as ApiError, Secret};
use canvas_core::identity::IdentityKey;
use canvas_core::store::{OpenIdentity, Store};

use crate::credentials::{self, ActiveSource, CredError, CredentialRow};
use crate::exit::{CliError, ExitKind};
use crate::origin::origin_url;
use crate::paths::CliPaths;
use crate::selection::Selected;

const USER_AGENT: &str = concat!("canvas-cli/", env!("CARGO_PKG_VERSION"));

/// Resolved bearer token and its source label for status.
#[derive(Debug, Clone)]
pub struct ResolvedToken {
    pub token: String,
    pub source: TokenSource,
}

/// Where the token came from for this invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Env,
    Keyring,
    File,
}

impl TokenSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Keyring => "keyring",
            Self::File => "file",
        }
    }
}

/// Resolve a token: `CANVAS_TOKEN` → active store with hash check.
pub fn resolve_token(
    paths: &CliPaths,
    store: &Store,
    key: &IdentityKey,
) -> Result<ResolvedToken, CliError> {
    if let Ok(token) = std::env::var("CANVAS_TOKEN") {
        if !token.is_empty() {
            return Ok(ResolvedToken {
                token,
                source: TokenSource::Env,
            });
        }
    }

    let row = CredentialRow::load(store, key)?;
    if row.active_source == ActiveSource::None || row.token_sha256.is_none() {
        return Err(CliError::auth(
            "no stored token; create one at https://<host>/profile/settings then run auth login",
        ));
    }
    let expected = row.token_sha256.as_deref().unwrap();
    let token = credentials::read_token(paths, key, row.active_source).map_err(|e| match e {
        CredError::NotFound => CliError::auth(
            "stored token does not match the recorded one; run auth login",
        ),
        other => CliError::from(other),
    })?;
    if credentials::token_sha256(&token) != expected {
        return Err(CliError::auth(
            "stored token does not match the recorded one; run auth login",
        ));
    }
    let source = match row.active_source {
        ActiveSource::Keyring => TokenSource::Keyring,
        ActiveSource::File => TokenSource::File,
        ActiveSource::None => unreachable!(),
    };
    Ok(ResolvedToken { token, source })
}

/// Call `GET /users/self` and map outcomes to exits 3/4.
pub async fn validate_users_self(origin: &str, token: &str) -> Result<User, CliError> {
    let url = origin_url(origin)?;
    let client = Client::new(url, Secret::new(token), USER_AGENT)
        .map_err(|e| CliError::usage(format!("invalid origin for API client: {e}")))?;
        match client.get::<User>("/api/v1/users/self").await {
        Ok(user) => Ok(user),
        Err(ApiError::Unauthorized) => Err(CliError::auth("token rejected")),
        Err(ApiError::Network) => Err(CliError::network("network failure talking to Canvas")),
        Err(ApiError::RateLimited) => Err(CliError::new(ExitKind::Generic, "rate limited")),
        Err(other) => Err(CliError::network(format!("Canvas API error: {other}"))),
    }
}

/// Validate that `/users/self` matches an expected user id.
pub async fn validate_identity_user(
    origin: &str,
    token: &str,
    expected_user_id: i64,
) -> Result<User, CliError> {
    let user = validate_users_self(origin, token).await?;
    if user.id != expected_user_id {
        return Err(CliError::auth("identity mismatch"));
    }
    Ok(user)
}

/// Open the store for a selected identity.
pub fn open_store(selected: &Selected) -> Result<OpenIdentity, CliError> {
    OpenIdentity::open(&selected.core_paths, &selected.identity).map_err(CliError::from)
}

/// User-Agent string for API clients.
#[must_use]
pub fn user_agent() -> &'static str {
    USER_AGENT
}

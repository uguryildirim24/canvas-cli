//! CLI config and data directory resolution (§9).

use std::path::PathBuf;

use etcetera::{BaseStrategy, choose_base_strategy};

use crate::exit::{CliError, ExitKind};

const APP_DIR: &str = "canvas-cli";

/// Resolved config and data roots for this process.
#[derive(Debug, Clone)]
pub struct CliPaths {
    /// Config directory (`…/canvas-cli/`).
    pub config_dir: PathBuf,
    /// Data root (`…/canvas-cli/` or Windows `…/canvas-cli/data/`).
    pub data_dir: PathBuf,
}

impl CliPaths {
    /// Resolve paths from env overrides or the platform base strategy.
    pub fn resolve() -> Result<Self, CliError> {
        let config_dir = if let Ok(dir) = std::env::var("CANVAS_CONFIG_DIR") {
            PathBuf::from(dir)
        } else {
            let strategy = choose_base_strategy().map_err(|e| {
                CliError::new(
                    ExitKind::Local,
                    format!("cannot resolve home directories: {e}"),
                )
            })?;
            strategy.config_dir().join(APP_DIR)
        };

        let data_dir = if let Ok(dir) = std::env::var("CANVAS_DATA_DIR") {
            PathBuf::from(dir)
        } else {
            #[cfg(windows)]
            {
                let strategy = choose_base_strategy().map_err(|e| {
                    CliError::new(
                        ExitKind::Local,
                        format!("cannot resolve home directories: {e}"),
                    )
                })?;
                // SPEC §9: `%LOCALAPPDATA%\canvas-cli\data\`.
                strategy.cache_dir().join(APP_DIR).join("data")
            }
            #[cfg(not(windows))]
            {
                let strategy = choose_base_strategy().map_err(|e| {
                    CliError::new(
                        ExitKind::Local,
                        format!("cannot resolve home directories: {e}"),
                    )
                })?;
                strategy.data_dir().join(APP_DIR)
            }
        };

        Ok(Self {
            config_dir,
            data_dir,
        })
    }

    /// Path to `config.toml`.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Fallback credentials file (Unix only).
    #[must_use]
    pub fn credentials_file(&self) -> PathBuf {
        self.config_dir.join("credentials.toml")
    }

    /// Lock file for the credentials fallback.
    #[must_use]
    pub fn credentials_lock(&self) -> PathBuf {
        self.config_dir.join("credentials.lock")
    }

    /// Env-binding TOML path.
    #[must_use]
    pub fn env_bindings_file(&self) -> PathBuf {
        self.data_dir.join("env-bindings.toml")
    }

    /// Env-binding lock path.
    #[must_use]
    pub fn env_bindings_lock(&self) -> PathBuf {
        self.data_dir.join("env-bindings.lock")
    }

    /// Per-identity credential lock.
    #[must_use]
    pub fn cred_lock(&self, key: &str) -> PathBuf {
        self.data_dir.join("locks").join(format!("{key}.cred.lock"))
    }
}

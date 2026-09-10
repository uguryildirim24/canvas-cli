//! Config load, validate, get, set, and edit (§9).

#![allow(dead_code)]

use fs4::fs_std::FileExt;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exit::CliError;
use crate::paths::CliPaths;

/// Full typed configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub download: DownloadConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub bridge: BridgeConfig,
}

/// Named profile pointing at an identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub origin: String,
    pub user_id: i64,
    pub key: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
}

/// Download defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadConfig {
    #[serde(default)]
    pub dest: Option<String>,
    #[serde(default = "default_jobs")]
    pub jobs: u32,
}

fn default_jobs() -> u32 {
    4
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            dest: None,
            jobs: default_jobs(),
        }
    }
}

/// Cache TTL strings (figment keeps the SPEC duration form).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheConfig {
    #[serde(default = "ttl_courses")]
    pub ttl_courses: String,
    #[serde(default = "ttl_short")]
    pub ttl_assignments: String,
    #[serde(default = "ttl_short")]
    pub ttl_planner: String,
    #[serde(default = "ttl_short")]
    pub ttl_missing: String,
    #[serde(default = "ttl_short")]
    pub ttl_grades: String,
    #[serde(default = "ttl_hour")]
    pub ttl_modules: String,
    #[serde(default = "ttl_hour")]
    pub ttl_files: String,
    #[serde(default = "ttl_announcements")]
    pub ttl_announcements: String,
    #[serde(default = "ttl_hour")]
    pub ttl_calendar: String,
}

fn ttl_courses() -> String {
    "6h".into()
}
fn ttl_short() -> String {
    "10m".into()
}
fn ttl_hour() -> String {
    "1h".into()
}
fn ttl_announcements() -> String {
    "15m".into()
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_courses: ttl_courses(),
            ttl_assignments: ttl_short(),
            ttl_planner: ttl_short(),
            ttl_missing: ttl_short(),
            ttl_grades: ttl_short(),
            ttl_modules: ttl_hour(),
            ttl_files: ttl_hour(),
            ttl_announcements: ttl_announcements(),
            ttl_calendar: ttl_hour(),
        }
    }
}

/// Network concurrency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkConfig {
    #[serde(default = "default_jobs")]
    pub api_concurrency: u32,
    #[serde(default = "default_jobs")]
    pub storage_concurrency: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            api_concurrency: default_jobs(),
            storage_concurrency: default_jobs(),
        }
    }
}

/// Output preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputConfig {
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_color() -> String {
    "auto".into()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            color: default_color(),
        }
    }
}

/// Companion broker settings (M7-a).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeConfig {
    /// How long a hidden tab keeps sharing before the companion pauses it.
    #[serde(default = "default_pause_hidden_after")]
    pub pause_hidden_after: String,
    /// The one Chrome extension id allowed to start `bridge host`.
    #[serde(default)]
    pub extension_id: Option<String>,
}

fn default_pause_hidden_after() -> String {
    "10m".into()
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            pause_hidden_after: default_pause_hidden_after(),
            extension_id: None,
        }
    }
}

/// `bridge.pause_hidden_after` when the config says nothing, in milliseconds.
pub const DEFAULT_PAUSE_HIDDEN_MS: u64 = 10 * 60 * 1000;

/// Parse `bridge.pause_hidden_after` into milliseconds.
///
/// The SPEC duration form is `<n>h` or `<n>m`, as every `cache.ttl_*` key
/// uses. Anything else falls back to the default rather than sharing for
/// longer than the person asked for.
#[must_use]
pub fn pause_hidden_after_ms(raw: &str) -> u64 {
    let parse = |text: &str, unit: u64| -> Option<u64> {
        text.parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(unit))
            .filter(|ms| *ms > 0)
    };
    if let Some(hours) = raw.strip_suffix('h')
        && let Some(ms) = parse(hours, 60 * 60 * 1000)
    {
        return ms;
    }
    if let Some(minutes) = raw.strip_suffix('m')
        && let Some(ms) = parse(minutes, 60 * 1000)
    {
        return ms;
    }
    if let Some(seconds) = raw.strip_suffix('s')
        && let Some(ms) = parse(seconds, 1000)
    {
        return ms;
    }
    DEFAULT_PAUSE_HIDDEN_MS
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: None,
            profiles: BTreeMap::new(),
            download: DownloadConfig::default(),
            cache: CacheConfig::default(),
            network: NetworkConfig::default(),
            output: OutputConfig::default(),
            bridge: BridgeConfig::default(),
        }
    }
}

impl Config {
    /// Load defaults → `config.toml` → `CANVAS_*` env (config keys only).
    pub fn load(paths: &CliPaths) -> Result<Self, CliError> {
        let mut figment = Self::file_figment(paths);
        // Ignore auth/path/test env vars that share the CANVAS_ prefix.
        figment = figment.merge(
            Env::prefixed("CANVAS_")
                .ignore(&[
                    "TOKEN",
                    "HOST",
                    "PROFILE",
                    "CONFIG_DIR",
                    "DATA_DIR",
                    "TEST_FORCE_FILE",
                    "TEST_CRASH_AFTER",
                    "TEST_KEYRING",
                    "NOW",
                ])
                .map(|key| {
                    let raw = key.as_str().to_ascii_lowercase();
                    let mapped = if raw.contains("__") {
                        raw.replace("__", ".")
                    } else if let Some((root, field)) = raw.split_once('_') {
                        if ["download", "cache", "network", "output", "bridge"].contains(&root) {
                            format!("{root}.{field}")
                        } else {
                            raw
                        }
                    } else {
                        raw
                    };
                    mapped.into()
                }),
        );
        figment
            .extract()
            .map_err(|e| CliError::local(format!("config parse error: {e}")))
    }

    pub fn load_with_flags(
        paths: &CliPaths,
        globals: &crate::cli::Globals,
    ) -> Result<Self, CliError> {
        let mut config = Self::load(paths)?;
        if let Some(color) = &globals.color {
            config.output.color = match color {
                crate::cli::ColorChoice::Auto => "auto",
                crate::cli::ColorChoice::Always => "always",
                crate::cli::ColorChoice::Never => "never",
            }
            .into();
        }
        Ok(config)
    }

    fn file_figment(paths: &CliPaths) -> Figment {
        Figment::new()
            .merge(Serialized::defaults(Config::default()))
            .merge(Toml::file(paths.config_file()))
    }

    /// Read persistent settings without baking temporary environment overrides into them.
    pub fn load_file(paths: &CliPaths) -> Result<Self, CliError> {
        Self::file_figment(paths)
            .extract()
            .map_err(|_| CliError::local("config parse error"))
    }

    /// Persist the config atomically.
    pub fn save(&self, paths: &CliPaths) -> Result<(), CliError> {
        fs::create_dir_all(&paths.config_dir).map_err(|e| {
            CliError::local(format!(
                "cannot create config dir {}: {e}",
                paths.config_dir.display()
            ))
        })?;
        let raw = toml::to_string_pretty(self)
            .map_err(|e| CliError::local(format!("config serialize error: {e}")))?;
        atomic_write(&paths.config_file(), raw.as_bytes())?;
        Ok(())
    }

    /// Get a dotted key as JSON.
    pub fn get_value(&self, key: &str) -> Result<Value, CliError> {
        validate_key(key)?;
        let root = serde_json::to_value(self)
            .map_err(|e| CliError::local(format!("config encode error: {e}")))?;
        lookup_path(&root, key)
            .cloned()
            .ok_or_else(|| CliError::usage(format!("unknown config key `{key}`")))
    }

    /// Set a dotted key; returns the previous JSON value when present.
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<Option<Value>, CliError> {
        validate_key(key)?;
        let previous = self.get_value(key).ok();
        apply_set(self, key, value)?;
        Ok(previous)
    }
}

const KNOWN_TOP: &[&str] = &[
    "default_profile",
    "download.dest",
    "download.jobs",
    "cache.ttl_courses",
    "cache.ttl_assignments",
    "cache.ttl_planner",
    "cache.ttl_missing",
    "cache.ttl_grades",
    "cache.ttl_modules",
    "cache.ttl_files",
    "cache.ttl_announcements",
    "cache.ttl_calendar",
    "network.api_concurrency",
    "network.storage_concurrency",
    "output.color",
    "bridge.pause_hidden_after",
    "bridge.extension_id",
];

fn validate_key(key: &str) -> Result<(), CliError> {
    if KNOWN_TOP.contains(&key) {
        return Ok(());
    }
    if let Some(rest) = key.strip_prefix("profiles.") {
        let mut parts = rest.split('.');
        let name = parts.next().unwrap_or("");
        let field = parts.next().unwrap_or("");
        if parts.next().is_none()
            && !name.is_empty()
            && matches!(field, "origin" | "user_id" | "key" | "name" | "time_zone")
        {
            return Ok(());
        }
    }
    Err(CliError::usage(format!("unknown config key `{key}`")))
}

fn lookup_path<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = value;
    for part in key.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

fn apply_set(config: &mut Config, key: &str, value: &str) -> Result<(), CliError> {
    match key {
        "default_profile" => {
            config.default_profile = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
        }
        "download.dest" => {
            config.download.dest = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
        }
        "download.jobs" => {
            config.download.jobs = parse_u32(value)?;
        }
        "cache.ttl_courses" => config.cache.ttl_courses = value.to_owned(),
        "cache.ttl_assignments" => config.cache.ttl_assignments = value.to_owned(),
        "cache.ttl_planner" => config.cache.ttl_planner = value.to_owned(),
        "cache.ttl_missing" => config.cache.ttl_missing = value.to_owned(),
        "cache.ttl_grades" => config.cache.ttl_grades = value.to_owned(),
        "cache.ttl_modules" => config.cache.ttl_modules = value.to_owned(),
        "cache.ttl_files" => config.cache.ttl_files = value.to_owned(),
        "cache.ttl_announcements" => config.cache.ttl_announcements = value.to_owned(),
        "cache.ttl_calendar" => config.cache.ttl_calendar = value.to_owned(),
        "network.api_concurrency" => config.network.api_concurrency = parse_u32(value)?,
        "network.storage_concurrency" => {
            config.network.storage_concurrency = parse_u32(value)?;
        }
        "output.color" => {
            if !matches!(value, "auto" | "always" | "never") {
                return Err(CliError::usage(
                    "output.color must be auto, always, or never",
                ));
            }
            config.output.color = value.to_owned();
        }
        "bridge.pause_hidden_after" => {
            if pause_hidden_after_ms(value) == DEFAULT_PAUSE_HIDDEN_MS && value != "10m" {
                return Err(CliError::usage(
                    "bridge.pause_hidden_after must be a duration such as 10m or 1h",
                ));
            }
            config.bridge.pause_hidden_after = value.to_owned();
        }
        "bridge.extension_id" => {
            if value.is_empty() {
                config.bridge.extension_id = None;
            } else if canvas_core::bridge::state::is_extension_id(value) {
                config.bridge.extension_id = Some(value.to_owned());
            } else {
                return Err(CliError::usage(
                    "bridge.extension_id is 32 characters from a to p",
                ));
            }
        }
        other if other.starts_with("profiles.") => set_profile_field(config, other, value)?,
        other => return Err(CliError::usage(format!("unknown config key `{other}`"))),
    }
    Ok(())
}

fn set_profile_field(config: &mut Config, key: &str, value: &str) -> Result<(), CliError> {
    let rest = key.strip_prefix("profiles.").unwrap();
    let (name, field) = rest
        .split_once('.')
        .ok_or_else(|| CliError::usage(format!("unknown config key `{key}`")))?;
    let profile = config.profiles.entry(name.to_owned()).or_insert(Profile {
        origin: String::new(),
        user_id: 0,
        key: String::new(),
        name: None,
        time_zone: None,
    });
    match field {
        "origin" => profile.origin = value.to_owned(),
        "user_id" => {
            profile.user_id = value
                .parse()
                .map_err(|_| CliError::usage("user_id must be an integer"))?;
        }
        "key" => profile.key = value.to_owned(),
        "name" => {
            profile.name = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
        }
        "time_zone" => {
            profile.time_zone = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
        }
        _ => return Err(CliError::usage(format!("unknown config key `{key}`"))),
    }
    Ok(())
}

fn parse_u32(value: &str) -> Result<u32, CliError> {
    value
        .parse()
        .map_err(|_| CliError::usage(format!("expected an integer, got `{value}`")))
}

/// Open the config file in `$EDITOR`, or refuse without a TTY.
pub fn edit_config(paths: &CliPaths) -> Result<(), CliError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError::usage(
            "config edit requires a terminal; refuse without a TTY",
        ));
    }
    fs::create_dir_all(&paths.config_dir).map_err(|e| CliError::local(e.to_string()))?;
    let file = paths.config_file();
    if !file.exists() {
        Config::default().save(paths)?;
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let status = Command::new(&editor)
        .arg(&file)
        .status()
        .map_err(|e| CliError::local(format!("failed to launch EDITOR `{editor}`: {e}")))?;
    if !status.success() {
        return Err(CliError::local(format!(
            "editor `{editor}` exited with {status}"
        )));
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| CliError::local(e.to_string()))?;
    let tmp = parent.join(format!(".config.{}.tmp", uuid::Uuid::new_v4()));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| CliError::local(e.to_string()))?;
        f.write_all(bytes)
            .map_err(|e| CliError::local(e.to_string()))?;
        f.sync_all().map_err(|e| CliError::local(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| CliError::local(e.to_string()))?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| CliError::local(e.to_string()))?;
    Ok(())
}

/// Ensure a profile name is usable when creating via `auth login`.
#[must_use]
pub fn profile_name_or_default(flag: Option<&str>) -> String {
    flag.unwrap_or("default").to_owned()
}

/// Path helper for tests and commands.
#[must_use]
pub fn config_path_value(paths: &CliPaths) -> PathBuf {
    paths.config_file()
}

/// Serialize read-modify-replace across login, config set, and removal.
pub struct ConfigLock {
    _file: fs::File,
}
impl ConfigLock {
    pub fn acquire(paths: &CliPaths) -> Result<Self, CliError> {
        fs::create_dir_all(&paths.config_dir).map_err(|e| CliError::local(e.to_string()))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(paths.config_dir.join("config.lock"))
            .map_err(|e| CliError::local(e.to_string()))?;
        FileExt::lock_exclusive(&file).map_err(|e| CliError::local(e.to_string()))?;
        Ok(Self { _file: file })
    }
}

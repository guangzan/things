use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const GROUP_CONTAINER: &str = "JLMPQHK86H.com.culturedcode.ThingsMac";
pub const ENV_DATABASE: &str = "THINGSDB";
pub const ENV_AUTH_TOKEN: &str = "THINGS_AUTH_TOKEN";
pub const ENV_READ_ONLY: &str = "THINGS_READ_ONLY";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub database: Option<PathBuf>,
    pub auth_token: Option<String>,
    pub read_only: bool,
    pub output: OutputConfig,
    pub write: WriteConfig,
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputConfig {
    pub format: String,
    pub color: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: "text".to_string(),
            color: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WriteConfig {
    pub verify_timeout_ms: u64,
    pub verify_poll_ms: u64,
    pub create_missing_tags: bool,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            verify_timeout_ms: 8_000,
            verify_poll_ms: 250,
            create_missing_tags: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    pub read_only: bool,
    pub max_page_size: usize,
    pub default_page_size: usize,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            read_only: false,
            max_page_size: 100,
            default_page_size: 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseSelection {
    pub path: PathBuf,
    pub source: DatabaseSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseSource {
    Explicit,
    Environment,
    Config,
    Discovered,
}

impl DatabaseSelection {
    #[must_use]
    pub const fn source_name(&self) -> &'static str {
        match self.source {
            DatabaseSource::Explicit => "flag",
            DatabaseSource::Environment => ENV_DATABASE,
            DatabaseSource::Config => "config",
            DatabaseSource::Discovered => "discovered",
        }
    }
}

impl Config {
    /// Load `~/.config/things/config.toml`, then environment variables.
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        let mut config = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::Config { path, source })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => return Err(Error::Io(err)),
        };

        if let Ok(path) = env::var(ENV_DATABASE)
            && !path.trim().is_empty()
        {
            config.database = Some(PathBuf::from(path));
        }
        if let Ok(token) = env::var(ENV_AUTH_TOKEN)
            && !token.trim().is_empty()
        {
            config.auth_token = Some(token);
        }
        if let Ok(value) = env::var(ENV_READ_ONLY) {
            config.read_only = matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        Ok(config)
    }

    #[must_use]
    pub fn config_path() -> PathBuf {
        BaseDirs::new().map_or_else(
            || PathBuf::from(".config/things/config.toml"),
            |dirs| dirs.home_dir().join(".config/things/config.toml"),
        )
    }

    /// Resolve a database path without opening it. Explicit `--db` wins.
    pub fn select_database(&self, explicit: Option<&Path>) -> Result<DatabaseSelection> {
        if let Some(path) = explicit {
            return Ok(DatabaseSelection {
                path: path.to_path_buf(),
                source: DatabaseSource::Explicit,
            });
        }
        if let Some(path) = &self.database {
            let source = if env::var(ENV_DATABASE).is_ok() {
                DatabaseSource::Environment
            } else {
                DatabaseSource::Config
            };
            return Ok(DatabaseSelection {
                path: path.clone(),
                source,
            });
        }
        discover_database().map(|path| DatabaseSelection {
            path,
            source: DatabaseSource::Discovered,
        })
    }

    #[must_use]
    pub fn token(&self) -> Option<&str> {
        self.auth_token.as_deref().filter(|value| !value.is_empty())
    }
}

/// Locate the live database, refusing to guess when multiple data sets exist.
pub fn discover_database() -> Result<PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| Error::Internal("could not determine the home directory".to_string()))?;
    let container = base
        .home_dir()
        .join("Library/Group Containers")
        .join(GROUP_CONTAINER);
    let entries = fs::read_dir(&container).map_err(|err| {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            Error::Permission(format!("{}: {err}", container.display()))
        } else {
            Error::DatabaseNotFound
        }
    })?;

    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("ThingsData-") {
            continue;
        }
        let path = entry
            .path()
            .join("Things Database.thingsdatabase")
            .join("main.sqlite");
        if path.is_file() {
            matches.push(path);
        }
    }
    matches.sort();

    match matches.len() {
        0 => Err(Error::DatabaseNotFound),
        1 => Ok(matches.remove(0)),
        _ => Err(Error::MultipleDatabases(matches)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_ai_safe() {
        let config = Config::default();
        assert!(!config.read_only);
        assert!(!config.write.create_missing_tags);
        assert_eq!(config.mcp.max_page_size, 100);
    }

    #[test]
    fn unknown_config_key_is_rejected() {
        let result: std::result::Result<Config, _> = toml::from_str("unknown = true");
        assert!(result.is_err());
    }

    #[test]
    fn config_path_is_xdg_style_on_macos() {
        let path = Config::config_path();
        assert!(path.ends_with(".config/things/config.toml"));
        assert!(!path.to_string_lossy().contains("Application Support"));
    }
}

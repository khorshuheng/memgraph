use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use config::{Config, ConfigError, Environment, File, FileFormat};
use serde::Deserialize;

/// Default configuration written to the user's home directory on first run and
/// used as the base layer that the on-disk config overrides.
const DEFAULT_CONFIG_TOML: &str = r#"[server]
host = "127.0.0.1"
port = 7373

[database]
path = "memgraph.sqlite"
max_connections = 5

[search]
confidence_threshold = 0.70
min_matched_idf = 2.0
min_matched_terms = 2
max_terms = 6
prefix_min_length = 4
per_segment_limit = 5
summary_chars = 160
split_points = [".", "!", "?", ";", "\n"]
"#;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    pub fn listener_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SearchConfig {
    pub confidence_threshold: f64,
    pub min_matched_idf: f64,
    pub min_matched_terms: usize,
    pub max_terms: usize,
    pub prefix_min_length: usize,
    pub per_segment_limit: usize,
    pub summary_chars: usize,
    pub split_points: Vec<String>,
}

impl SearchConfig {
    pub fn split_characters(&self) -> Vec<char> {
        self.split_points
            .iter()
            .flat_map(|point| point.chars())
            .collect()
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.70,
            min_matched_idf: 2.0,
            min_matched_terms: 2,
            max_terms: 6,
            prefix_min_length: 4,
            per_segment_limit: 5,
            summary_chars: 160,
            split_points: vec![
                ".".to_string(),
                "!".to_string(),
                "?".to_string(),
                ";".to_string(),
                "\n".to_string(),
            ],
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub search: SearchConfig,
}

impl AppConfig {
    /// Loads configuration, seeding `$XDG_CONFIG_HOME/memgraph/config.toml`
    /// (falling back to `$HOME/.config`) with defaults when it is missing.
    pub fn new() -> Result<Self, ConfigError> {
        let path = default_config_path()?;
        ensure_config_file(&path)?;
        Self::load(&path)
    }

    fn load(path: &Path) -> Result<Self, ConfigError> {
        Self::load_from(path, true)
    }

    fn load_from(path: &Path, with_environment: bool) -> Result<Self, ConfigError> {
        let builder = Config::builder()
            .add_source(File::from_str(DEFAULT_CONFIG_TOML, FileFormat::Toml))
            // The file is guaranteed to exist by `ensure_config_file`; keeping it
            // required surfaces read errors instead of silently using defaults.
            .add_source(File::from(path));
        let builder = if with_environment {
            builder.add_source(Environment::with_prefix("MEMGRAPH").separator("__"))
        } else {
            builder
        };
        builder.build()?.try_deserialize()
    }
}

/// Resolves the config location following the XDG base directory spec.
fn default_config_path() -> Result<PathBuf, ConfigError> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| {
            ConfigError::Message(
                "unable to locate home directory: set XDG_CONFIG_HOME or HOME".to_string(),
            )
        })?;
    Ok(config_home.join("memgraph").join("config.toml"))
}

/// Creates the config file with defaults if it does not already exist.
fn ensure_config_file(path: &Path) -> Result<(), ConfigError> {
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(foreign)?;
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(DEFAULT_CONFIG_TOML.as_bytes())
                .map_err(foreign)?;
            tracing::info!(path = %path.display(), "created default configuration");
            Ok(())
        }
        // Another process created it first; keep whatever is already there.
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(foreign(error)),
    }
}

fn foreign(error: impl std::error::Error + Send + Sync + 'static) -> ConfigError {
    ConfigError::Foreign(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_config_deserializes() {
        let config = Config::builder()
            .add_source(File::from_str(DEFAULT_CONFIG_TOML, FileFormat::Toml))
            .build()
            .expect("default config builds")
            .try_deserialize::<AppConfig>()
            .expect("default config deserializes");
        assert_eq!(config.server.listener_address(), "127.0.0.1:7373");
        assert_eq!(config.database.max_connections, 5);
        // Guard against the embedded template drifting from the built-in default.
        assert_eq!(config.search, SearchConfig::default());
    }

    #[test]
    fn home_config_overrides_embedded_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "memgraph-config-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).expect("create temp config dir");
        let path = dir.join("config.toml");
        fs::write(&path, "[server]\nport = 9000\n").expect("write partial config");

        // Load without the environment layer so ambient `MEMGRAPH__*` variables
        // cannot make this test flaky.
        let config = AppConfig::load_from(&path, false).expect("partial config loads");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.host, "127.0.0.1");

        fs::remove_dir_all(&dir).ok();
    }
}

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize)]
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
    pub fn new() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::with_name("config/default.toml"))
            .add_source(File::with_name("config/override.toml").required(false))
            .add_source(Environment::with_prefix("MEMGRAPH").separator("__"))
            .build()?;
        config.try_deserialize()
    }
}

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub rcon_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub server_cfg: String,
    pub baseq2: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub paths: PathsConfig,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::load("config.defaults.yaml")
            .unwrap_or_else(|_| panic!("Failed to load default config"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_valid_config() {
        let yaml = r#"
server:
  host: test.local
  port: 27910
  rcon_password: test123
paths:
  server_cfg: /tmp/server.cfg
  baseq2: /tmp/baseq2
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.server.host, "test.local");
        assert_eq!(config.server.port, 27910);
        assert_eq!(config.paths.baseq2, "/tmp/baseq2");
    }

    #[test]
    fn test_load_missing_file() {
        let result = Config::load("nonexistent.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_yaml() {
        let yaml = "invalid: yaml: :";
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }
}

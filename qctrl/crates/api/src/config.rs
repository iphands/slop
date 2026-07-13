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

/// How often the background poller touches the server.
///
/// The two intervals are deliberately very different because they hit two
/// different rate limits. The OOB status query is bounded by `sv_status_limit`
/// (default 15/sec), so 1 Hz is 15x under budget. RCON is bounded by
/// `sv_rcon_limit` (default 1/sec), and exceeding it makes the server reply
/// `Bad rcon_password` to everything — so RCON is polled rarely, and only for
/// the client numbers and addresses the OOB reply does not carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PollConfig {
    /// OOB status poll interval. Also the accuracy bound on the map clock: a map
    /// change is noticed within one of these, and that is where the clock anchors.
    pub status_interval_ms: u64,
    /// How stale the RCON-sourced client-number/address table may get. A newly
    /// connected player refreshes it early regardless, so this is just the
    /// backstop.
    pub rcon_identity_interval_ms: u64,
    /// Keep `sv_uptime 1` set on the server. It is what lets qctrl notice a
    /// server restart onto the same map — without it the map clock would keep
    /// counting from a dead anchor and be silently wrong. See `crate::clock`.
    pub manage_sv_uptime: bool,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            status_interval_ms: 1000,
            rcon_identity_interval_ms: 30_000,
            manage_sv_uptime: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub paths: PathsConfig,
    /// Absent from existing config.yaml files, so it must default.
    #[serde(default)]
    pub poll: PollConfig,
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

    /// Every deployed config.yaml predates the `poll:` block. Loading must not
    /// start failing on them.
    #[test]
    fn a_config_without_a_poll_block_still_loads() {
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
        assert_eq!(config.poll.status_interval_ms, 1000);
        assert_eq!(config.poll.rcon_identity_interval_ms, 30_000);
        assert!(config.poll.manage_sv_uptime);
    }

    #[test]
    fn poll_settings_are_overridable() {
        let yaml = r#"
server:
  host: test.local
  port: 27910
  rcon_password: test123
paths:
  server_cfg: /tmp/server.cfg
  baseq2: /tmp/baseq2
poll:
  status_interval_ms: 5000
  manage_sv_uptime: false
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.poll.status_interval_ms, 5000);
        assert!(!config.poll.manage_sv_uptime);
        // Unspecified keys keep their defaults.
        assert_eq!(config.poll.rcon_identity_interval_ms, 30_000);
    }
}

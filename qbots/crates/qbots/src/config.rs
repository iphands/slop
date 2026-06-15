//! Load `config.yaml` — server address + on-disk Q2 paths (for BSP loading in Plan 05).

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: Server,
    pub paths: Paths,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct Paths {
    pub server_cfg: PathBuf,
    pub baseq2: PathBuf,
}

impl Config {
    /// Read and parse a YAML config file.
    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        serde_yaml::from_str(&text).map_err(|e| format!("parse {path}: {e}"))
    }

    /// `host:port` for connecting.
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// Path to `<baseq2>/maps/<name>.bsp`.
    pub fn map_bsp(&self, map_name: &str) -> PathBuf {
        self.paths
            .baseq2
            .join("maps")
            .join(format!("{map_name}.bsp"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_config() {
        let yaml = "\
server:
  host: noir.lan
  port: 27910
paths:
  server_cfg: /srv/q2/baseq2/server.cfg
  baseq2: /srv/q2/baseq2
";
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.server_addr(), "noir.lan:27910");
        assert_eq!(cfg.paths.baseq2, PathBuf::from("/srv/q2/baseq2"));
        assert_eq!(
            cfg.map_bsp("q2dm1"),
            PathBuf::from("/srv/q2/baseq2/maps/q2dm1.bsp")
        );
    }
}

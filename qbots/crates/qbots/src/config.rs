//! Load `config.yaml` — server address, on-disk Q2 paths, and the bot fleet roster.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: Server,
    pub paths: Paths,
    /// Fleet roster. When present, `qbots run` launches this many bots.
    #[serde(default)]
    pub fleet: Fleet,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Server {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Paths {
    pub server_cfg: PathBuf,
    pub baseq2: PathBuf,
}

/// Fleet roster — describes N bots spawned by `qbots run` (Plan 09).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Fleet {
    /// Number of bots to spawn.
    pub count: usize,
    /// Bots are named `<name_prefix><index>` (e.g. `qb0`, `qb1`).
    pub name_prefix: String,
    /// Base qport; bot *i* uses `qport_base + i`. Must be distinct per bot.
    pub qport_base: u16,
    /// Delay between successive connect starts (ms), to avoid a connectionless burst.
    pub connect_stagger_ms: u64,
    /// Restart a bot that disconnects (with backoff).
    pub reconnect: bool,
    /// Max reconnect attempts before giving up (0 = unlimited).
    pub max_reconnects: u32,
    /// Hard cap on bots spawned, regardless of `count`. Guards against exceeding
    /// the server's `maxclients` (leave headroom for humans). 0 = no cap.
    pub max_bots: usize,
    /// Max time (ms) a bot may spend reaching `Active` before its join is treated as
    /// failed. Backstops silent drops the reject-parse can't classify; a fleet-fatal
    /// join failure aborts the run unless `--loose-botcap` is set. Default 10_000.
    pub connect_timeout_ms: u64,
    /// Brain (decision plugin) for the fleet: `main` (default), `sentry`, `runtester`, or `q3`.
    /// `None`/absent → `main`. The CLI `--brain` overrides this. Independent of the nav backend
    /// (`--navmode`).
    pub brain: Option<String>,
    /// Q3 personality for the fleet when `brain = "q3"`: `grunt`/`major`/`sarge`/`camper`.
    /// `None`/absent → the skill-derived default character. CLI `--char` overrides this.
    pub char: Option<String>,
}

impl Default for Fleet {
    fn default() -> Self {
        Self {
            count: 0,
            name_prefix: "qb".to_string(),
            qport_base: 28000,
            connect_stagger_ms: 250,
            reconnect: true,
            max_reconnects: 0,
            max_bots: 0,
            connect_timeout_ms: 10_000,
            brain: None,
            char: None,
        }
    }
}

impl Fleet {
    /// The display name for bot `i`.
    pub fn bot_name(&self, i: usize) -> String {
        format!("{}{}", self.name_prefix, i)
    }

    /// Parse the configured `brain` string into a `BrainKind`. `None` (absent) or an
    /// unrecognized value falls back to `main` (logged), so an old/garbled config still runs.
    pub fn brain_kind(&self) -> brain::BrainKind {
        use clap::ValueEnum;
        match self.brain.as_deref() {
            None => brain::BrainKind::Main,
            Some(s) => brain::BrainKind::from_str(s, true).unwrap_or_else(|_| {
                tracing::warn!(brain = s, "unknown [fleet].brain; falling back to 'main'");
                brain::BrainKind::Main
            }),
        }
    }

    /// Parse the configured `char` string into a `CharPreset`. `None` (absent) → `None`
    /// (skill-derived default); an unrecognized value falls back to `None` (logged).
    pub fn char_preset(&self) -> Option<brain::CharPreset> {
        use clap::ValueEnum;
        match self.char.as_deref() {
            None => None,
            Some(s) => brain::CharPreset::from_str(s, true)
                .map(Some)
                .unwrap_or_else(|_| {
                    tracing::warn!(char = s, "unknown [fleet].char; ignoring");
                    None
                }),
        }
    }

    /// Is the fleet enabled (any bots to spawn)?
    pub fn enabled(&self) -> bool {
        self.count > 0
    }
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
        // Fleet defaults when absent.
        assert!(!cfg.fleet.enabled());
    }

    #[test]
    fn parses_fleet_roster() {
        let yaml = "\
server: { host: noir.lan, port: 27910 }
paths: { server_cfg: /x, baseq2: /y }
fleet:
  count: 6
  name_prefix: bot
  qport_base: 28000
  connect_stagger_ms: 300
  reconnect: true
  max_reconnects: 5
";
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.fleet.enabled());
        assert_eq!(cfg.fleet.bot_name(0), "bot0");
        assert_eq!(cfg.fleet.bot_name(5), "bot5");
        assert_eq!(cfg.fleet.qport_base, 28000);
        assert_eq!(cfg.fleet.connect_stagger_ms, 300);
    }
}

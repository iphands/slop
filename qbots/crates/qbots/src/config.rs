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
    /// Optional serverframe beacon for qctrl (Plan 66). Absent → disabled.
    #[serde(default)]
    pub beacon: BeaconCfg,
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

/// Optional serverframe beacon (Plan 66) — publishes the fleet's view of `sv.framenum`
/// on a unix socket so qctrl can know the exact age of the running map without
/// connecting a Q2 client of its own.
///
/// **Off by default.** An existing `config.yaml` with no `beacon:` block behaves exactly
/// as it did before: no socket is created and no task is spawned.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct BeaconCfg {
    pub enabled: bool,
    /// Unix socket qbots listens on. qctrl connects to it. Note: if the two ever run as
    /// systemd units with `PrivateTmp=`, `/tmp` is *not* shared — move this to `/run/qbots/`.
    pub socket_path: PathBuf,
    /// Heartbeat interval. A level change publishes immediately regardless of this, and
    /// each line carries `age_ms`, so a slow interval costs no accuracy — only resolution.
    pub publish_interval_ms: u64,
    /// Mode applied to the socket file. The beacon carries no secrets (unlike qctrl's
    /// config, which holds the rcon password), but it is still world-readable telemetry.
    pub socket_mode: u32,
    /// Refuse more than this many concurrent readers.
    pub max_clients: usize,
}

impl Default for BeaconCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            socket_path: PathBuf::from("/tmp/qbots-beacon.sock"),
            publish_interval_ms: 1000,
            socket_mode: 0o666,
            max_clients: 4,
        }
    }
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
    /// Max time (ms) an `Active` bot may go without a single new server frame before
    /// treating its slot as dead and re-handshaking (Plan 65). The server streams
    /// `svc_frame` at 10 Hz, intermission included, so this only trips when the slot is
    /// gone — e.g. a hard map change whose unreliable `svc_reconnect` copies were all
    /// lost, which otherwise leaves the bot Active forever feeding `clc_move` into a
    /// recycled slot the server ignores. Default 10_000.
    pub stall_timeout_ms: u64,
    /// Brain (decision plugin) for the fleet: `main` (default), `sentry`, `runtester`, or `q3`.
    /// `None`/absent → `main`. The CLI `--brain` overrides this. Independent of the nav backend
    /// (`--navmode`).
    pub brain: Option<String>,
    /// Q3 personality for the fleet when `brain = "q3"`: `grunt`/`major`/`sarge`/`camper`.
    /// `None`/absent → the skill-derived default character. CLI `--char` overrides this.
    pub char: Option<String>,
    /// Xonotic personality for `xon`-brain fleet bots (`rus`/`shp`/`trt`/`nob` or long names;
    /// Plan 62). `None`/absent → a neutral XonSkill at the master skill level.
    pub xonchar: Option<String>,
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
            stall_timeout_ms: 10_000,
            brain: None,
            char: None,
            xonchar: None,
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

    /// Parse the configured `xonchar` string into an `XonCharPreset` (same contract as
    /// [`Self::char_preset`]).
    pub fn xonchar_preset(&self) -> Option<brain::XonCharPreset> {
        use clap::ValueEnum;
        match self.xonchar.as_deref() {
            None => None,
            Some(s) => brain::XonCharPreset::from_str(s, true)
                .map(Some)
                .unwrap_or_else(|_| {
                    tracing::warn!(xonchar = s, "unknown [fleet].xonchar; ignoring");
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

    /// Every config in the wild predates Plan 66 and has no `beacon:` block. Those configs
    /// must keep behaving exactly as they did — which means the beacon stays OFF.
    #[test]
    fn a_config_without_a_beacon_block_leaves_it_disabled() {
        let yaml = "\
server: { host: noir.lan, port: 27910 }
paths: { server_cfg: /x, baseq2: /y }
";
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.beacon.enabled);
        assert_eq!(
            cfg.beacon.socket_path,
            PathBuf::from("/tmp/qbots-beacon.sock")
        );
        assert_eq!(cfg.beacon.publish_interval_ms, 1000);
    }

    #[test]
    fn beacon_settings_are_overridable_and_unspecified_keys_keep_defaults() {
        let yaml = "\
server: { host: noir.lan, port: 27910 }
paths: { server_cfg: /x, baseq2: /y }
beacon:
  enabled: true
  socket_path: /run/qbots/beacon.sock
";
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.beacon.enabled);
        assert_eq!(
            cfg.beacon.socket_path,
            PathBuf::from("/run/qbots/beacon.sock")
        );
        // Untouched keys keep their defaults.
        assert_eq!(cfg.beacon.publish_interval_ms, 1000);
        assert_eq!(cfg.beacon.max_clients, 4);
    }
}

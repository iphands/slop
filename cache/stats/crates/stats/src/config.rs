//! Environment configuration.
//!
//! Env vars with `${VAR:-default}` semantics, matching the discipline the shell
//! scripts already use. No config file: every knob is a container `ENV`, so
//! `scripts/noir/create-stats.sh` is the single place a deployment is described.

use std::path::PathBuf;

/// Runtime knobs, all overridable by environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// nginx's TSV logs. Mounted **rw**: this service prunes consumed files,
    /// because it is the only process that knows which are fully ingested.
    pub logs_dir: PathBuf,
    /// Our own scratch: `stats.sqlite`, `.ingest.lock`, `labels.json`.
    pub data_dir: PathBuf,
    /// The nginx package cache, mounted **ro**, used only for a size walk.
    /// `None` disables the cache-fullness tile.
    pub cache_dir: Option<PathBuf>,
    pub bind: String,
    pub tick_seconds: u64,
    pub log_retention_days: i64,
    pub db_retention_days: i64,
    /// SQLite WAL. Must be false on NFS/CIFS, where WAL does not work.
    pub wal: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_num<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Config {
    pub fn from_env() -> Self {
        let cache = env_or("PKGCACHE_CACHE", "/cache");
        Self {
            logs_dir: env_or("PKGCACHE_LOGS", "/logs").into(),
            data_dir: env_or("PKGCACHE_DATA", "/data").into(),
            // An empty value disables it explicitly, rather than silently
            // reporting a bogus zero-byte cache.
            cache_dir: if cache.trim().is_empty() {
                None
            } else {
                Some(cache.into())
            },
            bind: env_or("PKGCACHE_BIND", "0.0.0.0:8081"),
            tick_seconds: env_num("PKGCACHE_TICK_SECONDS", 5).max(1),
            log_retention_days: env_num("PKGCACHE_LOG_RETENTION_DAYS", 3).max(1),
            db_retention_days: env_num("PKGCACHE_DB_RETENTION_DAYS", 30).max(1),
            wal: !matches!(
                env_or("PKGCACHE_WAL", "1").as_str(),
                "0" | "false" | "no" | "off"
            ),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("stats.sqlite")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.data_dir.join(".ingest.lock")
    }

    /// Hand-edited `{"ip": "label"}`, hot-reloaded each tick. Deliberately not
    /// reverse DNS: this service makes zero outbound network calls.
    pub fn labels_path(&self) -> PathBuf {
        self.data_dir.join("labels.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env is process-global, so the env-mutating assertions live in one test
    /// rather than racing each other across threads.
    #[test]
    fn defaults_and_overrides_resolve() {
        // Defaults match the container ENV block and scripts/noir/create-stats.sh.
        std::env::remove_var("PKGCACHE_LOGS");
        std::env::remove_var("PKGCACHE_CACHE");
        std::env::remove_var("PKGCACHE_TICK_SECONDS");
        std::env::remove_var("PKGCACHE_WAL");
        let c = Config::from_env();
        assert_eq!(c.logs_dir, PathBuf::from("/logs"));
        assert_eq!(c.data_dir, PathBuf::from("/data"));
        assert_eq!(c.cache_dir, Some(PathBuf::from("/cache")));
        assert_eq!(c.tick_seconds, 5);
        assert!(c.wal);
        assert_eq!(c.db_path(), PathBuf::from("/data/stats.sqlite"));
        assert_eq!(c.lock_path(), PathBuf::from("/data/.ingest.lock"));

        std::env::set_var("PKGCACHE_LOGS", "/tmp/l");
        std::env::set_var("PKGCACHE_TICK_SECONDS", "17");
        std::env::set_var("PKGCACHE_WAL", "0");
        let c = Config::from_env();
        assert_eq!(c.logs_dir, PathBuf::from("/tmp/l"));
        assert_eq!(c.tick_seconds, 17);
        assert!(!c.wal, "WAL must be disablable for NFS/CIFS");

        // An empty cache dir disables the tile rather than reporting a bogus 0.
        std::env::set_var("PKGCACHE_CACHE", "");
        assert_eq!(Config::from_env().cache_dir, None);

        // A garbage number falls back to the default instead of panicking.
        std::env::set_var("PKGCACHE_TICK_SECONDS", "not-a-number");
        assert_eq!(Config::from_env().tick_seconds, 5);

        // Zero would be a busy-loop; clamped.
        std::env::set_var("PKGCACHE_TICK_SECONDS", "0");
        assert_eq!(Config::from_env().tick_seconds, 1);

        for k in [
            "PKGCACHE_LOGS",
            "PKGCACHE_CACHE",
            "PKGCACHE_TICK_SECONDS",
            "PKGCACHE_WAL",
        ] {
            std::env::remove_var(k);
        }
    }
}

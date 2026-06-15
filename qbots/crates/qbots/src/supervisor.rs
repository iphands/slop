//! Fleet supervisor — spawns N bot tasks sharing one nav graph, with staggered
//! connects, reconnect-on-disconnect, and graceful shutdown (Plan 09).
//!
//! Each bot is fully independent (AGENTS.md §Concurrency): its own socket,
//! connection FSM, and brain. The only shared state is the read-only nav graph,
//! built **once per map** via [`NavCache`] and handed out as an `Arc`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time;

use crate::config::Config;

/// A cached, shared nav graph + roam nodes for a map. Built once per map name;
/// cloned cheaply as `Arc` to every bot.
#[derive(Clone)]
pub struct MapNav {
    pub graph: Arc<world::NavGraph>,
    pub roam_nodes: Vec<usize>,
}

/// Process-wide cache of nav graphs keyed by map name. The first bot to discover
/// a map builds its graph; the rest reuse the `Arc`. Build happens under a lock
/// so concurrent discoverers don't duplicate work.
#[derive(Clone, Default)]
pub struct NavCache {
    maps: Arc<Mutex<HashMap<String, Arc<MapNav>>>>,
}

impl NavCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached nav graph for `map`, building it from `cfg` if absent.
    /// On a build error, logs and returns `None` (the bot runs without nav).
    pub fn get_or_build(&self, cfg: &Config, map: &str) -> Option<Arc<MapNav>> {
        // Fast path: already cached.
        {
            let maps = self.maps.lock().unwrap();
            if let Some(existing) = maps.get(map) {
                return Some(Arc::clone(existing));
            }
        }
        // Slow path: build under the lock (once per map; ~tens of ms).
        let built = build_map_nav(cfg, map)?;
        let arc = Arc::new(built);
        self.maps
            .lock()
            .unwrap()
            .insert(map.to_string(), Arc::clone(&arc));
        Some(arc)
    }
}

/// Build the nav graph + roam nodes for `map` from the BSP on disk.
fn build_map_nav(cfg: &Config, map: &str) -> Option<MapNav> {
    let bsp = match world::Bsp::load(&cfg.paths.baseq2, map) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(map, "nav load failed: {e}  (no nav)");
            return None;
        }
    };
    let cm = world::CollisionModel::from_bsp(&bsp);
    let m = bsp.models.first().expect("bsp has models");
    let t0 = std::time::Instant::now();
    let g = world::NavGraph::generate(&cm, (m.mins, m.maxs), 64.0);
    let largest = g.components().into_iter().next().unwrap_or_default();
    tracing::info!(
        map,
        nodes = g.node_count(),
        edges = g.edge_count(),
        largest = largest.len(),
        ms = t0.elapsed().as_millis() as u64,
        "nav graph ready"
    );
    Some(MapNav {
        graph: Arc::new(g),
        roam_nodes: largest,
    })
}

/// Shared shutdown signal. Set by the signal listener; bots poll it each tick.
#[derive(Clone, Default)]
pub struct Shutdown {
    flag: Arc<AtomicBool>,
}

impl Shutdown {
    pub fn new() -> Self {
        Self::default()
    }

    /// Request all bots to disconnect and exit.
    pub fn fire(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn requested(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Sleep for `dur`, returning early if shutdown is requested.
    pub async fn sleep_or_cancel(&self, dur: Duration) {
        let _ = tokio::time::timeout(dur, async {
            while !self.requested() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
    }
}

/// Spawn the process-wide signal listener (SIGINT/SIGTERM) that fires `shutdown`.
/// Returns its handle; it runs until the process exits.
pub fn spawn_signal_listener(shutdown: Shutdown) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("install SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down fleet…"),
            _ = sigint.recv() => tracing::info!("received SIGINT, shutting down fleet…"),
        }
        shutdown.fire();
    })
}

/// Reconnect policy for a bot in the fleet.
#[derive(Clone, Copy)]
struct Reconnect {
    enabled: bool,
    max_attempts: u32,
}

/// Run the full fleet from config: shared nav cache + shutdown, one task per bot,
/// staggered connects, reconnect-on-disconnect with backoff. Returns when all
/// bot tasks have exited (typically after shutdown is requested).
pub async fn run_fleet(cfg: Arc<Config>, addr: SocketAddr) -> std::io::Result<()> {
    // Apply the maxclients guard: never spawn more than `max_bots` (leave slots
    // for humans). 0 = uncapped.
    let mut count = cfg.fleet.count;
    if cfg.fleet.max_bots > 0 && count > cfg.fleet.max_bots {
        tracing::warn!(
            requested = count,
            cap = cfg.fleet.max_bots,
            "clamping fleet size to max_bots (server maxclients headroom)"
        );
        count = cfg.fleet.max_bots;
    }

    if count == 0 {
        tracing::error!("fleet.count is 0 — nothing to run (use `connect-one` for a single bot)");
        return Ok(());
    }
    let stagger = cfg.fleet.connect_stagger_ms;
    let reconnect = Reconnect {
        enabled: cfg.fleet.reconnect,
        max_attempts: cfg.fleet.max_reconnects,
    };

    let nav_cache = NavCache::new();
    let shutdown = Shutdown::new();
    let _signals = spawn_signal_listener(shutdown.clone());

    tracing::info!(count, "launching fleet to {addr}");

    let mut tasks = Vec::new();
    for i in 0..count {
        let name = cfg.fleet.bot_name(i);
        let qport = cfg.fleet.bot_qport(i);
        let cfg = Arc::clone(&cfg);
        let nav = nav_cache.clone();
        let sd = shutdown.clone();
        tasks.push(tokio::spawn(async move {
            bot_supervisor_loop(addr, name, qport, cfg, nav, sd, reconnect).await;
        }));
        // Stagger connects so we don't burst the server's connectionless handler.
        time::sleep(Duration::from_millis(stagger)).await;
    }

    // Periodic fleet heartbeat (liveness + count). Per-bot events carry the bot
    // name via the `bot` tracing span, so individual bots are filterable in logs.
    let sd = shutdown.clone();
    let status = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            tracing::info!(total = count, "fleet heartbeat");
            if sd.requested() {
                break;
            }
        }
    });

    for t in tasks {
        let _ = t.await;
    }
    status.abort();
    tracing::info!("fleet exited");
    Ok(())
}

/// Per-bot supervisor: run `bot_task`, and if it exits due to a disconnect
/// (not shutdown), reconnect with exponential backoff up to `max_reconnects`.
async fn bot_supervisor_loop(
    addr: SocketAddr,
    name: String,
    qport: u16,
    cfg: Arc<Config>,
    nav: NavCache,
    shutdown: Shutdown,
    reconnect: Reconnect,
) {
    let mut attempts: u32 = 0;
    let mut backoff_ms: u64 = 1000;
    loop {
        if shutdown.requested() {
            return;
        }
        match crate::bot_task(addr, &name, qport, &cfg, &nav, &shutdown).await {
            Ok(()) => {
                tracing::info!(%name, "bot task exited");
            }
            Err(e) => {
                tracing::warn!(%name, "bot task error: {e}");
            }
        }
        if !reconnect.enabled || shutdown.requested() {
            return;
        }
        attempts += 1;
        if reconnect.max_attempts > 0 && attempts > reconnect.max_attempts {
            tracing::warn!(%name, attempts, "giving up after max reconnects");
            return;
        }
        tracing::info!(%name, backoff_ms, "reconnecting");
        shutdown
            .sleep_or_cancel(Duration::from_millis(backoff_ms))
            .await;
        backoff_ms = (backoff_ms * 2).min(15_000);
    }
}

/// Run a single bot for `connect-one`. Builds a private nav cache + shutdown and
/// installs the signal listener, then runs one `bot_task` (no reconnect loop).
pub async fn run_single(
    cfg: &Config,
    addr: SocketAddr,
    name: &str,
    qport: u16,
) -> std::io::Result<()> {
    let nav = NavCache::new();
    let shutdown = Shutdown::new();
    let _signals = spawn_signal_listener(shutdown.clone());
    crate::bot_task(addr, name, qport, cfg, &nav, &shutdown).await
}

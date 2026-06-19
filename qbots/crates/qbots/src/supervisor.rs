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

pub use crate::stats::FleetStats;

/// A cached, shared nav graph + roam nodes for a map. Built once per map name;
/// cloned cheaply as `Arc` to every bot.
#[derive(Clone)]
pub struct MapNav {
    pub graph: Arc<world::NavGraph>,
    /// The collision model the graph was built from — retained so the tick can run
    /// line-of-sight traces (Plan 11) and reactive wall probes (Plan 13) without
    /// rebuilding it.
    pub cm: Arc<world::CollisionModel>,
    pub roam_nodes: Vec<usize>,
    /// World-model bounds (`models[0].mins/maxs`) — the navmesh backend builds its
    /// heightfield over this extent. Retained so a `--navmode navmesh` bot can build the
    /// mesh via [`get_or_build_navmesh`] without reparsing the BSP.
    pub bounds: ([f32; 3], [f32; 3]),
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

    /// Return the cached nav graph for `map`, loading it from `cfg` if absent.
    /// A load failure is fatal (`build_map_nav` exits the process), so a returned
    /// `None` only ever means an internal invariant slipped — never "run without nav".
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

const DEFAULT_CACHE_DIR: &str = "data/mapcache";

/// Build the nav graph + roam nodes for `map` from the BSP on disk.
/// Loads `./data/mapcache/<spacing>/<map>.qnav` (load-only — caches are generated
/// ahead of time with `qbots generate-map-cache`). A missing/stale cache or any load
/// failure is **fatal**: running bots with no nav data on the server's real map is a
/// silent-failure trap, so we abort the whole process rather than flail without nav.
fn build_map_nav(cfg: &Config, map: &str) -> Option<MapNav> {
    let t0 = std::time::Instant::now();
    let cache_dir = std::path::Path::new(DEFAULT_CACHE_DIR);
    let built = match world::cached_map_nav(
        &cfg.paths.baseq2,
        map,
        Some(cache_dir),
        world::ELEVATOR_PENALTY,
        world::GRID_SPACING,
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(map, "nav load failed: {e}");
            tracing::error!(map, "aborting: no usable nav data for the server's map");
            std::process::exit(1);
        }
    };
    // Hard abort: a broken nav graph means no bot on this map can navigate.
    // All Q2 dm maps guarantee full spawn reachability — failure is our bug.
    if let Err(diag) = world::check_spawn_connectivity(&built) {
        tracing::error!(map, "{diag}");
        tracing::error!(
            map,
            "aborting: nav connectivity bug — bots cannot navigate this map"
        );
        std::process::exit(1);
    }
    tracing::info!(
        map,
        nodes = built.graph.node_count(),
        edges = built.graph.edge_count(),
        largest = built.largest.len(),
        seeded = built.seeded,
        jump_edges = built.added_jumps,
        ms = t0.elapsed().as_millis() as u64,
        "nav graph ready"
    );
    let model = &built.bsp.models[0];
    let bounds = (model.mins, model.maxs);
    Some(MapNav {
        graph: Arc::new(built.graph),
        cm: built.cm,
        roam_nodes: built.largest,
        bounds,
    })
}

/// Process-global navmesh cache so the N bots of a `--navmode navmesh` run share one built
/// mesh instead of each rebuilding it (mirrors [`NavCache`]). Keyed by map name; the first
/// bot to ask builds it under the lock, the rest clone the `Arc`. Honors `QBOTS_ERODE`.
/// (A later phase will replace this with a disk cache like `cached_map_nav`.)
pub(crate) fn get_or_build_navmesh(
    map: &str,
    cm: &world::CollisionModel,
    bounds: ([f32; 3], [f32; 3]),
) -> Arc<world::NavMesh> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<world::NavMesh>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(m) = guard.get(map) {
        return Arc::clone(m);
    }
    // cell 8 (fine enough that agent-radius erosion keeps 32u-doorway centerlines) + erode by
    // agent_radius/cell = 16/8 = 2 cells, so bots aren't routed into near-wall hull-jam cells.
    let params = world::VoxelParams {
        cell_size: 8.0,
        ..Default::default()
    };
    let mut hf = world::Heightfield::build(cm, bounds, params);
    let drops = hf.find_drops(cm); // on the FULL heightfield, before erosion removes ledge edges
                                   // erode 1 cell (8u): de-jams near walls while keeping thin
                                   // (~32u) Q2 ledges (the RL route); the full radius erases them.
    let erode = std::env::var("QBOTS_ERODE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    hf.erode(erode);
    let mut mesh = world::NavMesh::build(&hf, params.walkable_climb, Some(cm));
    mesh.add_drops(&drops);
    tracing::info!(
        map,
        polys = mesh.polys.len(),
        "navmesh built (mode=navmesh)"
    );
    let arc = Arc::new(mesh);
    guard.insert(map.to_string(), Arc::clone(&arc));
    arc
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

/// Shared, clone-cheap fleet infrastructure handed to each bot's supervisor loop
/// (Plan 09): the nav cache, shutdown signal, and kill/death tally. Bundled so
/// the per-bot dispatcher stays under clippy's argument count.
#[derive(Clone)]
struct FleetShared {
    nav: NavCache,
    shutdown: Shutdown,
    stats: FleetStats,
}

/// Run the full fleet from config: shared nav cache + shutdown, one task per bot,
/// staggered connects, reconnect-on-disconnect with backoff. Returns when all
/// bot tasks have exited (typically after shutdown is requested). `mode` selects the
/// navigation backend (`--navmode`) for the whole fleet.
///
/// `count_override` (CLI `--count`) replaces `[fleet].count`; `name_override` (CLI
/// `--name`) replaces the roster's naming, yielding `<name>_1`, `<name>_2`, … (1-based).
/// `qport_base_override` (CLI `--qport-base`) pins the qport base; when `None` a
/// per-process value (`default_qport`) is used so two concurrent fleets on the same host
/// don't collide on the server's `(ip, qport)` client-slot key.
#[allow(clippy::too_many_arguments)]
pub async fn run_fleet(
    cfg: Arc<Config>,
    addr: SocketAddr,
    mode: crate::NavMode,
    brain: brain::BrainKind,
    name_override: Option<String>,
    count_override: Option<usize>,
    qport_base_override: Option<u16>,
    skin: crate::skins::SkinSelection,
    q3char: Option<brain::Q3CharPreset>,
) -> std::io::Result<()> {
    // Apply the maxclients guard: never spawn more than `max_bots` (leave slots
    // for humans). 0 = uncapped. `--count` overrides the config roster size first.
    let mut count = count_override.unwrap_or(cfg.fleet.count);
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
    let stats = FleetStats::new();
    let _signals = spawn_signal_listener(shutdown.clone());
    let shared = FleetShared {
        nav: nav_cache,
        shutdown: shutdown.clone(),
        stats: stats.clone(),
    };

    // Per-process default so concurrent `run` fleets get disjoint qport ranges (the
    // server keys client slots on base-IP + qport, ignoring UDP source port). `--qport-base`
    // pins it for reproducibility.
    let qport_base = qport_base_override.unwrap_or_else(crate::default_fleet_qport_base);
    tracing::info!(count, qport_base, "launching fleet to {addr}");

    // One RNG for the whole fleet so random skins vary bot-to-bot within this run.
    let mut skin_rng = crate::skins::Rng::new();
    let mut tasks = Vec::new();
    for i in 0..count {
        // `--name foo` → `foo_1, foo_2, …` (1-based); else the config roster name.
        let name = match name_override.as_deref() {
            Some(prefix) => format!("{prefix}_{}", i + 1),
            None => cfg.fleet.bot_name(i),
        };
        let qport = qport_base.wrapping_add(i as u16);
        // A selected Q3 character pins its recognizable skin; else draw once per bot (kept
        // across reconnects); `None` keeps the userinfo default.
        let bot_skin = q3char
            .map(|q| q.skin().to_string())
            .or_else(|| skin.per_bot(&mut skin_rng));
        let cfg = Arc::clone(&cfg);
        let shared = shared.clone();
        tasks.push(tokio::spawn(async move {
            bot_supervisor_loop(
                addr, name, qport, bot_skin, cfg, shared, reconnect, mode, brain, q3char,
            )
            .await;
        }));
        // Stagger connects so we don't burst the server's connectionless handler.
        time::sleep(Duration::from_millis(stagger)).await;
    }

    // Periodic fleet heartbeat (liveness + count + rolling kill/death tally).
    // Per-bot events carry the bot name via the `bot` tracing span, so individual
    // bots are filterable in logs.
    let sd = shutdown.clone();
    let hb_stats = stats.clone();
    let status = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            let totals = hb_stats.totals();
            tracing::info!(
                bots = count,
                kills = totals.kills,
                deaths = totals.deaths,
                "fleet heartbeat"
            );
            if sd.requested() {
                break;
            }
        }
    });

    for t in tasks {
        let _ = t.await;
    }
    status.abort();
    // All bots have now disconnected (each sends `disconnect` on shutdown before
    // exiting) — emit the final tally.
    log_final_stats(&stats);
    tracing::info!("fleet exited");
    Ok(())
}

/// Short, stable tag for a nav backend — used as the competitor name prefix (`<tag>_<i>`) so the
/// scoreboard can group bots by mode, and as the per-mode skin label.
pub(crate) fn mode_tag(mode: crate::NavMode) -> &'static str {
    match mode {
        crate::NavMode::Astar => "astar",
        crate::NavMode::Navmesh => "navmesh",
        crate::NavMode::HybridFallback => "fallback",
        crate::NavMode::HybridRace => "race",
        crate::NavMode::HybridHier => "hier",
        crate::NavMode::HybridSegment => "segment",
    }
}

/// Run a **competition**: spawn `per_group_count` bots for each `(mode, brain, q3char?)` group in
/// a single process, all sharing one `NavCache` (built once, not once per mode), each group wearing
/// a distinct skin. Bots are named `<mode>_<brain>[_<q3char>]_<i>` (e.g. `astar_main_1`, `race_q3_1`,
/// `astar_q3_grunt_1`) so the per-group frag scoreboard can group them. Returns when all bots exit
/// (after shutdown). Reuses the fleet's per-bot supervisor loop (reconnect/backoff/graceful
/// disconnect) with a **per-bot** `mode`/`brain`/`q3char`.
#[allow(clippy::too_many_arguments)]
pub async fn run_competition(
    cfg: Arc<Config>,
    addr: SocketAddr,
    modes: Vec<crate::NavMode>,
    brains: Vec<brain::BrainKind>,
    q3chars: Vec<brain::Q3CharPreset>,
    per_group_count: usize,
    qport_base_override: Option<u16>,
    skins_per_mode: Vec<Option<String>>,
) -> std::io::Result<()> {
    if modes.is_empty() || brains.is_empty() || per_group_count == 0 {
        tracing::error!("competition needs at least one mode, one brain, and --count >= 1");
        return Ok(());
    }
    // A group is one (navmode, brain, q3char?) tuple; the competition spawns `per_group_count`
    // bots for the full cross product. The `q3char` axis only expands the `q3` brain (others get a
    // single `None` sub-group). Group tags are `<mode>_<brain>[_<q3char>]` (see `group_tag`).
    // The Q3 personalities that expand a brain into sub-groups (`[None]` = the default character).
    let chars_for = |bk: brain::BrainKind| -> Vec<Option<brain::Q3CharPreset>> {
        if bk == brain::BrainKind::Quake3 && !q3chars.is_empty() {
            q3chars.iter().map(|&c| Some(c)).collect()
        } else {
            vec![None]
        }
    };
    let groups_per_mode: usize = brains.iter().map(|&b| chars_for(b).len()).sum();
    let num_groups = modes.len() * groups_per_mode;
    // maxclients guard: clamp the per-group count so `groups × count` leaves human headroom.
    let mut per_group_count = per_group_count;
    if cfg.fleet.max_bots > 0 && num_groups * per_group_count > cfg.fleet.max_bots {
        let clamped = (cfg.fleet.max_bots / num_groups).max(1);
        tracing::warn!(
            requested_per_group = per_group_count,
            groups = num_groups,
            cap = cfg.fleet.max_bots,
            clamped_per_group = clamped,
            "clamping per-group count to fit max_bots"
        );
        per_group_count = clamped;
    }
    let total = num_groups * per_group_count;

    let stagger = cfg.fleet.connect_stagger_ms;
    let reconnect = Reconnect {
        enabled: cfg.fleet.reconnect,
        max_attempts: cfg.fleet.max_reconnects,
    };
    let shutdown = Shutdown::new();
    let stats = FleetStats::new();
    let _signals = spawn_signal_listener(shutdown.clone());
    let shared = FleetShared {
        nav: NavCache::new(), // ONE shared cache across every mode (the in-process perf win)
        shutdown: shutdown.clone(),
        stats: stats.clone(),
    };
    // Contiguous per-mode qport blocks (`base + mi*count + i`) are disjoint, so the server's
    // (ip, qport) slot keys never collide across modes.
    let qport_base = qport_base_override.unwrap_or_else(crate::default_fleet_qport_base);
    tracing::info!(
        modes = modes.len(),
        brains = brains.len(),
        groups = num_groups,
        per_group_count,
        total,
        qport_base,
        "launching competition to {addr}"
    );

    // Stable group ordering (mode-major, brain-minor) → contiguous, disjoint qport blocks.
    // `group_tags` is the scoreboard's grouping key list, in the same order.
    let mut tasks = Vec::new();
    let mut group_tags: Vec<String> = Vec::new();
    let mut g = 0usize;
    for (mi, &mode) in modes.iter().enumerate() {
        let mode_skin = skins_per_mode.get(mi).cloned().flatten();
        for &bk in &brains {
            for q3char in chars_for(bk) {
                let tag = group_tag(mode, bk, q3char);
                // A named Q3 character wears its own recognizable skin; else the per-mode skin.
                let skin = q3char
                    .map(|q| q.skin().to_string())
                    .or_else(|| mode_skin.clone());
                group_tags.push(tag.clone());
                tracing::info!(group = %tag, skin = ?skin, count = per_group_count, "competitor entering");
                for i in 0..per_group_count {
                    let name = format!("{tag}_{}", i + 1);
                    let qport = qport_base.wrapping_add((g * per_group_count + i) as u16);
                    let bot_skin = skin.clone();
                    let cfg = Arc::clone(&cfg);
                    let shared = shared.clone();
                    tasks.push(tokio::spawn(async move {
                        bot_supervisor_loop(
                            addr, name, qport, bot_skin, cfg, shared, reconnect, mode, bk, q3char,
                        )
                        .await;
                    }));
                    time::sleep(Duration::from_millis(stagger)).await;
                }
                g += 1;
            }
        }
    }

    // Heartbeat: a live per-group scoreboard every 30 s.
    let sd = shutdown.clone();
    let hb_stats = stats.clone();
    let hb_tags = group_tags.clone();
    let status = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            log_competition_scoreboard(&hb_stats, &hb_tags, "live");
            if sd.requested() {
                break;
            }
        }
    });

    for t in tasks {
        let _ = t.await;
    }
    status.abort();
    log_competition_scoreboard(&stats, &group_tags, "FINAL");
    tracing::info!("competition exited");
    Ok(())
}

/// The scoreboard grouping tag for a `(mode, brain, q3char?)` group: nav plan first, brain always
/// present, underscore-joined → `<mode>_<brain>[_<q3char>]` (e.g. `astar_main`, `race_q3`,
/// `race_q3_grunt`). Every token is separator-free, so the `<tag>_<i>` bot name still index-splits
/// on its trailing `_` in [`mode_scoreboard`].
fn group_tag(
    mode: crate::NavMode,
    brain: brain::BrainKind,
    q3char: Option<brain::Q3CharPreset>,
) -> String {
    match q3char {
        Some(c) => format!("{}_{}_{}", mode_tag(mode), brain::brain_tag(brain), c.tag()),
        None => format!("{}_{}", mode_tag(mode), brain::brain_tag(brain)),
    }
}

/// One mode's aggregate competition standing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModeScore {
    tag: String,
    kills: u64,
    deaths: u64,
    bots: usize,
}

/// Group the fleet's per-bot tallies by the `<group_tag>_<i>` name prefix and sum kills/deaths,
/// seeding every competing group (from `group_tags`) so a frag-less group still shows. Returned
/// ranked by kills desc (then tag) — pure, so the scoreboard formatting is unit-testable.
fn mode_scoreboard(stats: &FleetStats, group_tags: &[String]) -> Vec<ModeScore> {
    use std::collections::HashMap;
    let mut by_tag: HashMap<String, (u64, u64, usize)> = HashMap::new();
    for t in group_tags {
        by_tag.entry(t.clone()).or_default();
    }
    for (name, tally) in stats.snapshot() {
        let tag = name
            .rsplit_once('_')
            .map(|(p, _)| p.to_string())
            .unwrap_or(name);
        let e = by_tag.entry(tag).or_default();
        e.0 += tally.kills;
        e.1 += tally.deaths;
        e.2 += 1;
    }
    let mut rows: Vec<ModeScore> = by_tag
        .into_iter()
        .map(|(tag, (kills, deaths, bots))| ModeScore {
            tag,
            kills,
            deaths,
            bots,
        })
        .collect();
    rows.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.tag.cmp(&b.tag)));
    rows
}

/// Log a per-group frag scoreboard. `label` distinguishes the periodic "live" board from "FINAL".
fn log_competition_scoreboard(stats: &FleetStats, group_tags: &[String], label: &str) {
    tracing::info!("── competition scoreboard [{label}] (group: kills/deaths, K/D) ──");
    for (rank, s) in mode_scoreboard(stats, group_tags).iter().enumerate() {
        let kd = if s.deaths > 0 {
            s.kills as f32 / s.deaths as f32
        } else {
            s.kills as f32
        };
        tracing::info!(
            "  #{:<2} {:<9} bots={:<2} kills={:<4} deaths={:<4} kd={:.2}",
            rank + 1,
            s.tag,
            s.bots,
            s.kills,
            s.deaths,
            kd
        );
    }
}

/// Per-bot supervisor: run `bot_task`, and if it exits due to a disconnect
/// (not shutdown), reconnect with exponential backoff up to `max_reconnects`.
#[allow(clippy::too_many_arguments)]
async fn bot_supervisor_loop(
    addr: SocketAddr,
    name: String,
    qport: u16,
    skin: Option<String>,
    cfg: Arc<Config>,
    shared: FleetShared,
    reconnect: Reconnect,
    mode: crate::NavMode,
    brain: brain::BrainKind,
    q3char: Option<brain::Q3CharPreset>,
) {
    let mut attempts: u32 = 0;
    let mut backoff_ms: u64 = 1000;
    loop {
        if shared.shutdown.requested() {
            return;
        }
        match crate::bot_task(
            addr,
            &name,
            qport,
            skin.as_deref(),
            &cfg,
            &shared.nav,
            &shared.shutdown,
            &shared.stats,
            mode,
            brain,
            q3char,
        )
        .await
        {
            Ok(()) => {
                tracing::info!(%name, "bot task exited");
            }
            Err(e) => {
                tracing::warn!(%name, "bot task error: {e}");
            }
        }
        if !reconnect.enabled || shared.shutdown.requested() {
            return;
        }
        attempts += 1;
        if reconnect.max_attempts > 0 && attempts > reconnect.max_attempts {
            tracing::warn!(%name, attempts, "giving up after max reconnects");
            return;
        }
        tracing::info!(%name, backoff_ms, "reconnecting");
        shared
            .shutdown
            .sleep_or_cancel(Duration::from_millis(backoff_ms))
            .await;
        backoff_ms = (backoff_ms * 2).min(15_000);
    }
}

/// Run a single bot for `connect-one`. Builds a private nav cache + shutdown and
/// installs the signal listener, then runs one `bot_task` (no reconnect loop).
#[allow(clippy::too_many_arguments)]
pub async fn run_single(
    cfg: &Config,
    addr: SocketAddr,
    name: &str,
    qport: u16,
    mode: crate::NavMode,
    brain: brain::BrainKind,
    q3char: Option<brain::Q3CharPreset>,
) -> std::io::Result<()> {
    let nav = NavCache::new();
    let shutdown = Shutdown::new();
    let stats = FleetStats::new();
    let _signals = spawn_signal_listener(shutdown.clone());
    // A selected Q3 character wears its recognizable skin even as a single bot.
    let skin = q3char.map(|q| q.skin());
    let res = crate::bot_task(
        addr, name, qport, skin, cfg, &nav, &shutdown, &stats, mode, brain, q3char,
    )
    .await;
    // bot_task has disconnected (or errored) — emit the single-bot tally.
    log_final_stats(&stats);
    res
}

/// Emit the fleet's final kill/death tally: totals + a per-bot breakdown (frag
/// leaders first). Call after the fleet has disconnected.
fn log_final_stats(stats: &FleetStats) {
    let totals = stats.totals();
    tracing::info!(
        kills = totals.kills,
        deaths = totals.deaths,
        bots = stats.bot_count(),
        "fleet final stats"
    );
    for (name, t) in stats.snapshot() {
        tracing::info!("{:>3} kills / {:>3} deaths  {}", t.kills, t.deaths, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_scoreboard_groups_by_name_prefix_and_ranks_by_kills() {
        let stats = FleetStats::new();
        // race fleet: 3 kills / 1 death across 2 bots; astar fleet: 1 kill / 2 deaths.
        stats.record_kill("race_1");
        stats.record_kill("race_1");
        stats.record_kill("race_2");
        stats.record_death("race_2");
        stats.record_kill("astar_1");
        stats.record_death("astar_1");
        stats.record_death("astar_2");
        // Multi-underscore group tag (`<mode>_<brain>_<char>`): the index still splits off the
        // trailing `_`, so this attributes to `astar_q3_grunt`, not `astar_q3`.
        stats.record_kill("astar_q3_grunt_1");

        let group_tags = vec![
            "astar".to_string(),
            "race".to_string(),
            "astar_q3_grunt".to_string(),
            "navmesh".to_string(), // never fragged → must still appear, last
        ];
        let board = mode_scoreboard(&stats, &group_tags);
        assert_eq!(board.len(), 4);
        // Ranked by kills desc: race (3) > {astar (1), astar_q3_grunt (1) by tag} > navmesh (0).
        assert_eq!(board[0].tag, "race");
        assert_eq!((board[0].kills, board[0].deaths, board[0].bots), (3, 1, 2));
        // Two groups tied at 1 kill → ordered by tag asc: `astar` before `astar_q3_grunt`.
        assert_eq!(board[1].tag, "astar");
        assert_eq!((board[1].kills, board[1].deaths, board[1].bots), (1, 2, 2));
        assert_eq!(board[2].tag, "astar_q3_grunt");
        assert_eq!((board[2].kills, board[2].deaths, board[2].bots), (1, 0, 1));
        assert_eq!(board[3].tag, "navmesh");
        assert_eq!((board[3].kills, board[3].deaths, board[3].bots), (0, 0, 0));
    }

    #[test]
    fn group_tag_is_mode_first_underscore_joined() {
        use crate::NavMode;
        use brain::{BrainKind, Q3CharPreset};
        // Brain always present; nav plan first; underscore-joined.
        assert_eq!(
            group_tag(NavMode::Astar, BrainKind::Main, None),
            "astar_main"
        );
        assert_eq!(
            group_tag(NavMode::HybridRace, BrainKind::Quake3, None),
            "race_q3"
        );
        assert_eq!(
            group_tag(
                NavMode::HybridRace,
                BrainKind::Quake3,
                Some(Q3CharPreset::Grunt)
            ),
            "race_q3_grunt"
        );
        assert_eq!(
            group_tag(NavMode::Navmesh, BrainKind::Sentry, None),
            "navmesh_sentry"
        );
    }
}

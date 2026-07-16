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
    /// Static item table (Plan 30) — every `item_*`/`weapon_*`/`ammo_*` spawn from the BSP,
    /// classified + nearest-node-resolved once per map and shared to every bot via `BrainMap`.
    pub items: Vec<brain::brains::core::MapItem>,
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
    let built =
        match world::cached_map_nav(&cfg.paths.baseq2, map, Some(cache_dir), world::GRID_SPACING) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(map, "nav load failed: {e}");
                crate::fatal!(map, "aborting: no usable nav data for the server's map");
            }
        };
    // Hard abort: a broken nav graph means no bot on this map can navigate.
    // All Q2 dm maps guarantee full spawn reachability — failure is our bug.
    if let Err(diag) = world::check_spawn_connectivity(&built) {
        tracing::error!(map, "{diag}");
        crate::fatal!(
            map,
            "aborting: nav connectivity bug — bots cannot navigate this map"
        );
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
    // Static item table (Plan 30) — built here where the full BSP entity lump is still in scope,
    // before `built.graph` is moved into the shared `Arc`.
    let items = brain::items::build_map_items(&built.bsp, &built.graph);
    tracing::info!(map, item_spawns = items.len(), "map item table built");
    Some(MapNav {
        graph: Arc::new(built.graph),
        cm: built.cm,
        roam_nodes: built.largest,
        bounds,
        items,
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

/// Start the Plan 66 serverframe beacon, if `[beacon] enabled`.
///
/// `None` when disabled (the default) — no socket, no task, and `bot_task` takes exactly the
/// code path it took before Plan 66. A beacon that cannot bind logs and disables itself: it is
/// telemetry for qctrl, never a dependency of the fleet.
fn start_beacon(
    cfg: &Config,
    addr: SocketAddr,
    shutdown: &Shutdown,
) -> Option<crate::beacon::Beacon> {
    if !cfg.beacon.enabled {
        return None;
    }
    let beacon = crate::beacon::Beacon::new();
    tokio::spawn(crate::beacon::serve(
        beacon.clone(),
        cfg.beacon.clone(),
        // The resolved ip:port the bots actually reach, plus the name as configured. qctrl
        // checks a beacon against the server IT manages before trusting it, and the two sides
        // routinely spell the same host differently.
        addr.to_string(),
        cfg.server_addr(),
        shutdown.clone(),
    ));
    Some(beacon)
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
    /// First fatal join failure (server full / connect timeout), if any. Set once by the
    /// first bot that fails to join; the run returns an error unless `loose_botcap`.
    join_failure: Arc<Mutex<Option<String>>>,
    /// When true, a join failure only warns + drops that bot instead of failing the fleet.
    loose_botcap: bool,
    /// Plan 66: the serverframe beacon, when `[beacon] enabled` is set. `None` (the default)
    /// means no socket, no task, and no change to bot behaviour.
    beacon: Option<crate::beacon::Beacon>,
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
    char: Option<brain::CharPreset>,
    loose_botcap: bool,
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

    // Plan 55: refuse to launch a fleet the server can't seat, before spawning any bot.
    preflight_capacity(addr, count, loose_botcap).await?;

    let stagger = cfg.fleet.connect_stagger_ms;
    let reconnect = Reconnect {
        enabled: cfg.fleet.reconnect,
        max_attempts: cfg.fleet.max_reconnects,
    };

    let nav_cache = NavCache::new();
    let shutdown = Shutdown::new();
    let stats = FleetStats::new();
    let _signals = spawn_signal_listener(shutdown.clone());

    let beacon = start_beacon(&cfg, addr, &shutdown);

    let shared = FleetShared {
        nav: nav_cache,
        shutdown: shutdown.clone(),
        stats: stats.clone(),
        join_failure: Arc::new(Mutex::new(None)),
        loose_botcap,
        beacon,
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
        let xonchar = cfg.fleet.xonchar_preset();
        let bot_skin = char
            .map(|q| q.skin().to_string())
            .or_else(|| xonchar.map(|x| x.skin().to_string()))
            .or_else(|| skin.per_bot(&mut skin_rng));
        let cfg = Arc::clone(&cfg);
        let shared = shared.clone();
        tasks.push(tokio::spawn(async move {
            bot_supervisor_loop(
                addr, name, qport, bot_skin, cfg, shared, reconnect, mode, brain, char, xonchar,
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
                env_suicides = totals.env_total(),
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
    fleet_join_result(&shared)
}

/// Turn a recorded fatal join failure into a process-level error. `Ok(())` when every bot
/// joined (or `--loose-botcap` downgraded the failures to warnings, leaving the slot unset).
fn fleet_join_result(shared: &FleetShared) -> std::io::Result<()> {
    if let Some(reason) = shared.join_failure.lock().unwrap().take() {
        return Err(std::io::Error::other(format!(
            "fleet join failed: {reason} — raise the server's maxclients or pass \
             --loose-botcap to proceed with warnings"
        )));
    }
    Ok(())
}

/// Whether `total` bots fit the server: `total <= maxclients - players` (saturating, so an
/// already-full or over-full server yields 0 free slots). Pure, for unit testing.
fn fits_capacity(total: usize, maxclients: u32, players: usize) -> bool {
    total <= (maxclients as usize).saturating_sub(players)
}

/// Capacity preflight (Plan 55): query the server's `status` **before** spawning and refuse
/// to launch a roster that can't fit. This is the early gate to Plan 53's join-time gate —
/// it exits immediately (non-zero, via the dispatch's `Err → ExitCode::FAILURE`) instead of
/// spawning bots that get refused mid-handshake.
///
/// We only block when we *know* it won't fit: a failed status query or a server that reports
/// no `maxclients` warns and proceeds (Plan 53 remains the backstop). `--loose-botcap`
/// downgrades a known over-subscription to a warning too.
async fn preflight_capacity(
    addr: SocketAddr,
    total: usize,
    loose_botcap: bool,
) -> std::io::Result<()> {
    let report = match crate::query_status(addr).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                "capacity preflight: status query failed ({e}); proceeding \
                 (join-time checks still apply)"
            );
            return Ok(());
        }
    };
    let Some(maxc) = report.maxclients else {
        tracing::warn!("capacity preflight: server reported no maxclients; skipping");
        return Ok(());
    };
    let players = report.player_count();
    let free = (maxc as usize).saturating_sub(players);
    if fits_capacity(total, maxc, players) {
        tracing::info!(
            want = total,
            free,
            players,
            maxclients = maxc,
            "capacity preflight ok"
        );
        return Ok(());
    }
    let msg = format!(
        "server can't fit the roster: want {total} bots but only {free} free slot(s) \
         ({players}/{maxc} in use)"
    );
    if loose_botcap {
        tracing::warn!("{msg}; --loose-botcap set, spawning anyway (expect join failures)");
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{msg} — free up slots, lower the count, or pass --loose-botcap"
        )))
    }
}

/// Full, human-readable name for a nav backend — used in the competition legend
/// ([`log_competition_legend`]) to expand the short [`mode_code`] used in bot names.
pub(crate) fn mode_tag(mode: crate::NavMode) -> &'static str {
    match mode {
        crate::NavMode::Astar => "astar",
        crate::NavMode::Navmesh => "navmesh",
        crate::NavMode::HybridFallback => "fallback",
        crate::NavMode::HybridRace => "race",
        crate::NavMode::HybridHier => "hier",
        crate::NavMode::HybridSegment => "segment",
        crate::NavMode::XonGoal => "xon-goal",
    }
}

/// 2-char code for a nav backend, used in competition bot names so `<brain>_<mode>[_<char>]_<i>`
/// stays within Q2's 15-char `netname` limit (`game/player/client.c` `Q_strlcpy` into
/// `netname[16]`). Names are the only consumer; the full name is [`mode_tag`].
pub(crate) fn mode_code(mode: crate::NavMode) -> &'static str {
    match mode {
        crate::NavMode::Astar => "as",
        crate::NavMode::Navmesh => "nm",
        crate::NavMode::HybridFallback => "fb",
        crate::NavMode::HybridRace => "rc",
        crate::NavMode::HybridHier => "hr",
        crate::NavMode::HybridSegment => "sg",
        crate::NavMode::XonGoal => "xg",
    }
}

/// 3-char code for a brain, used in competition bot names (see [`mode_code`]). The full,
/// log-facing name is [`brain::brain_tag`]. `q3`/`zb2` keep their already-short iconic forms.
pub(crate) fn brain_code(brain: brain::BrainKind) -> &'static str {
    match brain {
        brain::BrainKind::Main => "mai",
        brain::BrainKind::Sentry => "sen",
        brain::BrainKind::RunTester => "run",
        brain::BrainKind::Quake3 => "q3",
        brain::BrainKind::Zb2 => "zb2",
        brain::BrainKind::Xon => "xon",
    }
}

/// The per-group personality axis: q3 groups may carry a Q3 character, xon groups an
/// Xonotic character; every other brain has none (Plan 62 T1).
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum GroupChar {
    None,
    Q3(brain::CharPreset),
    Xon(brain::XonCharPreset),
}

impl GroupChar {
    pub(crate) fn q3(self) -> Option<brain::CharPreset> {
        match self {
            GroupChar::Q3(c) => Some(c),
            _ => None,
        }
    }
    pub(crate) fn xon(self) -> Option<brain::XonCharPreset> {
        match self {
            GroupChar::Xon(x) => Some(x),
            _ => None,
        }
    }
    pub(crate) fn skin(self) -> Option<String> {
        match self {
            GroupChar::Q3(c) => Some(c.skin().to_string()),
            GroupChar::Xon(x) => Some(x.skin().to_string()),
            GroupChar::None => None,
        }
    }
}

/// 3-char code for an Xonotic character (xon brain only) — same 15-char-name budget as
/// [`char_code`]. The full name is [`brain::XonCharPreset::tag`].
pub(crate) fn xon_char_code(x: brain::XonCharPreset) -> &'static str {
    match x {
        brain::XonCharPreset::Rusher => "rus",
        brain::XonCharPreset::Sharp => "shp",
        brain::XonCharPreset::Turtle => "trt",
        brain::XonCharPreset::Noob => "nob",
    }
}

/// 3-char code for a Q3 character (q3 brain only), used in competition bot names (see
/// [`mode_code`]). The full, log-facing name is [`brain::CharPreset::tag`].
pub(crate) fn char_code(char: brain::CharPreset) -> &'static str {
    match char {
        brain::CharPreset::Grunt => "gru",
        brain::CharPreset::Major => "maj",
        brain::CharPreset::Sarge => "sar",
        brain::CharPreset::Camper => "cam",
    }
}

/// One competition group: a `(mode, brain, char?)` combo, its per-group bot `count`, the skin all
/// its bots wear, and the scoreboard `tag`. The CLI-matrix path builds these via [`matrix_specs`];
/// the `--roster` path builds them from a YAML file (`crate::roster`). `run_competition` consumes a
/// `Vec<GroupSpec>` and no longer knows which path produced it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupSpec {
    pub(crate) mode: crate::NavMode,
    pub(crate) brain: brain::BrainKind,
    pub(crate) gc: GroupChar,
    pub(crate) count: usize,
    pub(crate) skin: Option<String>,
    pub(crate) tag: String,
}

/// Expand the CLI matrix (`modes × brains × chars_for`) into the flat group list, preserving the
/// historical **mode-major, brain-minor, char-innermost** ordering and the `gc.skin().or(mode_skin)`
/// rule — so the matrix competition path is byte-for-byte what it was before the `GroupSpec`
/// refactor (see the equivalence test). `skins_per_mode` is positionally indexed by `modes` (as it
/// was in `run_competition`). Every group gets the same `per_group_count`; roster files vary it.
pub(crate) fn matrix_specs(
    modes: &[crate::NavMode],
    brains: &[brain::BrainKind],
    chars: &[brain::CharPreset],
    xonchars: &[brain::XonCharPreset],
    per_group_count: usize,
    skins_per_mode: &[Option<String>],
) -> Vec<GroupSpec> {
    // The `char` axis only expands the `q3`/`xon` brains (others get a single `None` sub-group).
    let chars_for = |bk: brain::BrainKind| -> Vec<GroupChar> {
        if bk == brain::BrainKind::Quake3 && !chars.is_empty() {
            chars.iter().map(|&c| GroupChar::Q3(c)).collect()
        } else if bk == brain::BrainKind::Xon && !xonchars.is_empty() {
            xonchars.iter().map(|&x| GroupChar::Xon(x)).collect()
        } else {
            vec![GroupChar::None]
        }
    };
    let mut specs = Vec::new();
    for (mi, &mode) in modes.iter().enumerate() {
        let mode_skin = skins_per_mode.get(mi).cloned().flatten();
        for &brain in brains {
            for gc in chars_for(brain) {
                // A named character wears its own recognizable skin; else the per-mode skin.
                let skin = gc.skin().or_else(|| mode_skin.clone());
                specs.push(GroupSpec {
                    mode,
                    brain,
                    gc,
                    count: per_group_count,
                    skin,
                    tag: group_tag(mode, brain, gc),
                });
            }
        }
    }
    specs
}

/// Run a **competition**: spawn each [`GroupSpec`]'s bots in a single process, all sharing one
/// `NavCache` (built once, not once per mode), each group wearing a distinct skin. Bots are named
/// `<tag>_<i>` with short-code tags (e.g. `mai_as_1`, `q3_rc_gru_1`) so the name fits Q2's 15-char
/// limit and the per-group frag scoreboard can group them (a code→full-name legend is logged at
/// launch). Returns when all bots exit (after shutdown). Reuses the fleet's per-bot supervisor loop
/// (reconnect/backoff/graceful disconnect) with a **per-bot** `mode`/`brain`/`char`. Emits a
/// K/D-ranked roster YAML to `./logs/roster/` on exit (edit it down for the next round).
pub async fn run_competition(
    cfg: Arc<Config>,
    addr: SocketAddr,
    mut specs: Vec<GroupSpec>,
    qport_base_override: Option<u16>,
    loose_botcap: bool,
) -> std::io::Result<()> {
    if specs.is_empty() {
        tracing::error!("competition needs at least one group");
        return Ok(());
    }
    // maxclients guard: clamp per-group counts so the total leaves human headroom. Scale each
    // group proportionally to its requested count (for uniform counts this is bit-identical to the
    // old `(max_bots / num_groups).max(1)` — integer identity, asserted in the clamp test).
    let requested_total: usize = specs.iter().map(|s| s.count).sum();
    if cfg.fleet.max_bots > 0 && requested_total > cfg.fleet.max_bots {
        for s in &mut specs {
            s.count = (s.count * cfg.fleet.max_bots / requested_total).max(1);
        }
        tracing::warn!(
            requested_total,
            cap = cfg.fleet.max_bots,
            clamped_total = specs.iter().map(|s| s.count).sum::<usize>(),
            "clamping per-group counts to fit max_bots"
        );
    }
    let num_groups = specs.len();
    let total: usize = specs.iter().map(|s| s.count).sum();

    // Plan 55: bail out now if the server plainly can't seat this many bots, before we
    // spawn any. Strict (default) exits non-zero; --loose-botcap warns and proceeds.
    preflight_capacity(addr, total, loose_botcap).await?;

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
        join_failure: Arc::new(Mutex::new(None)),
        loose_botcap,
        beacon: start_beacon(&cfg, addr, &shutdown),
    };
    // Contiguous per-group qport blocks (`base + running_offset + i`) are disjoint, so the
    // server's (ip, qport) slot keys never collide across groups.
    let qport_base = qport_base_override.unwrap_or_else(crate::default_fleet_qport_base);
    tracing::info!(
        groups = num_groups,
        total,
        qport_base,
        "launching competition to {addr}"
    );
    // Bot names use short codes to fit Q2's 15-char limit; print the legend so the
    // scoreboard's `mai_as`-style tags are readable.
    log_competition_legend(&specs);

    // `group_tags` is the scoreboard's grouping key list, in spec order.
    let mut tasks = Vec::new();
    let mut group_tags: Vec<String> = Vec::new();
    let mut qport_offset = 0usize;
    for spec in &specs {
        group_tags.push(spec.tag.clone());
        tracing::info!(group = %spec.tag, skin = ?spec.skin, count = spec.count, "competitor entering");
        for i in 0..spec.count {
            let name = format!("{}_{}", spec.tag, i + 1);
            let qport = qport_base.wrapping_add((qport_offset + i) as u16);
            let bot_skin = spec.skin.clone();
            let (mode, bk, gc) = (spec.mode, spec.brain, spec.gc);
            let cfg = Arc::clone(&cfg);
            let shared = shared.clone();
            tasks.push(tokio::spawn(async move {
                bot_supervisor_loop(
                    addr,
                    name,
                    qport,
                    bot_skin,
                    cfg,
                    shared,
                    reconnect,
                    mode,
                    bk,
                    gc.q3(),
                    gc.xon(),
                )
                .await;
            }));
            time::sleep(Duration::from_millis(stagger)).await;
        }
        qport_offset += spec.count;
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
    log_map_changes(&stats);
    // Plan 69: emit a ranked, ready-to-edit roster of every group — trim it and pass it back via
    // `--roster` for the next round. Written before `fleet_join_result` so a join-failure run
    // (which returns Err) still leaves the standings on disk.
    dump_final_roster(&stats, &group_tags, &specs);
    tracing::info!("competition exited");
    fleet_join_result(&shared)
}

/// Report how many map changes the run went through — one aggregated fleet-wide number plus the
/// chronological map sequence (Plan 70). Silent when the run never rotated (a single level).
fn log_map_changes(stats: &FleetStats) {
    let changes = stats.map_changes();
    let seq = stats.map_sequence();
    if changes == 0 {
        // Still note the single map for context, but no "changes" fanfare.
        if let Some(only) = seq.first() {
            tracing::info!(map_changes = 0, map = %only, "run stayed on one map");
        }
        return;
    }
    tracing::info!(
        map_changes = changes,
        levels = seq.len(),
        maps = %seq.join(" → "),
        "run went through {changes} map change(s)"
    );
}

/// Write the FINAL standings as a ranked roster YAML to `./logs/roster/<unix_ts>.yaml` (Plan 69).
/// Best-effort: an IO error is logged, never fatal — the competition already ran.
fn dump_final_roster(stats: &FleetStats, group_tags: &[String], specs: &[GroupSpec]) {
    let ranked = mode_scoreboard(stats, group_tags);
    let yaml = crate::roster::emit_ranked_yaml(&ranked, specs);
    let dir = std::path::Path::new("logs/roster");
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("could not create {}: {e}", dir.display());
        return;
    }
    // `::time` (the crate) — module-local `time` is `tokio::time` here.
    let ts = ::time::OffsetDateTime::now_utc().unix_timestamp().max(0);
    let path = dir.join(format!("{ts}.yaml"));
    match std::fs::write(&path, yaml) {
        Ok(()) => {
            tracing::info!(path = %path.display(), "wrote ranked roster (edit + --roster for a rematch)")
        }
        Err(e) => tracing::warn!("could not write {}: {e}", path.display()),
    }
}

/// Log a one-line-per-axis legend mapping the short codes used in bot names back to their full
/// names, for the brains/modes/chars actually fielded this run (first-seen order over the specs).
/// Keeps the short-code scoreboard readable without bloating the names themselves.
fn log_competition_legend(specs: &[GroupSpec]) {
    // First-seen dedup preserving order (small N; a Vec is simpler than an ordered set).
    fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
        if !v.contains(&x) {
            v.push(x);
        }
    }
    let mut modes: Vec<crate::NavMode> = Vec::new();
    let mut brains: Vec<brain::BrainKind> = Vec::new();
    let mut chars: Vec<brain::CharPreset> = Vec::new();
    let mut xonchars: Vec<brain::XonCharPreset> = Vec::new();
    for s in specs {
        push_unique(&mut modes, s.mode);
        push_unique(&mut brains, s.brain);
        if let Some(c) = s.gc.q3() {
            push_unique(&mut chars, c);
        }
        if let Some(x) = s.gc.xon() {
            push_unique(&mut xonchars, x);
        }
    }
    let join = |pairs: Vec<String>| pairs.join(", ");
    let brain_leg = join(
        brains
            .iter()
            .map(|&b| format!("{}={}", brain_code(b), brain::brain_tag(b)))
            .collect(),
    );
    let mode_leg = join(
        modes
            .iter()
            .map(|&m| format!("{}={}", mode_code(m), mode_tag(m)))
            .collect(),
    );
    tracing::info!("name-code legend — brain: {brain_leg}");
    tracing::info!("name-code legend — mode:  {mode_leg}");
    if !chars.is_empty() {
        let char_leg = join(
            chars
                .iter()
                .map(|&c| format!("{}={}", char_code(c), c.tag()))
                .collect(),
        );
        tracing::info!("name-code legend — char:  {char_leg}");
    }
    if !xonchars.is_empty() {
        let xc_leg = join(
            xonchars
                .iter()
                .map(|&x| format!("{}={}", xon_char_code(x), x.tag()))
                .collect(),
        );
        tracing::info!("name-code legend — xonchar: {xc_leg}");
    }
}

/// The scoreboard grouping tag for a `(mode, brain, char?)` group: short brain code first,
/// then nav-plan code, then the optional character code — underscore-joined →
/// `<brain>_<mode>[_<char>]` (e.g. `mai_as`, `q3_rc`, `q3_rc_gru`). Short codes keep the
/// `<tag>_<i>` bot name inside Q2's 15-char `netname` limit. Every token is `_`-free, so the
/// name still index-splits on its trailing `_` in [`mode_scoreboard`].
pub(crate) fn group_tag(mode: crate::NavMode, brain: brain::BrainKind, gc: GroupChar) -> String {
    match gc {
        GroupChar::Q3(c) => format!("{}_{}_{}", brain_code(brain), mode_code(mode), char_code(c)),
        GroupChar::Xon(x) => {
            format!(
                "{}_{}_{}",
                brain_code(brain),
                mode_code(mode),
                xon_char_code(x)
            )
        }
        GroupChar::None => format!("{}_{}", brain_code(brain), mode_code(mode)),
    }
}

/// One mode's aggregate competition standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModeScore {
    pub(crate) tag: String,
    pub(crate) kills: u64,
    pub(crate) deaths: u64,
    pub(crate) env_suicides: u64,
    /// Health points picked up (Plan 67) — amounts, not item counts.
    pub(crate) health_picked: u64,
    /// Armor points picked up (Plan 67).
    pub(crate) armor_picked: u64,
    /// Weapons picked up (Plan 68) — a count.
    pub(crate) weapons_picked: u64,
    pub(crate) bots: usize,
}

impl ModeScore {
    /// Kill/death ratio; a death-less group scores its raw kill count (the
    /// display convention the board has always used).
    pub(crate) fn kd(&self) -> f32 {
        if self.deaths > 0 {
            self.kills as f32 / self.deaths as f32
        } else {
            self.kills as f32
        }
    }
}

/// Group the fleet's per-bot tallies by the `<group_tag>_<i>` name prefix and sum kills/deaths,
/// seeding every competing group (from `group_tags`) so a frag-less group still shows. Returned
/// ranked by K/D desc, then kills desc (efficiency first, volume as tiebreak), then tag —
/// pure, so the scoreboard formatting is unit-testable.
fn mode_scoreboard(stats: &FleetStats, group_tags: &[String]) -> Vec<ModeScore> {
    use std::collections::HashMap;
    #[derive(Default)]
    struct Acc {
        kills: u64,
        deaths: u64,
        env: u64,
        hp: u64,
        ap: u64,
        wp: u64,
        bots: usize,
    }
    let mut by_tag: HashMap<String, Acc> = HashMap::new();
    for t in group_tags {
        by_tag.entry(t.clone()).or_default();
    }
    for (name, tally) in stats.snapshot() {
        let tag = name
            .rsplit_once('_')
            .map(|(p, _)| p.to_string())
            .unwrap_or(name);
        let e = by_tag.entry(tag).or_default();
        e.kills += tally.kills;
        e.deaths += tally.deaths;
        e.env += tally.env_total();
        e.hp += tally.health_picked;
        e.ap += tally.armor_picked;
        e.wp += tally.weapons_picked;
        e.bots += 1;
    }
    let mut rows: Vec<ModeScore> = by_tag
        .into_iter()
        .map(|(tag, a)| ModeScore {
            tag,
            kills: a.kills,
            deaths: a.deaths,
            env_suicides: a.env,
            health_picked: a.hp,
            armor_picked: a.ap,
            weapons_picked: a.wp,
            bots: a.bots,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.kd()
            .total_cmp(&a.kd())
            .then_with(|| b.kills.cmp(&a.kills))
            .then_with(|| a.tag.cmp(&b.tag))
    });
    rows
}

/// Log a per-group frag scoreboard. `label` distinguishes the periodic "live" board from "FINAL".
fn log_competition_scoreboard(stats: &FleetStats, group_tags: &[String], label: &str) {
    tracing::info!(
        "── competition scoreboard [{label}] (K/D-ranked; env suicides; hp/ap = health/armor points, wp = weapons picked up) ──"
    );
    for (rank, s) in mode_scoreboard(stats, group_tags).iter().enumerate() {
        let kd = s.kd();
        tracing::info!(
            "  #{:<2} {:<9} bots={:<2} kills={:<4} deaths={:<4} kd={:.2} env={:<3} hp={:<5} ap={:<4} wp={:<3}",
            rank + 1,
            s.tag,
            s.bots,
            s.kills,
            s.deaths,
            kd,
            s.env_suicides,
            s.health_picked,
            s.armor_picked,
            s.weapons_picked
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
    char: Option<brain::CharPreset>,
    xonchar: Option<brain::XonCharPreset>,
) {
    let mut attempts: u32 = 0;
    let mut backoff_ms: u64 = 1000;
    // Plan 64: once a bot_task has completed a session (Ok = it connected and later
    // got disconnected), later handshake failures are REJOIN failures — the server is
    // mid-map-rotation, briefly full of ghost slots, or still loading. Those retry
    // with backoff; only a bot that never joined keeps the fatal Plan 53 semantics.
    let mut had_session = false;
    loop {
        if shared.shutdown.requested() {
            return;
        }
        // Per-bot log attribution: instrument the FUTURE (a `span.enter()` inside the
        // async fn leaks across `.await` and cross-tags other bots' events).
        let span = tracing::info_span!("bot", %name, qport);
        // Plan 65: the reconnect budget guards CONSECUTIVE failed attempts, not lifetime
        // reconnects — hours of map rotations must not exhaust `max_reconnects` or pin
        // the backoff at its cap. A task that ran this long had a real session (a failing
        // handshake exits within connect_timeout_ms), so its exit resets the budget.
        // Time-based rather than `Ok`-based because a genuine session can still end in
        // `Err` (e.g. the frame-stall watchdog's ConnectionReset).
        const BUDGET_RESET_AFTER: Duration = Duration::from_secs(60);
        let session_start = time::Instant::now();
        // Plan 65: run bot_task as its OWN tokio task so a panic anywhere inside it
        // (e.g. a brain bug — live: stale roam_idx indexing the new map's smaller graph)
        // is caught at the task boundary as a JoinError instead of unwinding this
        // supervisor loop. Before this, one brain panic silently removed the bot from
        // the fleet forever; now it's just another retryable session end.
        let task = tokio::spawn({
            let name = name.clone();
            let skin = skin.clone();
            let cfg = Arc::clone(&cfg);
            let shared = shared.clone();
            async move {
                tracing::Instrument::instrument(
                    crate::bot_task(
                        addr,
                        &name,
                        qport,
                        skin.as_deref(),
                        &cfg,
                        &shared.nav,
                        &shared.shutdown,
                        &shared.stats,
                        shared.beacon.as_ref(),
                        mode,
                        brain,
                        char,
                        None, // TODO(P27): per-bot fleet persona from config
                        xonchar,
                    ),
                    span,
                )
                .await
            }
        });
        let result = match task.await {
            Ok(r) => r,
            Err(join_err) => Err(std::io::Error::other(format!(
                "bot task panicked: {join_err}"
            ))),
        };
        match result {
            Ok(()) => {
                had_session = true;
                tracing::info!(%name, "bot task exited");
            }
            Err(e) => {
                // An INITIAL join failure (server full / connect timeout) is never
                // retryable — the slot won't open by trying again. In strict mode it
                // fails the whole fleet; with --loose-botcap it just drops this one bot
                // with a warning. After a completed session the same errors are rejoin
                // hiccups (map rotation in progress, ghost slots timing out) and fall
                // through to the normal retry-with-backoff below (Plan 64).
                if !had_session
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
                    )
                {
                    if shared.loose_botcap {
                        tracing::warn!(%name, "join failed ({e}); --loose-botcap set, dropping this bot");
                    } else {
                        tracing::error!(%name, "join failed ({e}); failing the fleet (pass --loose-botcap to proceed with warnings)");
                        let mut slot = shared.join_failure.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(e.to_string());
                        }
                        drop(slot);
                        shared.shutdown.fire();
                    }
                    return;
                }
                tracing::warn!(%name, "bot task error: {e}");
            }
        }
        if !reconnect.enabled || shared.shutdown.requested() {
            return;
        }
        if session_start.elapsed() >= BUDGET_RESET_AFTER {
            attempts = 0;
            backoff_ms = 1000;
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
    char: Option<brain::CharPreset>,
    persona: Option<brain::persona::Persona>,
    xonchar: Option<brain::XonCharPreset>,
) -> std::io::Result<()> {
    let nav = NavCache::new();
    let shutdown = Shutdown::new();
    let stats = FleetStats::new();
    let _signals = spawn_signal_listener(shutdown.clone());
    // A selected character wears its recognizable skin even as a single bot.
    let skin = char.map(|q| q.skin()).or(xonchar.map(|x| x.skin()));
    let span = tracing::info_span!("bot", %name, qport);
    let res = tracing::Instrument::instrument(
        // No beacon for `connect-one`: it's a single-bot dev tool, and the beacon is a
        // fleet-level facility that qctrl expects to be fed by a running fleet.
        crate::bot_task(
            addr, name, qport, skin, cfg, &nav, &shutdown, &stats, None, mode, brain, char,
            persona, xonchar,
        ),
        span,
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
        env_suicides = totals.env_total(),
        health_picked = totals.health_picked,
        armor_picked = totals.armor_picked,
        weapons_picked = totals.weapons_picked,
        bots = stats.bot_count(),
        "fleet final stats"
    );
    for (name, t) in stats.snapshot() {
        let env = t.env_breakdown();
        if env.is_empty() {
            tracing::info!(
                "{:>3} kills / {:>3} deaths / hp {:>4} ap {:>3} wp {:>2}  {}",
                t.kills,
                t.deaths,
                t.health_picked,
                t.armor_picked,
                t.weapons_picked,
                name
            );
        } else {
            tracing::info!(
                "{:>3} kills / {:>3} deaths / hp {:>4} ap {:>3} wp {:>2}  {}  [{}]",
                t.kills,
                t.deaths,
                t.health_picked,
                t.armor_picked,
                t.weapons_picked,
                name,
                env
            );
        }
    }
    if totals.env_total() > 0 {
        tracing::warn!(
            total = totals.env_total(),
            breakdown = %totals.env_breakdown(),
            "environmental suicides this run"
        );
    }
    log_map_changes(stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NavMode;
    use brain::{BrainKind, CharPreset};

    /// The running qport offset each spec's bots start at — the `GroupSpec` analog of the old
    /// `g * per_group_count` block base. Test helper mirroring `run_competition`'s cumsum.
    fn qport_offsets(specs: &[GroupSpec]) -> Vec<usize> {
        let mut offs = Vec::new();
        let mut acc = 0usize;
        for s in specs {
            offs.push(acc);
            acc += s.count;
        }
        offs
    }

    #[test]
    fn matrix_specs_reproduces_the_historical_group_order_and_qports() {
        // modes=[as,nm] × brains=[mai,q3] × chars=[gru,cam], count=2, one skin per mode.
        let specs = matrix_specs(
            &[NavMode::Astar, NavMode::Navmesh],
            &[BrainKind::Main, BrainKind::Quake3],
            &[CharPreset::Grunt, CharPreset::Camper],
            &[],
            2,
            &[Some("male/grunt".into()), Some("female/athena".into())],
        );
        // Mode-major, brain-minor, char-innermost; mai has no char axis (one None group).
        let tags: Vec<&str> = specs.iter().map(|s| s.tag.as_str()).collect();
        assert_eq!(
            tags,
            [
                "mai_as",
                "q3_as_gru",
                "q3_as_cam",
                "mai_nm",
                "q3_nm_gru",
                "q3_nm_cam"
            ]
        );
        // Uniform count 2 → offsets are exactly the old `g * per_group_count`.
        assert_eq!(qport_offsets(&specs), [0, 2, 4, 6, 8, 10]);
    }

    #[test]
    fn matrix_specs_char_skin_beats_mode_skin() {
        let specs = matrix_specs(
            &[NavMode::Astar],
            &[BrainKind::Main, BrainKind::Quake3],
            &[CharPreset::Grunt],
            &[],
            1,
            &[Some("male/mode".into())],
        );
        // mai (no char) wears the per-mode skin; the q3 char wears its own recognizable skin.
        let mai = specs.iter().find(|s| s.tag == "mai_as").unwrap();
        let q3 = specs.iter().find(|s| s.tag == "q3_as_gru").unwrap();
        assert_eq!(mai.skin.as_deref(), Some("male/mode"));
        assert_eq!(q3.skin.as_deref(), Some(CharPreset::Grunt.skin()));
    }

    #[test]
    fn matrix_specs_empty_xonchars_gives_one_none_xon_group() {
        let specs = matrix_specs(
            &[NavMode::Navmesh],
            &[BrainKind::Xon],
            &[],
            &[], // no xonchars → a single default (None) xon group
            3,
            &[None],
        );
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].tag, "xon_nm");
        assert_eq!(specs[0].gc, GroupChar::None);
        assert_eq!(specs[0].count, 3);
    }

    #[test]
    fn proportional_clamp_matches_old_uniform_formula() {
        // The old clamp was `(max_bots / num_groups).max(1)`, applied to a uniform count.
        // The new per-spec clamp is `(count * max_bots / total).max(1)`. For uniform counts
        // (total = num_groups * count) these are the same integer for every group.
        for &num_groups in &[1usize, 3, 6, 7] {
            for &count in &[1usize, 2, 5, 8] {
                for &max_bots in &[1usize, 4, 16, 30, 64] {
                    let total = num_groups * count;
                    let old = (max_bots / num_groups).max(1);
                    let new = (count * max_bots / total).max(1);
                    assert_eq!(
                        old, new,
                        "clamp mismatch: groups={num_groups} count={count} cap={max_bots}"
                    );
                }
            }
        }
    }

    #[test]
    fn mode_scoreboard_groups_by_name_prefix_and_ranks_by_kd_then_kills() {
        let stats = FleetStats::new();
        // race fleet: 3 kills / 1 death across 2 bots; astar fleet: 1 kill / 2 deaths.
        stats.record_kill("race_1");
        stats.record_kill("race_1");
        stats.record_kill("race_2");
        stats.record_death("race_2");
        stats.record_kill("astar_1");
        stats.record_death("astar_1");
        stats.record_death("astar_2");
        // Multi-underscore group tag (`<brain>_<mode>_<char>`): the index still splits off the
        // trailing `_`, so this attributes to `q3_astar_grunt`, not `q3_astar`.
        stats.record_kill("q3_astar_grunt_1");
        // Pickups sum per group across its bots (Plans 67/68).
        stats.record_health_pickup("race_1", 25);
        stats.record_health_pickup("race_2", 100);
        stats.record_armor_pickup("race_2", 50);
        stats.record_weapon_pickup("race_1");
        stats.record_weapon_pickup("race_2");
        stats.record_weapon_pickup("race_2");

        let group_tags = vec![
            "astar".to_string(),
            "race".to_string(),
            "q3_astar_grunt".to_string(),
            "navmesh".to_string(), // never fragged → must still appear, last
        ];
        let board = mode_scoreboard(&stats, &group_tags);
        assert_eq!(board.len(), 4);
        // Ranked by K/D desc: race (3.0) > q3_astar_grunt (1 kill, 0 deaths → 1.0)
        // > astar (0.5) > navmesh (0.0).
        assert_eq!(board[0].tag, "race");
        assert_eq!((board[0].kills, board[0].deaths, board[0].bots), (3, 1, 2));
        assert_eq!((board[0].health_picked, board[0].armor_picked), (125, 50));
        assert_eq!(board[0].weapons_picked, 3);
        assert_eq!(board[1].tag, "q3_astar_grunt");
        assert_eq!((board[1].kills, board[1].deaths, board[1].bots), (1, 0, 1));
        assert_eq!(board[2].tag, "astar");
        assert_eq!((board[2].kills, board[2].deaths, board[2].bots), (1, 2, 2));
        assert_eq!(board[3].tag, "navmesh");
        assert_eq!((board[3].kills, board[3].deaths, board[3].bots), (0, 0, 0));
    }

    #[test]
    fn mode_scoreboard_breaks_kd_ties_by_kills_then_tag() {
        let stats = FleetStats::new();
        // Both groups at K/D = 1.0, but `busy` did it at 3/3 vs `calm` at 1/1
        // → volume ranks busy first.
        for _ in 0..3 {
            stats.record_kill("busy_1");
            stats.record_death("busy_1");
        }
        stats.record_kill("calm_1");
        stats.record_death("calm_1");
        // Same K/D AND same kills as calm → falls through to tag order.
        stats.record_kill("zeta_1");
        stats.record_death("zeta_1");

        let tags = vec!["busy".to_string(), "calm".to_string(), "zeta".to_string()];
        let board = mode_scoreboard(&stats, &tags);
        assert_eq!(board[0].tag, "busy");
        assert_eq!(board[1].tag, "calm");
        assert_eq!(board[2].tag, "zeta");
    }

    #[test]
    fn fits_capacity_respects_free_slots() {
        // 55/64 in use → 9 free.
        assert!(fits_capacity(9, 64, 55), "9 bots fit in 9 free slots");
        assert!(!fits_capacity(10, 64, 55), "10 bots do not fit in 9 free");
        // Exactly full.
        assert!(fits_capacity(0, 64, 64), "0 bots always fit");
        assert!(!fits_capacity(1, 64, 64), "no room when server is full");
        // Over-full (saturating: never underflows to a huge free count).
        assert!(!fits_capacity(1, 64, 70), "over-full server has 0 free");
        // Empty server.
        assert!(fits_capacity(64, 64, 0), "full roster fits an empty server");
        assert!(
            !fits_capacity(65, 64, 0),
            "one past maxclients does not fit"
        );
    }

    #[test]
    fn group_tag_uses_short_codes_brain_first() {
        // Brain code first, then nav-plan code, then optional character code; underscore-joined.
        assert_eq!(
            group_tag(NavMode::Astar, BrainKind::Main, GroupChar::None),
            "mai_as"
        );
        assert_eq!(
            group_tag(NavMode::HybridRace, BrainKind::Quake3, GroupChar::None),
            "q3_rc"
        );
        assert_eq!(
            group_tag(
                NavMode::HybridRace,
                BrainKind::Quake3,
                GroupChar::Q3(CharPreset::Grunt)
            ),
            "q3_rc_gru"
        );
        assert_eq!(
            group_tag(NavMode::Navmesh, BrainKind::Sentry, GroupChar::None),
            "sen_nm"
        );
        assert_eq!(
            group_tag(NavMode::HybridFallback, BrainKind::Zb2, GroupChar::None),
            "zb2_fb"
        );
        assert_eq!(
            group_tag(
                NavMode::XonGoal,
                BrainKind::Xon,
                GroupChar::Xon(brain::XonCharPreset::Rusher)
            ),
            "xon_xg_rus"
        );
    }

    /// Every brain × mode × {no char, each char} combo must fit Q2's 15-char `netname` limit,
    /// even at a two-digit (or three-digit) bot index — the whole point of the short codes.
    #[test]
    fn every_competition_name_fits_15_chars() {
        use brain::{BrainKind, CharPreset, XonCharPreset};
        use clap::ValueEnum;
        let chars: Vec<GroupChar> = std::iter::once(GroupChar::None)
            .chain(
                CharPreset::value_variants()
                    .iter()
                    .map(|&c| GroupChar::Q3(c)),
            )
            .chain(
                XonCharPreset::value_variants()
                    .iter()
                    .map(|&x| GroupChar::Xon(x)),
            )
            .collect();
        for &mode in crate::NavMode::value_variants() {
            for &brain in BrainKind::value_variants() {
                for &char in &chars {
                    let tag = group_tag(mode, brain, char);
                    // Worst realistic index is 3 digits → `_999` (4 chars) appended.
                    let name_len = tag.len() + "_999".len();
                    assert!(
                        name_len <= 15,
                        "name `{tag}_999` is {name_len} chars (> 15): {brain:?}/{mode:?}/{char:?}"
                    );
                }
            }
        }
    }
}

//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` connects a single bot and keeps it alive; `run`
//! launches the full fleet (Plan 09). Server address and on-disk Q2 paths come from
//! `config.yaml`. The fleet supervisor + per-bot task live in [`supervisor`].

mod config;
mod scenario;
mod skins;
mod stats;
mod status;
mod supervisor;

use std::time::{Duration, Instant};

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use glam::Vec3;

/// Which navigation backend a movement scenario drives the bot with. Two co-maintained
/// representations behind one flag (`--mode`); the steering loop is identical for both.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NavMode {
    /// Waypoint-graph backend: A* over grid-sampled nodes (the default, proven backend).
    Astar,
    /// Navmesh backend: A* over walkable polygons + funnel (Recast-style).
    Navmesh,
    /// Hybrid: A* primary, navmesh takes over the segment on a hard-stuck (Plan 20).
    HybridFallback,
}

impl NavMode {
    /// True for the backends that need a navmesh built (so the factory can skip building it
    /// for pure `astar`, whose construction is graph-only).
    fn needs_mesh(self) -> bool {
        !matches!(self, NavMode::Astar)
    }
}

/// Build the navigation backend for `mode`, sharing the read-only nav graph and (for the
/// navmesh + hybrid modes) a navmesh built lazily by `build_mesh`. Both dispatch sites
/// (`bot_task` and the movement scenarios) go through this so the mode→backend mapping lives
/// in one place. `build_mesh` is only invoked for modes that need it.
fn build_navigator(
    mode: NavMode,
    graph: Arc<world::NavGraph>,
    build_mesh: impl FnOnce() -> Arc<world::NavMesh>,
) -> Box<dyn brain::Navigator + Send> {
    /// Navmesh funnel inset (agent radius) — matches the pure-navmesh dispatch.
    const AGENT_RADIUS: f32 = 16.0;
    let mesh = mode.needs_mesh().then(build_mesh);
    match mode {
        NavMode::Astar => Box::new(brain::NavigationDriver::new(graph)),
        NavMode::Navmesh => Box::new(brain::NavmeshDriver::new(mesh.unwrap(), AGENT_RADIUS)),
        NavMode::HybridFallback => Box::new(brain::hybrid::HybridFallback::new(
            graph,
            mesh.unwrap(),
            AGENT_RADIUS,
        )),
    }
}

#[derive(Parser)]
#[command(
    name = "qbots",
    about = "External Quake 2 bot clients that connect to a real server over UDP"
)]
struct Cli {
    /// Config file (server address + Q2 paths).
    #[arg(long, default_value = "config.yaml", global = true)]
    config: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect a single bot to a server and keep it alive.
    ConnectOne {
        /// Server address (defaults to config's server, e.g. `noir.lan:27910`).
        #[arg(long)]
        addr: Option<String>,
        /// Bot display name (userinfo `name`).
        #[arg(long)]
        name: Option<String>,
        /// Client qport (defaults to a per-process value; must be unique across bots).
        #[arg(long)]
        qport: Option<u16>,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-map-cache --map <m>` first.
        #[arg(long, value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
    },
    /// Launch the full bot fleet from the config's `[fleet]` roster.
    Run {
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Navigation backend for the whole fleet: `astar` (waypoint graph, default) or
        /// `navmesh` (polygon mesh + funnel). The navmesh backend requires the map's nav
        /// cache to be present (`generate-map-cache --map <m>`).
        #[arg(long, value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Name prefix override; bots are named `<name>_1`, `<name>_2`, … (1-based).
        /// Defaults to the config's `[fleet].name_prefix` (named `<prefix>0`, `<prefix>1`, …).
        #[arg(long)]
        name: Option<String>,
        /// Number of bots to spawn; overrides `[fleet].count`. Still clamped by
        /// `[fleet].max_bots` (server maxclients headroom).
        #[arg(long)]
        count: Option<usize>,
        /// Base qport for the fleet; bot *i* uses `<base>+i`. Defaults to a per-process
        /// value so two concurrent `run` fleets don't collide on the server's
        /// `(ip, qport)` client-slot key (which ignores UDP source port). Pin it for
        /// reproducible qports.
        #[arg(long)]
        qport_base: Option<u16>,
        /// Skin for every bot: `model/skin` (e.g. `male/grunt`) or a bare skin name
        /// resolved to its model (e.g. `sniper` → male/sniper, `cobalt` → female/cobalt).
        /// Mutually exclusive with the random-skin flags.
        #[arg(long, group = "skin_sel")]
        skin: Option<String>,
        /// Give each bot a random male skin.
        #[arg(long, group = "skin_sel")]
        skin_random_male: bool,
        /// Give each bot a random female skin.
        #[arg(long, group = "skin_sel")]
        skin_random_female: bool,
    },
    /// Print the loaded config (server + paths + fleet) and exit.
    Config,
    /// Query the server's connectionless `status` (map + player list). The fleet
    /// verification lens — confirms bots are connected and fragging (Plan 09).
    Status {
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
    },
    /// Load + dump a BSP (planes/nodes/leafs/brushes counts) from the configured baseq2.
    BspInfo { map: String },
    /// Build the collision model for a map and fire test rays from its center.
    Trace { map: String },
    /// Show PVS info for a map (cluster at the center + how many clusters it sees).
    Pvs { map: String },
    /// Generate the nav graph for a map and find a corner-to-corner path.
    Nav { map: String },
    /// Learn a nav graph by running a bot through the map and recording its path.
    Learn {
        /// Map to learn from.
        map: String,
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Output path for the learned nav graph.
        #[arg(long)]
        output: Option<String>,
    },
    /// Drive one bot from spawn to the farthest DM spawn point; log movement; stop.
    /// The measurement lens for movement quality (Plan 10).
    SpawnToSpawn {
        /// Map to load (defaults to q2dm1 / the server's map).
        #[arg(long)]
        map: Option<String>,
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Bot display name (appends _<timestamp> for multiple bots).
        #[arg(long)]
        name: Option<String>,
        /// Number of bots to spawn (default 1).
        #[arg(long, default_value = "1")]
        count: u8,
        /// Hard wall-clock cap per bot in seconds (default 30).
        #[arg(long, default_value = "30.0")]
        max_secs: f32,
        /// TODO(elevator-hack): extra A* cost on elevator ride edges so bots route
        /// around lifts (dodges the func_plat multi-bot deadlock). 0 = use lifts
        /// freely. Temporary until real wait/ride/step-off behaviour exists.
        #[arg(long, default_value = "5000")]
        lift_penalty: f32,
        /// Grid spacing (units) of the nav graph to use. Each spacing has its own cache
        /// dir (`data/mapcache/<spacing>/`); generate it first with `generate-map-cache
        /// --spacing <n>`. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-navmesh --map <m>` first.
        #[arg(long, value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
    },
    /// Drive one bot from spawn to a named weapon's BSP origin; log movement; stop.
    SpawnToWeapon {
        /// Weapon to reach, e.g. `rocketlauncher` (resolved as `weapon_<name>`).
        weapon_name: String,
        /// Map to load (defaults to q2dm1 / the server's map).
        #[arg(long)]
        map: Option<String>,
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Bot display name.
        #[arg(long)]
        name: Option<String>,
        /// Number of bots to spawn (default 1).
        #[arg(long, default_value = "1")]
        count: u8,
        /// Hard wall-clock cap per bot in seconds (default 30).
        #[arg(long, default_value = "30.0")]
        max_secs: f32,
        /// TODO(elevator-hack): extra A* cost on elevator ride edges so bots route
        /// around lifts (dodges the func_plat multi-bot deadlock). 0 = use lifts
        /// freely. Temporary until real wait/ride/step-off behaviour exists.
        #[arg(long, default_value = "5000")]
        lift_penalty: f32,
        /// Grid spacing (units) of the nav graph to use. Each spacing has its own cache
        /// dir (`data/mapcache/<spacing>/`); generate it first with `generate-map-cache
        /// --spacing <n>`. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-navmesh --map <m>` first.
        #[arg(long, value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
    },
    /// Diagnose disconnected nav-graph components: for each small component show
    /// the closest boundary-node pair to the main component, distances, and
    /// whether the direct hull trace / stair trace succeed. Run when
    /// generate-map-cache reports a connectivity bug so you can see exactly what
    /// geometry is blocking the connection.
    NavDebug {
        /// Map to analyse (e.g. `q2dm1`).
        map: String,
        /// Boundary pairs to show per minority component.
        #[arg(long, default_value = "8")]
        pairs: usize,
    },
    /// Pre-generate the nav graph cache for one or more maps.
    ///
    /// Run once per map (or after changing BSP or generation constants) so
    /// `qbots run` / `spawn-to-spawn` load from disk instead of regenerating.
    /// Supports a single map name (`q2dm1`) or a simple prefix glob (`q2dm*`).
    GenerateMapCache {
        /// Map name or glob (e.g. `q2dm1`, `q2dm*`). Required.
        #[arg(long)]
        map: String,
        /// Number of parallel map-generation workers (default: available CPU threads).
        #[arg(long)]
        jobs: Option<usize>,
        /// Output directory for `.qnav` cache files (default: `./data/mapcache`).
        #[arg(long, default_value = "data/mapcache")]
        out_dir: String,
        /// Grid spacing (units) to generate at. Each spacing caches into its own
        /// `<out_dir>/<spacing>/` subdir, so you can flip spacings without clobbering.
        #[arg(long, default_value = "24")]
        spacing: f32,
    },
}

/// A per-process default qport (distinct across concurrent bot processes).
pub(crate) fn default_qport() -> u16 {
    (std::process::id() & 0xFFFF) as u16
}

/// A per-process default qport **base** for a fleet, spaced 256 apart by PID low byte.
///
/// The fleet hands out `base + i` for bot `i`, so two fleets must not just differ — their
/// whole ranges must be disjoint, or they collide on the server's `(ip, qport)` client-slot
/// key. Two fleets launched back-to-back get *consecutive* PIDs, whose low bytes differ by
/// 1; shifting that into the high byte spaces their bases by 256 — disjoint for any sane
/// fleet size (≤256). Bases collide only when two PIDs share a low byte (differ by a
/// multiple of 256), which is improbable for concurrent launches; `--qport-base` pins it.
pub(crate) fn default_fleet_qport_base() -> u16 {
    ((std::process::id() & 0xFF) as u16) << 8
}

/// Squared 3D distance (for nearest-waypoint comparisons).
fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Custom elapsed time formatter for tracing (seconds.nanoseconds from startup).
/// Formats as NNNN.nnn (padded to 4 digits for seconds, 3 digits for milliseconds).
struct ElapsedFormatter(Instant);

impl tracing_subscriber::fmt::time::FormatTime for ElapsedFormatter {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let elapsed = self.0.elapsed();
        let secs = elapsed.as_secs();
        let millis = elapsed.subsec_millis();
        write!(w, "{secs:04}.{millis:03}")
    }
}

/// Abbreviate tracing level to single letter: T, D, I, W, E
fn abbreviate_level(level: tracing::Level) -> &'static str {
    match level {
        tracing::Level::TRACE => "T",
        tracing::Level::DEBUG => "D",
        tracing::Level::INFO => "I",
        tracing::Level::WARN => "W",
        tracing::Level::ERROR => "E",
    }
}

/// Max wall-time a run of identical lines is suppressed before we emit a
/// "repeated N times" coda and keep counting — so a stuck bot still shows life.
const DEDUP_FLUSH: Duration = Duration::from_secs(5);

/// What the deduper wants the formatter to do for one event.
#[derive(Debug, PartialEq)]
enum DedupAction {
    /// This event is a repeat — suppress it. If `coda` is `Some(n)`, first emit a
    /// "repeated n times" line (a periodic flush so a long run isn't silent).
    Suppress { coda: Option<u64> },
    /// Emit this event. If `prev_coda` is `Some(n)`, first close out the previous
    /// run with a "repeated n times" line.
    Emit { prev_coda: Option<u64> },
}

/// One run of consecutive identical log lines (`key` = level + fields, no
/// timestamp, so the changing clock doesn't defeat dedup).
#[derive(Clone)]
struct DedupRun {
    key: String,
    /// Suppressed repeats since the line was last emitted/flushed.
    count: u64,
    /// Elapsed at the run's start (or last periodic flush).
    first_seen: Duration,
}

/// Consecutive-duplicate suppressor: a pure state machine over `(key, elapsed)`
/// so it's unit-testable without a tracing subscriber. Collapses a stream like
/// `FSM transition check …` ×100 into one line + a "repeated N times" coda — the
/// syslog/journald "last message repeated" pattern.
#[derive(Default)]
struct Deduper {
    run: Option<DedupRun>,
}

impl Deduper {
    fn observe(&mut self, key: &str, elapsed: Duration, flush: Duration) -> DedupAction {
        if let Some(r) = self.run.as_mut() {
            if r.key == key {
                r.count += 1;
                if elapsed - r.first_seen >= flush {
                    let c = r.count;
                    r.count = 0;
                    r.first_seen = elapsed;
                    return DedupAction::Suppress { coda: Some(c) };
                }
                return DedupAction::Suppress { coda: None };
            }
            // Different line: close out the previous run, start a new one.
            let prev_coda = (r.count > 0).then_some(r.count);
            self.run = Some(DedupRun {
                key: key.to_string(),
                count: 0,
                first_seen: elapsed,
            });
            return DedupAction::Emit { prev_coda };
        }
        self.run = Some(DedupRun {
            key: key.to_string(),
            count: 0,
            first_seen: elapsed,
        });
        DedupAction::Emit { prev_coda: None }
    }
}

/// Write the "repeated N times" coda line (indented under the line it summarizes).
fn write_coda(
    writer: &mut tracing_subscriber::fmt::format::Writer<'_>,
    count: u64,
) -> std::fmt::Result {
    writeln!(writer, "  └─ repeated {count} times")
}

/// Custom event formatter: abbreviates levels to single letters and collapses
/// consecutive identical lines into one + a "repeated N times" coda. Kills the
/// per-tick FSM/nav spam without demoting every call site by hand.
#[derive(Clone)]
struct AbbreviatedFormat {
    start_time: Instant,
    dedup: Arc<Mutex<Deduper>>,
}

impl AbbreviatedFormat {
    fn new(start_time: Instant) -> Self {
        Self {
            start_time,
            dedup: Arc::new(Mutex::new(Deduper::default())),
        }
    }
}

impl<S, N> tracing_subscriber::fmt::format::FormatEvent<S, N> for AbbreviatedFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::format::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let elapsed = self.start_time.elapsed();
        let level = abbreviate_level(*event.metadata().level());

        // Format the event's fields into a buffer (the dedup key + the emit body).
        let mut fields = String::new();
        ctx.field_format().format_fields(
            tracing_subscriber::fmt::format::Writer::new(&mut fields),
            event,
        )?;
        let key = format!("{level} {fields}");

        // Hold the lock across decision + write so interleaved threads can't tear
        // a line or corrupt the run state.
        let mut dedup = self.dedup.lock().unwrap();
        match dedup.observe(&key, elapsed, DEDUP_FLUSH) {
            DedupAction::Suppress { coda: Some(n) } => write_coda(&mut writer, n),
            DedupAction::Suppress { coda: None } => Ok(()),
            DedupAction::Emit { prev_coda } => {
                if let Some(n) = prev_coda {
                    write_coda(&mut writer, n)?;
                }
                let secs = elapsed.as_secs();
                let millis = elapsed.subsec_millis();
                writeln!(writer, "{secs:04}.{millis:03} {level} {fields}")
            }
        }
    }
}

/// Send a connectionless `status` query and parse the reply (Plan 09). Times out
/// after 2 s — a down server or a dropped packet must not hang the CLI.
async fn query_status(addr: SocketAddr) -> std::io::Result<status::StatusReport> {
    use tokio::net::UdpSocket;
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    sock.send(b"\xff\xff\xff\xffstatus\n").await?;
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(2), sock.recv(&mut buf))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "status query timed out")
        })??;
    status::parse_status_response(&buf[..n]).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unparseable status response",
        )
    })
}

/// Resolve `host[:port]` to a socket address via DNS lookup. Hostnames (e.g.
/// `noir.lan`), `IP:port`, and bare IPs (defaulting port to 27910) all work.
async fn resolve_addr(addr: &str) -> Result<SocketAddr, String> {
    let target = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:27910")
    };
    // Pass `target` by value so the lookup future owns it (avoids a borrow that would
    // otherwise be extended across the await).
    match tokio::net::lookup_host(target).await {
        Ok(mut it) => it
            .next()
            .ok_or_else(|| format!("no addresses found for '{addr}'")),
        Err(e) => Err(format!("can't resolve '{addr}': {e}")),
    }
}

/// Wrapper that adds signal handling for graceful shutdown.
/// Sends a disconnect packet before teardown when SIGINT/SIGTERM received.
/// One bot's connection → frames → brain loop. Shares the nav graph via
/// `nav_cache` (built once per map across the whole fleet) and exits when
/// `shutdown` is requested or the connection drops.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn bot_task(
    addr: SocketAddr,
    name: &str,
    qport: u16,
    skin: Option<&str>,
    cfg: &Config,
    nav_cache: &supervisor::NavCache,
    shutdown: &supervisor::Shutdown,
    stats: &supervisor::FleetStats,
    mode: NavMode,
) -> std::io::Result<()> {
    use brain::fsm::{BehaviorIntent, BehaviorState};
    use brain::nav::NavGoal;
    use brain::perception::Worldview;
    use brain::steer::{move_from_world_dir, Steering};
    use brain::{
        BotSkill, CombatDriver, DangerDriver, MovementController, MovementIntent, Navigator,
        Recovery, RecoveryAction,
    };
    use client::{Conn, ConnState};
    use q2proto::Usercmd;
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time;

    // Attribute every event in this task to the bot name so fleet logs are
    // per-bot filterable (Plan 09 T3).
    let span = tracing::info_span!("bot", %name, qport);
    let _enter = span.enter();

    // Register this bot with the fleet tally so it appears in the report even
    // if it never frags/dies (Plan 09 observability).
    stats.register(name);

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);
    if let Some(s) = skin {
        // Userinfo skin is sent in the `connect` handshake, so set it before `start`.
        conn.userinfo.set("skin", s);
        tracing::info!(skin = s, "using skin");
    }

    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }

    let mut buf = vec![0u8; 4096];
    let mut ticker = time::interval(Duration::from_millis(100));
    let mut ticks: u32 = 0;

    let mut fsm = BehaviorState::Roam;
    let mut combat = CombatDriver::new();
    let danger = DangerDriver::new();
    let mut move_ctrl = MovementController::new();
    let mut skill = BotSkill::default();
    let mut steering = Steering::new(skill.combat());
    // Boxed behind the `Navigator` trait so the tick loop is backend-agnostic: `--mode`
    // picks A* (waypoint graph) or navmesh (polygons + funnel) at map load. `+ Send`
    // because this future is spawned on tokio and holds the driver across awaits.
    let mut nav_driver: Option<Box<dyn Navigator + Send>> = None;
    // Roam goals are node indices into the A* graph; the navmesh backend ignores
    // `NavGoal::Waypoint`, so a navmesh bot resolves them to world positions via this
    // graph handle. Set alongside `nav_driver` at map load.
    let mut nav_graph: Option<Arc<world::NavGraph>> = None;
    // Collision model the nav graph was built from — for LOS gating (Plan 11) and
    // reactive wall probes (Plan 13). Set when the map loads.
    let mut collision: Option<Arc<world::CollisionModel>> = None;
    let mut roam_nodes: Vec<usize> = Vec::new();
    let mut roam_idx: usize = 0;
    let mut map_loaded = false;
    let mut last_serverframe: Option<i32> = None;
    let mut recovery = Recovery::new();
    let mut last_health: Option<i32> = None; // Track health across frames for damage detection
    let mut last_frags: Option<i32> = None; // Track frags for kill detection

    // Plan 08: per-bot danger/popularity heatmap observer + the origin we were
    // at last time we were alive (death attribution, before the respawn teleport).
    let mut heatmap_obs: Option<brain::HeatmapObserver> = None;
    let mut last_alive_pos: Option<Vec3> = None;

    loop {
        if shutdown.requested() {
            if conn.state() == ConnState::Active {
                if let Some(pkt) = conn.disconnect() {
                    let _ = sock.send(&pkt).await;
                    let _ = sock.send(&pkt).await;
                    let _ = sock.send(&pkt).await;
                }
                time::sleep(Duration::from_millis(100)).await;
            }
            return Ok(());
        }

        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    tracing::info!("disconnected");
                    return Ok(());
                }
            }

            _ = ticker.tick() => {
                ticks = ticks.wrapping_add(1);

                let (frame_opt, cs) = (conn.frame.clone(), conn.configstrings().clone());
                let state = conn.state();
                let playernum = conn.serverdata.as_ref().map(|sd| sd.playernum).unwrap_or(0);

                // Track health across frames for damage detection
                if let Some(ref frame) = frame_opt {
                    let view = Worldview::from_frame(frame, &cs, playernum);
                    let current_health = view.self_state().health;
                    if current_health > 0 {
                        if let Some(prev) = last_health {
                            if prev > 0 && current_health < prev {
                                let damage = prev - current_health;
                                tracing::info!(
                                    health_before = prev,
                                    health_after = current_health,
                                    damage = damage,
                                    "being hit"
                                );
                                if current_health <= 0 {
                                    tracing::error!(health = 0, "bot death detected");
                                }
                            } else if current_health > prev && prev > 0 {
                                let healed = current_health - prev;
                                tracing::debug!(
                                    health_before = prev,
                                    health_after = current_health,
                                    healed = healed,
                                    "health restored"
                                );
                            }
                        }
                        last_health = Some(current_health);
                    }
                }

                if !map_loaded && state == ConnState::Active {
                    if let Some(bsp_path) = cs.get(33) {
                        if !bsp_path.is_empty() {
                            let bsp_path = bsp_path.to_owned();
                            let map = bsp_path
                                .strip_prefix("maps/")
                                .unwrap_or(&bsp_path)
                                .strip_suffix(".bsp")
                                .unwrap_or(&bsp_path)
                                .to_owned();
                            map_loaded = true;
                            tracing::info!(map, bsp = %bsp_path, "loading nav graph");
                            // Shared across the fleet: built once per map, reused as Arc.
                            if let Some(map_nav) = nav_cache.get_or_build(cfg, &map) {
                                roam_nodes = map_nav.roam_nodes.clone();
                                nav_graph = Some(Arc::clone(&map_nav.graph));
                                nav_driver = Some(build_navigator(
                                    mode,
                                    Arc::clone(&map_nav.graph),
                                    || {
                                        supervisor::get_or_build_navmesh(
                                            &map,
                                            &map_nav.cm,
                                            map_nav.bounds,
                                        )
                                    },
                                ));
                                collision = Some(Arc::clone(&map_nav.cm));
                                heatmap_obs = Some(brain::HeatmapObserver::new(
                                    Arc::clone(&map_nav.graph),
                                    name,
                                ));
                            }
                        }
                    }
                }

                let cmd = if state == ConnState::Active {
                    if let Some(frame) = frame_opt {
                        let view = Worldview::from_frame(&frame, &cs, playernum);

                        // Detect damage before creating worldview (need previous health)
                        let current_health = view.self_state().health;
                        if let Some(prev) = last_health.as_mut() {
                            if *prev > 0 && current_health < *prev {
                                let damage = *prev - current_health;
                                tracing::info!(
                                    health_before = *prev,
                                    health_after = current_health,
                                    damage = damage,
                                    "being hit"
                                );
                                // Plan 08: we're under fire here — mark the node dangerous.
                                if let Some(obs) = heatmap_obs.as_mut() {
                                    obs.on_self_damage(view.self_state().origin);
                                }
                                if current_health <= 0 {
                                    tracing::error!(health = 0, "bot death detected");
                                    // Respawn resets us to the spawn loadout (Blaster)
                                    // and the server reseeds delta_angles; let the
                                    // next frame's playerstate re-feed both.
                                    combat.on_respawn();
                                    // Eraser auto-skill: ease down after a death.
                                    skill.on_death();
                                    stats.record_death(name);
                                    // Plan 08: record where we died (highest-confidence
                                    // danger) and force a replan so the new path avoids it.
                                    let death_pos = last_alive_pos
                                        .unwrap_or(view.self_state().origin);
                                    if let Some(obs) = heatmap_obs.as_mut() {
                                        obs.on_self_death(death_pos);
                                    }
                                    if let Some(nav) = nav_driver.as_mut() {
                                        nav.force_replan();
                                    }
                                }
                            } else if current_health > *prev && *prev > 0 {
                                let healed = current_health - *prev;
                                tracing::debug!(
                                    health_before = *prev,
                                    health_after = current_health,
                                    healed = healed,
                                    "health restored"
                                );
                            }
                        }
                        last_health = Some(current_health);
                        if current_health > 0 {
                            last_alive_pos = Some(view.self_state().origin);
                        }

                        // Detect frags via STAT_FRAGS (server increments on kill).
                        let current_frags = view.self_state().frags;
                        if let Some(prev) = last_frags {
                            if current_frags > prev {
                                tracing::info!(frags = current_frags, gained = current_frags - prev, "*** FRAG ***");
                                skill.on_kill();
                                stats.record_kill(name);
                            }
                        }
                        last_frags = Some(current_frags);

                        // Plan 08 heatmap: observe this frame (presence + obituary
                        // prints), advance decay, and refresh the risk overlay the
                        // nav driver consumes when it next plans a goal. This is the
                        // strategic layer; the tactical projectile dodge (below)
                        // composes by overriding movement for a single frame.
                        if let Some(obs) = heatmap_obs.as_mut() {
                            const HEATMAP_DT: f32 = 0.1; // 10 Hz client tick
                            obs.tick(HEATMAP_DT);
                            obs.sample_presence(&view, &cs, HEATMAP_DT, frame.serverframe);
                            for text in conn.drain_prints() {
                                obs.on_print(&text, name, frame.serverframe);
                            }
                            let (w_danger, w_pop) = skill.heatmap_weights();
                            let overlay = obs.cost_overlay(w_danger, w_pop);
                            if let Some(nav) = nav_driver.as_mut() {
                                nav.set_risk_overlay(overlay);
                            }
                            // Periodic "danger map" snapshot at debug level (T4).
                            if ticks.is_multiple_of(50) {
                                let snap = obs.snapshot(4);
                                if snap.total_danger > 0.0 {
                                    tracing::debug!(
                                        total_danger = snap.total_danger,
                                        max_danger = snap.max_danger,
                                        hot = ?snap.hot_nodes,
                                        "heatmap overlay"
                                    );
                                }
                            }
                        }

                        // Feed the server's delta_angles into the movement controller so
                        // build_cmd can subtract it — without this, every aim/move direction
                        // is rotated by the persistent spawn-yaw offset. (pmove.c:1255)
                        move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);

                        // Health tracking is done above, before creating the view
                        // No need to call view.detect_damage() here
                        let jitter = (ticks as f32) * 0.1;
                        let combat_dec =
                            combat.evaluate(&view, &skill, jitter, collision.as_deref());

                        // Pass combat target to FSM for navigation goal.
                        // Only chase via nav when LOS holds (Plan 11 T4) — without
                        // LOS the bot was walking into walls toward walled enemies.
                        let fsm_intent = if let Some(target) = combat_dec.target_entity {
                            let target_entity = view.entities()
                                .find(|e| e.entity_number == target);
                            let target_pos = target_entity
                                .map(|e| e.origin)
                                .unwrap_or(view.self_state().origin);

                            // LOS check: only set Entity nav goal when the path is clear.
                            let has_los = target_entity
                                .and_then(|te| collision.as_deref().map(|cm| {
                                    let eye = brain::los::eye_origin(view.self_state().origin.into());
                                    brain::los::has_los_player(cm, eye, te.origin.into())
                                }))
                                .unwrap_or(true); // no cm yet → optimistic (old behavior)

                            if has_los {
                                if !matches!(fsm, BehaviorState::Engage { .. }) {
                                    tracing::debug!("forcing FSM into Engage (target={})", target);
                                    fsm = BehaviorState::Engage { target_entity: target };
                                }
                                tracing::trace!("combat target override: target={} pos={:?}", target, target_pos);
                                BehaviorIntent {
                                    nav_goal: Some(NavGoal::Entity(target_pos)),
                                    should_pickup: None,
                                }
                            } else {
                                // Target exists (grace-period fire still possible) but
                                // no clear path → let FSM navigate (Hunt last-known pos).
                                fsm.tick(&view, collision.as_deref())
                            }
                        } else {
                            fsm.tick(&view, collision.as_deref())
                        };

                        let mut mv = MovementIntent::new();

                        if combat_dec.should_fire {
                            mv.attack();
                        }

                        let pos = view.self_state().origin;

                        // Measured frame delta for turn-rate limiting (Open Q1, Plan 12).
                        let current_sf = frame.serverframe;
                        let dt = if let Some(prev_sf) = last_serverframe {
                            let sf_delta = (current_sf - prev_sf).max(0) as f32;
                            (sf_delta * 0.1).clamp(0.02, 0.3)
                        } else {
                            0.1
                        };
                        last_serverframe = Some(current_sf);

                        if let Some(nav) = nav_driver.as_mut() {
                            nav.update(pos, None);

                            // Give-up watchdog: if we've chased this goal too long
                            // without reaching a waypoint, abandon the current
                            // combat target so we stop re-issuing the same stale
                            // position and fall back to roaming.
                            if nav.goal_abandoned() {
                                combat.clear_target();
                                fsm = BehaviorState::Roam;
                            }

                            let goal = if let Some(g) = fsm_intent.nav_goal {
                                g
                            } else if let Some((item_pos, _)) =
                                brain::items::best_item_goal(&view, &skill)
                            {
                                // Seek the highest-value visible item (powerups,
                                // armor, weapons) weighted by value/distance and
                                // the bot's health need / quad_freak personality.
                                NavGoal::Position(item_pos)
                            } else if !roam_nodes.is_empty() {
                                // Campers dwell ~5x longer per node (first-cut
                                // camping; a true camp-node picker with cover/LOS
                                // is a follow-up). Default roamer cycles every 5s.
                                let dwell = if skill.camper { 250 } else { 50 };
                                if ticks.is_multiple_of(dwell) {
                                    roam_idx = (roam_idx + roam_nodes.len() / 7 + 1)
                                        % roam_nodes.len();
                                }
                                let node = roam_nodes[roam_idx];
                                // The navmesh backend doesn't index the A* graph's nodes, so
                                // express the roam target as a world position it can path to.
                                match (mode, nav_graph.as_deref()) {
                                    (NavMode::Navmesh, Some(g)) => {
                                        NavGoal::Position(Vec3::from(g.node_pos(node)))
                                    }
                                    _ => NavGoal::Waypoint(node),
                                }
                            } else {
                                NavGoal::Position(pos)
                            };

                            nav.set_goal(goal, pos);
                            // String-pull the path into longer straight runs (Plan 14 T1).
                            if let Some(cm) = collision.as_deref() {
                                nav.smooth_with_cm(cm, pos);
                            }

                            // Ideal-distance combat constants (Eraser BOT_IDEAL_DIST_FROM_ENEMY).
                            const IDEAL_DIST: f32 = 160.0;
                            const BACKUP_DIST: f32 = 80.0;

                            // Resolve enemy position + distance (if we have a target in view).
                            let enemy_dist_dir: Option<(f32, Vec3)> =
                                combat_dec.target_entity.and_then(|t| {
                                    view.entities()
                                        .find(|e| e.entity_number == t)
                                        .map(|enemy| {
                                            let to = enemy.origin - pos;
                                            let d = to.length();
                                            let dir = if d > 1.0 { to / d } else { Vec3::X };
                                            (d, dir)
                                        })
                                });

                            // ── 1. Ideal view yaw (priority: fire-aim > enemy-face > path) ──
                            let (ideal_yaw, ideal_pitch) = if combat_dec.should_fire {
                                (combat_dec.aim_yaw, combat_dec.aim_pitch)
                            } else if let Some((d, dir)) = enemy_dist_dir {
                                if d < IDEAL_DIST {
                                    // Face enemy while in ideal-distance range.
                                    let yaw = dir.y.atan2(dir.x).to_degrees();
                                    (yaw, 0.0)
                                } else {
                                    // Far from enemy — steer along the path toward them.
                                    nav.pursue_target(pos)
                                        .filter(|pt| (pt - pos).length_squared() > 1.0)
                                        .map(|pt| {
                                            let d = pt - pos;
                                            (d.y.atan2(d.x).to_degrees(), 0.0)
                                        })
                                        .unwrap_or((steering.view_yaw(), 0.0))
                                }
                            } else {
                                // No combat: steer along the path.
                                nav.pursue_target(pos)
                                    .filter(|pt| (pt - pos).length_squared() > 1.0)
                                    .map(|pt| {
                                        let d = pt - pos;
                                        (d.y.atan2(d.x).to_degrees(), 0.0)
                                    })
                                    .unwrap_or((steering.view_yaw(), 0.0))
                            };

                            // ── 2. Rate-limit the yaw turn toward ideal ───────────────────
                            let view_yaw = steering.change_yaw(ideal_yaw, dt);
                            mv.look_at(view_yaw, ideal_pitch);

                            // ── 3. World move direction + face-then-go mode ───────────────
                            // T5 circle-strafe: when Engage + LOS holds, separate aim (view_yaw →
                            // enemy) from walk (radial ± tangential). Eraser: combat 1 = no strafe.
                            let is_engage_los = combat_dec.should_fire
                                || matches!(fsm, BehaviorState::Engage { .. });
                            let strafe_weight =
                                if is_engage_los && skill.combat() > 1.5 { 0.7 } else { 0.0 };

                            let (world_move_dir, face_then_go) =
                                if let Some((d, dir)) = enemy_dist_dir {
                                    if d < BACKUP_DIST {
                                        // Back away from enemy while keeping aim on them.
                                        let away =
                                            Vec3::new(-dir.x, -dir.y, 0.0).normalize_or_zero();
                                        // Add tangential even while backing (keeps bot moving).
                                        let tan = Vec3::new(-dir.y, dir.x, 0.0)
                                            * steering.strafe_tick(dt)
                                            * strafe_weight;
                                        ((away + tan).normalize_or_zero(), false)
                                    } else if d < IDEAL_DIST {
                                        // Hold ideal distance — pure circle-strafe tangentially.
                                        let tan = Vec3::new(-dir.y, dir.x, 0.0)
                                            * steering.strafe_tick(dt);
                                        (tan.normalize_or_zero() * strafe_weight, false)
                                    } else {
                                        // Chase via nav look-ahead + light tangential strafe.
                                        let nav_dir = nav
                                            .pursue_target(pos)
                                            .map(|pt| {
                                                let d = pt - pos;
                                                Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
                                            })
                                            .unwrap_or(Vec3::ZERO);
                                        if strafe_weight > 0.0 {
                                            let tan = Vec3::new(-dir.y, dir.x, 0.0)
                                                * steering.strafe_tick(dt)
                                                * strafe_weight;
                                            ((nav_dir + tan).normalize_or_zero(), false)
                                        } else {
                                            (nav_dir, true)
                                        }
                                    }
                                } else {
                                    // Roaming: follow path look-ahead.
                                    let dir = nav
                                        .pursue_target(pos)
                                        .map(|pt| {
                                            let d = pt - pos;
                                            Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
                                        })
                                        .unwrap_or(Vec3::ZERO);
                                    (dir, true)
                                };

                            // ── 4. Arrive throttle (slows near final goal) ────────────────
                            let arrive = nav
                                .pursue_target(pos)
                                .map(|pt| brain::steer::Steering::arrive_scale((pt - pos).length()))
                                .unwrap_or(1.0);

                            // ── 5. Decompose into view-relative (forward, side) ───────────
                            let (fwd, side) =
                                move_from_world_dir(world_move_dir, view_yaw, face_then_go);
                            mv.move_forward(fwd * arrive);
                            mv.move_side(side * arrive);

                            // ── 6. Stuck recovery (Plan 13) ───────────────────────────────
                            let has_nav_target = nav.pursue_target(pos).is_some();
                            let engaging = matches!(fsm, BehaviorState::Engage { .. });
                            let rec_action = recovery.evaluate(
                                pos, dt,
                                collision.as_deref(),
                                view_yaw,
                                has_nav_target,
                                engaging,
                            );
                            match rec_action {
                                RecoveryAction::None => {}
                                RecoveryAction::Jump => {
                                    tracing::debug!(?pos, "stuck — jump");
                                    mv.jump();
                                }
                                RecoveryAction::Strafe { dir } => {
                                    tracing::debug!(?pos, dir, "stuck — strafe");
                                    mv.move_side(dir);
                                }
                                RecoveryAction::BackOffThenRepath => {
                                    tracing::debug!(?pos, "stuck — back off + repath");
                                    mv.move_forward(-0.5);
                                    nav.force_replan();
                                }
                                RecoveryAction::UseHeading(yaw) => {
                                    tracing::debug!(?pos, yaw, "no nav — steer free heading");
                                    let r = yaw.to_radians();
                                    let free_dir = Vec3::new(r.cos(), r.sin(), 0.0);
                                    let (hfwd, hside) =
                                        move_from_world_dir(free_dir, view_yaw, true);
                                    mv.move_forward(hfwd);
                                    mv.move_side(hside);
                                }
                            }

                            // ── 7. Jump-edge activation (Plan 14 T2) ─────────────────────
                            if nav.current_edge_is_jump() {
                                mv.jump();
                            }
                        } else if !combat_dec.should_fire {
                            // No nav graph loaded yet — just walk forward.
                            mv.move_forward(1.0);
                            if ticks.is_multiple_of(20) {
                                mv.jump();
                            }
                        }

                        // Request a weapon switch via `use <name>` stringcmd (Q2
                        // ignores impulse). Queued as a reliable message; flushed
                        // on the next transmit_cmd below.
                        if let Some(req) = combat_dec.weapon_request {
                            conn.queue_stringcmd(&format!("use {}", req.0.name()));
                        }

                        // Tactical override: dodge an incoming projectile. This is
                        // frame-scale and takes precedence over nav/engage intent.
                        // The dodge direction (world space) is projected onto the
                        // bot's right vector → a view-relative `side` strafe so we
                        // keep facing the target while stepping off the line.
                        let dodge = danger.evaluate(&view, skill.combat());
                        if dodge.is_active() {
                            tracing::debug!(?dodge.strafe_dir, jump = dodge.jump, "dodging projectile");
                            let yaw_rad = mv.yaw.to_radians();
                            let right = Vec3::new(yaw_rad.sin(), -yaw_rad.cos(), 0.0);
                            mv.side = dodge.strafe_dir.dot(right).clamp(-1.0, 1.0);
                            mv.forward = 0.0;
                            if dodge.jump {
                                mv.jump();
                            }
                        }

                        move_ctrl.set_msec(dt);
                        move_ctrl.build_cmd(mv)
                    } else {
                        Usercmd::default()
                    }
                } else {
                    Usercmd::default()
                };

                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                    let _ = sock.send(&pkt).await;
                }

                if ticks.is_multiple_of(10) {
                    match conn.frame.as_ref() {
                        Some(f) => {
                            let o = f.playerstate.pmove.origin_f32();

                            tracing::debug!(
                                state = ?conn.state(),
                                frame = f.serverframe,
                                ents = f.entities.len(),
                                "origin=({:.1},{:.1},{:.1}) fsm={:?}",
                                o[0], o[1], o[2],
                                fsm
                            );
                        }
                        None => tracing::debug!(state = ?conn.state(), "(no frame yet)"),
                    }
                }
            }
        }
    }
}

/// Shared CLI plumbing for the two movement scenarios (Plan 10): resolve the
/// server address + bot name, then hand off to [`scenario::run_scenario`] and map
/// its result to a process exit code.
#[allow(clippy::too_many_arguments)]
async fn run_scenario_cmd(
    cfg: &Config,
    addr: Option<String>,
    name: Option<String>,
    map: Option<String>,
    goal: scenario::ScenarioGoal,
    count: u8,
    max_secs: f32,
    lift_penalty: f32,
    spacing: f32,
    mode: NavMode,
) -> ExitCode {
    let base_name = name.unwrap_or_else(|| "qbots".to_string());
    let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
    let addr = match resolve_addr(&addr_str).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let unix_ts = time::OffsetDateTime::now_utc().unix_timestamp();
    // (bot_name, join_handle) pairs for per-bot summary (T5).
    let mut handles: Vec<(String, tokio::task::JoinHandle<ExitCode>)> = Vec::new();
    let map_clone = map.clone();
    let goal_clone = goal.clone();

    for i in 0..count {
        let bot_name = if count > 1 {
            format!("{}_{}", base_name, unix_ts + i as i64)
        } else {
            base_name.clone()
        };

        tracing::info!("spawning bot {}/{}: {}", i + 1, count, bot_name);

        let cfg = cfg.clone();
        let map = map_clone.clone();
        let goal = goal_clone.clone();
        let bot_name_task = bot_name.clone();

        // Stagger spawns by 500ms when count > 1
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        let handle = tokio::task::spawn(async move {
            let bot_qport = crate::default_qport() + i as u16;
            match scenario::run_scenario(
                &cfg,
                addr,
                &bot_name_task,
                map.as_deref(),
                goal,
                max_secs,
                bot_qport,
                lift_penalty,
                spacing,
                mode,
            )
            .await
            {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!("scenario for {}: {e}", bot_name_task);
                    ExitCode::FAILURE
                }
            }
        });
        handles.push((bot_name, handle));
    }

    // Wait for all bots; emit per-bot result lines then an aggregate summary (T5).
    let total = handles.len();
    let mut reached = 0usize;
    let mut result = ExitCode::SUCCESS;
    for (name, handle) in handles {
        let code = match handle.await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("task join error for {name}: {e}");
                ExitCode::FAILURE
            }
        };
        let ok = code == ExitCode::SUCCESS;
        tracing::info!(bot = %name, reached = ok, "scenario result");
        if ok {
            reached += 1;
        } else if result == ExitCode::SUCCESS {
            result = code;
        }
    }
    tracing::info!("{reached}/{total} bots reached the goal");
    result
}

/// Enumerate all available map names under `baseq2`: loose `.bsp` files in
/// `<baseq2>/maps/` and entries matching `maps/*.bsp` in `pak0`–`pak9`.
/// Returns deduplicated, sorted map names (no extension, no `maps/` prefix).
fn enumerate_maps(baseq2: &std::path::Path) -> Vec<String> {
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Loose files.
    let maps_dir = baseq2.join("maps");
    if let Ok(rd) = std::fs::read_dir(&maps_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("bsp") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_ascii_lowercase());
                }
            }
        }
    }

    // Pak files.
    for n in 0..10u8 {
        let pak_path = baseq2.join(format!("pak{n}.pak"));
        if let Ok(pak) = world::Pak::open(&pak_path) {
            for name in pak.names() {
                let low = name.to_ascii_lowercase();
                if let Some(rest) = low.strip_prefix("maps/") {
                    if let Some(stem) = rest.strip_suffix(".bsp") {
                        names.insert(stem.to_string());
                    }
                }
            }
        }
    }

    let mut v: Vec<String> = names.into_iter().collect();
    v.sort();
    v
}

/// Match a map name against a simple `*`-only glob (e.g. `q2dm*` matches `q2dm1`).
fn glob_matches(pattern: &str, name: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        name.ends_with(suffix)
    } else {
        pattern == name
    }
}

/// `nav-debug` — generate the real nav graph (GRID_SPACING=24) and report exactly
/// WHY each minority component is disconnected from the main component. For each
/// boundary node pair it shows: distance, dz, whether the direct hull trace and
/// the stair trace succeed, and a one-line diagnosis. Run this when
/// `generate-map-cache` reports a connectivity bug.
fn nav_debug(cfg: &Config, map: &str, pairs: usize) -> ExitCode {
    use std::collections::HashSet;

    let built = match world::generate_map_nav(
        &cfg.paths.baseq2,
        map,
        world::ELEVATOR_PENALTY,
        world::GRID_SPACING,
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let comps = built.graph.components();
    tracing::info!(
        map,
        nodes = built.graph.node_count(),
        edges = built.graph.edge_count(),
        components = comps.len(),
        in_largest = built.in_largest,
        total_spawns = built.total_spawns,
        "nav-debug"
    );
    for (i, c) in comps.iter().enumerate().take(16) {
        // Bounding box of the component — reveals whether it's "the whole map"
        // or a small pocket/roof. (The boundary-pair analysis below uses nearest
        // euclidean node, which is misleading for vertically-stacked floors.)
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for &n in c {
            let p = built.graph.nodes[n];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        tracing::info!(
            "  component[{i}]: {} nodes  x[{:.0}..{:.0}] y[{:.0}..{:.0}] z[{:.0}..{:.0}]",
            c.len(),
            lo[0],
            hi[0],
            lo[1],
            hi[1],
            lo[2],
            hi[2],
        );
    }
    if comps.len() == 1 {
        tracing::info!("fully connected — nothing to debug");
        return ExitCode::SUCCESS;
    }

    let main_set: HashSet<usize> = comps[0].iter().copied().collect();
    let nodes = &built.graph.nodes;
    let cm = &built.cm;
    let grid = world::GRID_SPACING;

    // node → component index map, so the boundary scan can find the nearest node
    // in ANY OTHER component (the play-area fragments must merge with each other,
    // not with the roof at comp[0]).
    let mut node_comp = vec![usize::MAX; nodes.len()];
    for (ci, c) in comps.iter().enumerate() {
        for &n in c {
            node_comp[n] = ci;
        }
    }

    // Which components contain a spawn — those are the ones that MUST merge.
    let mut spawn_comps: HashSet<usize> = HashSet::new();
    for sp in &built.spawn_origins {
        if let Some(n) = built.graph.nearest(sp) {
            spawn_comps.insert(node_comp[n]);
        }
    }
    let mut spawn_comp_list: Vec<usize> = spawn_comps.iter().copied().collect();
    spawn_comp_list.sort_unstable();
    tracing::info!("spawn-bearing components: {:?}", spawn_comp_list);

    // Analyze each spawn-bearing component: find its node closest to a node in a
    // DIFFERENT component, and report why no edge bridges them.
    for &ci in &spawn_comp_list {
        let comp = &comps[ci];
        tracing::info!(
            "=== component[{ci}] ({} nodes) — cross-component boundary ===",
            comp.len()
        );

        // For each node in this comp, find the nearest node in any other component.
        let mut boundary: Vec<(usize, usize, f32)> = comp
            .iter()
            .filter_map(|&node| {
                let pos = nodes[node];
                nodes
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| node_comp[*j] != ci)
                    .map(|(j, mp)| {
                        let d2 = (pos[0] - mp[0]).powi(2)
                            + (pos[1] - mp[1]).powi(2)
                            + (pos[2] - mp[2]).powi(2);
                        (node, j, d2)
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
            })
            .collect();
        boundary.sort_by(|a, b| a.2.total_cmp(&b.2));
        boundary.dedup_by_key(|x| x.0);

        for (node, main_node, d2) in boundary.iter().take(pairs) {
            let pos = nodes[*node];
            let mpos = nodes[*main_node];
            let other_ci = node_comp[*main_node];
            let d3 = d2.sqrt();
            let dz = (pos[2] - mpos[2]).abs();
            let dh = ((pos[0] - mpos[0]).powi(2) + (pos[1] - mpos[1]).powi(2)).sqrt();

            // Is this pair grid-adjacent? (within one diagonal grid step + 10%)
            let grid_adj = dh <= grid * 2.0_f32.sqrt() * 1.1;

            // Why would the edge-builder have skipped this pair?
            let skip = if !grid_adj {
                format!(
                    "NOT-GRID-ADJACENT (dh={dh:.0}>{:.0})",
                    grid * 2.0_f32.sqrt()
                )
            } else if dz > world::STAIR_MAX {
                format!("dz={dz:.0} > STAIR_MAX={}", world::STAIR_MAX)
            } else {
                "in-range — trace determines edge".to_string()
            };

            // Direct hull trace both directions.
            let t_fwd = cm.trace(
                &pos,
                &mpos,
                &world::HULL_MINS,
                &world::HULL_MAXS,
                world::MASK_SOLID,
            );
            let t_rev = cm.trace(
                &mpos,
                &pos,
                &world::HULL_MINS,
                &world::HULL_MAXS,
                world::MASK_SOLID,
            );
            let direct = if !t_fwd.startsolid && t_fwd.fraction >= 1.0 {
                "CLEAR"
            } else if !t_rev.startsolid && t_rev.fraction >= 1.0 {
                "CLEAR(rev)"
            } else {
                "BLOCKED"
            };

            // Stair trace (only meaningful when dz in (STEP, STAIR_MAX]).
            let stair = if dz > world::STEP && dz <= world::STAIR_MAX {
                let (lo, hi) = if pos[2] < mpos[2] {
                    (pos, mpos)
                } else {
                    (mpos, pos)
                };
                if world::walkable_stair(cm, lo, hi) {
                    "stair=OK"
                } else {
                    "stair=FAIL"
                }
            } else if dz <= world::STEP {
                "flat-trace-only"
            } else {
                "dz-exceeds-STAIR_MAX"
            };

            // Point trace (ignores hull — useful to see if LoS exists at all).
            let zero = [0.0f32; 3];
            let tp = cm.trace(&pos, &mpos, &zero, &zero, world::MASK_SOLID);
            let point = if !tp.startsolid && tp.fraction >= 1.0 {
                "pt-CLEAR"
            } else {
                "pt-blocked"
            };

            tracing::info!(
                "  C{ci}:{node}({:.0},{:.0},{:.0}) <-> C{other_ci}:{main_node}({:.0},{:.0},{:.0})\
                 \n    d3={d3:.0} dh={dh:.0} dz={dz:.0}  adj={grid_adj}\
                 \n    skip=[{skip}]\
                 \n    hull={direct} {point} {stair}",
                pos[0],
                pos[1],
                pos[2],
                mpos[0],
                mpos[1],
                mpos[2],
            );
        }
    }

    // Also show spawn → nearest-main-node summary.
    tracing::info!("=== spawn reachability ===");
    for (i, sp) in built.spawn_origins.iter().enumerate() {
        let nearest = built.graph.nearest(sp);
        let in_main = nearest.is_some_and(|n| main_set.contains(&n));
        let comp_idx = nearest
            .and_then(|n| comps.iter().position(|c| c.contains(&n)))
            .unwrap_or(999);
        tracing::info!(
            "  spawn[{i}] ({:.0},{:.0},{:.0}) node={:?} comp={comp_idx} {}",
            sp[0],
            sp[1],
            sp[2],
            nearest,
            if in_main { "[ok]" } else { "[BUG]" }
        );
    }

    if built.in_largest < built.total_spawns {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `generate-map-cache` handler (Plan 18 T3). Synchronous — nav graph generation
/// is CPU-bound so we run it on plain threads, not tokio tasks.
fn generate_map_cache(
    cfg: &Config,
    map_arg: &str,
    jobs: Option<usize>,
    out_dir: &str,
    spacing: f32,
) -> ExitCode {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Per-spacing subdir, matching cached_map_nav's load path.
    let out_path = std::path::Path::new(out_dir).join(world::spacing_subdir(spacing));
    let out_path = out_path.as_path();

    // Resolve the list of maps to generate.
    let maps: Vec<String> = if map_arg.contains('*') {
        let all = enumerate_maps(&cfg.paths.baseq2);
        let matched: Vec<String> = all
            .into_iter()
            .filter(|n| glob_matches(map_arg, n))
            .collect();
        if matched.is_empty() {
            tracing::error!(
                pattern = map_arg,
                "no maps matched the glob pattern; check --map and baseq2 path"
            );
            return ExitCode::FAILURE;
        }
        matched
    } else {
        vec![map_arg.to_string()]
    };

    let n_jobs = jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    tracing::info!(
        maps = maps.len(),
        jobs = n_jobs,
        out_dir,
        "generating map cache"
    );

    let succeeded = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    // Parallel over maps using a thread pool sized by --jobs.
    std::thread::scope(|scope| {
        // Simple bounded pool: chunk the map list into `n_jobs` slices.
        let chunks: Vec<Vec<String>> = {
            let mut cs: Vec<Vec<String>> = vec![Vec::new(); n_jobs];
            for (i, m) in maps.iter().enumerate() {
                cs[i % n_jobs].push(m.clone());
            }
            cs
        };

        let mut handles = Vec::new();
        for chunk in &chunks {
            if chunk.is_empty() {
                continue;
            }
            let chunk = chunk.clone();
            let succeeded = &succeeded;
            let failed = &failed;
            let baseq2 = cfg.paths.baseq2.clone();
            let out_path = out_path.to_path_buf();
            handles.push(scope.spawn(move || {
                for map in &chunk {
                    let t0 = std::time::Instant::now();
                    let built = match world::generate_map_nav(
                        &baseq2,
                        map,
                        world::ELEVATOR_PENALTY,
                        spacing,
                    ) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!(map, "generate failed: {e}");
                            failed.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };
                    // Don't cache a broken graph — fail the map so the caller knows.
                    if let Err(diag) = world::check_spawn_connectivity(&built) {
                        tracing::error!(map, "{diag}");
                        failed.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let fp =
                        world::Fingerprint::from_bsp(&built.bsp, world::ELEVATOR_PENALTY, spacing);
                    let cache_path = out_path.join(format!("{map}.qnav"));
                    match world::save_mapcache(&cache_path, &built.graph, &fp) {
                        Ok(()) => {
                            tracing::info!(
                                map,
                                ms = t0.elapsed().as_millis() as u64,
                                path = %cache_path.display(),
                                "cached"
                            );
                            succeeded.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            tracing::error!(map, "save failed: {e}");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    });

    let ok = succeeded.load(Ordering::Relaxed);
    let err = failed.load(Ordering::Relaxed);
    tracing::info!(ok, err, "generate-map-cache complete");
    if err > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing subscriber with elapsed time formatting and abbreviated levels
    let start_time = Instant::now();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_timer(ElapsedFormatter(start_time))
        .with_target(false)
        .with_thread_ids(false)
        .event_format(AbbreviatedFormat::new(start_time))
        .init();

    let cli = Cli::parse();

    let cfg = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("config: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cli.cmd {
        Cmd::ConnectOne {
            addr,
            name,
            qport,
            mode,
        } => {
            let name = name.unwrap_or_else(|| "qbots".to_string());
            let qport = qport.unwrap_or_else(default_qport);
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            tracing::info!("connecting '{name}' to {addr} (qport {qport})…  Ctrl-C to stop.");

            match supervisor::run_single(&cfg, addr, &name, qport, mode).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Run {
            addr,
            mode,
            name,
            count,
            qport_base,
            skin,
            skin_random_male,
            skin_random_female,
        } => {
            // `--count` can enable a fleet even when the config roster is empty (and a
            // `--count 0` disables one the config would otherwise enable).
            let fleet_enabled = count.map_or(cfg.fleet.enabled(), |c| c > 0);
            if !fleet_enabled {
                tracing::error!(
                    "no fleet configured — set [fleet].count in config.yaml or pass --count"
                );
                return ExitCode::FAILURE;
            }
            let skin_sel = match skins::SkinSelection::from_cli(
                &cfg.paths.baseq2,
                skin.as_deref(),
                skin_random_male,
                skin_random_female,
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            tracing::info!(?skin_sel, "fleet skin selection");
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            match supervisor::run_fleet(
                Arc::new(cfg),
                addr,
                mode,
                name,
                count,
                qport_base,
                skin_sel,
            )
            .await
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Config => {
            tracing::info!("server      : {}", cfg.server_addr());
            tracing::info!("server_cfg  : {}", cfg.paths.server_cfg.display());
            tracing::info!("baseq2      : {}", cfg.paths.baseq2.display());
            tracing::info!(
                "fleet       : {} bots (prefix '{}'); qport base is per-process unless \
                 `run --qport-base` pins it (config seed {})",
                cfg.fleet.count,
                cfg.fleet.name_prefix,
                cfg.fleet.qport_base
            );
            let maps_dir = cfg.paths.baseq2.join("maps");
            match std::fs::read_dir(&maps_dir) {
                Ok(entries) => {
                    let n = entries
                        .filter_map(Result::ok)
                        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("bsp"))
                        .count();
                    tracing::info!("maps        : {n} .bsp files in {}", maps_dir.display());
                }
                Err(e) => tracing::info!("maps        : can't read {}: {e}", maps_dir.display()),
            }
            let q2dm1 = cfg.map_bsp("q2dm1");
            let exists = q2dm1.exists();
            tracing::info!(
                "q2dm1.bsp   : {} ({})",
                q2dm1.display(),
                if exists { "found" } else { "MISSING" }
            );
            ExitCode::SUCCESS
        }
        Cmd::Status { addr } => {
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            match query_status(addr).await {
                Ok(rep) => {
                    tracing::info!(
                        map = ?rep.map,
                        maxclients = ?rep.maxclients,
                        players = rep.player_count(),
                        "server status"
                    );
                    // Frag leaders first.
                    let mut players = rep.players;
                    players.sort_by_key(|b| std::cmp::Reverse(b.score));
                    for p in &players {
                        tracing::info!("{:>4}  {:>4}ms  {}", p.score, p.ping, p.name);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    tracing::error!("status query failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Trace { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let m = bsp.models.first().expect("bsp has models");
                let center = [
                    (m.mins[0] + m.maxs[0]) * 0.5,
                    (m.mins[1] + m.maxs[1]) * 0.5,
                    (m.mins[2] + m.maxs[2]) * 0.5,
                ];
                tracing::info!(
                    "{}: bounds [{:.0},{:.0},{:.0}]..[{:.0},{:.0},{:.0}]  center=({:.0},{:.0},{:.0})",
                    map,
                    m.mins[0],
                    m.mins[1],
                    m.mins[2],
                    m.maxs[0],
                    m.maxs[1],
                    m.maxs[2],
                    center[0],
                    center[1],
                    center[2]
                );
                tracing::info!(
                    "  point_contents(center) = {:#x}  is_solid={}",
                    cm.point_contents(&center),
                    cm.is_solid(&center)
                );
                // 8 horizontal rays, 4096 units each, from the center.
                const RAY: f32 = 4096.0;
                let dirs = [
                    [1.0f32, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [-1.0, 0.0, 0.0],
                    [0.0, -1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [-1.0, -1.0, 0.0],
                    [1.0, -1.0, 0.0],
                    [-1.0, 1.0, 0.0],
                ];
                for d in dirs {
                    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    let dir = [d[0] / n, d[1] / n, 0.0];
                    let end = [
                        center[0] + dir[0] * RAY,
                        center[1] + dir[1] * RAY,
                        center[2],
                    ];
                    let t = cm.trace(&center, &end, &[0.0; 3], &[0.0; 3], world::MASK_SOLID);
                    tracing::info!(
                        "  dir ({:+.1},{:+.1}): frac={:.3}  hit at {:.0} units  {}",
                        dir[0],
                        dir[1],
                        t.fraction,
                        t.fraction * RAY,
                        if t.fraction < 1.0 { "WALL" } else { "clear" }
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Pvs { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let pvs = world::Pvs::from_lump(bsp.vis.clone());
                match &pvs {
                    Some(p) => tracing::info!("{}: {} clusters", map, p.numclusters()),
                    None => tracing::info!("{}: no PVS lump", map),
                }
                let m = bsp.models.first().expect("bsp has models");
                let center = [
                    (m.mins[0] + m.maxs[0]) * 0.5,
                    (m.mins[1] + m.maxs[1]) * 0.5,
                    (m.mins[2] + m.maxs[2]) * 0.5,
                ];
                let cluster = cm.point_cluster(&center);
                tracing::info!(
                    "  center ({:.0},{:.0},{:.0}) → cluster {}",
                    center[0],
                    center[1],
                    center[2],
                    cluster
                );
                if let Some(p) = &pvs {
                    tracing::info!(
                        "  clusters visible from {}: {}",
                        cluster,
                        p.count_visible(cluster)
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Nav { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                let cm = world::CollisionModel::from_bsp(&bsp);
                let m = bsp.models.first().expect("bsp has models");
                let bounds = (m.mins, m.maxs);

                let t0 = std::time::Instant::now();
                let g = world::NavGraph::generate(&cm, bounds, 64.0);
                tracing::info!(
                    "{}: nav graph  {} nodes / {} edges  (spacing 64, {} ms)",
                    map,
                    g.node_count(),
                    g.edge_count(),
                    t0.elapsed().as_millis(),
                );

                // Diagnose connectivity, then find a path inside the largest component.
                let cz = (m.mins[2] + m.maxs[2]) * 0.5;
                let start = g.nearest(&[m.mins[0] + 200.0, m.mins[1] + 200.0, cz]);
                let comps = g.components();

                // Guard against empty nav graph
                let largest = comps
                    .first()
                    .expect("nav graph must have at least one component");
                if largest.is_empty() {
                    tracing::info!("  no walkable nodes in nav graph");
                    return ExitCode::SUCCESS;
                }

                tracing::info!(
                    "  {} components; largest = {} nodes",
                    comps.len(),
                    largest.len()
                );

                // Pick a start node from the largest component
                let s = if let Some(start) = start {
                    if largest.contains(&start) {
                        start
                    } else {
                        largest[0]
                    }
                } else {
                    largest[0]
                };

                // Find the farthest node in the largest component
                let farthest = largest
                    .iter()
                    .copied()
                    .max_by(|&x, &y| {
                        dist2(&g.nodes[x], &g.nodes[s]).total_cmp(&dist2(&g.nodes[y], &g.nodes[s]))
                    })
                    .expect("largest component must have nodes");

                if s == farthest {
                    tracing::info!("  only one node in largest component");
                    return ExitCode::SUCCESS;
                }

                let t0 = std::time::Instant::now();
                match g.path(s, farthest) {
                    Some(path) => {
                        let len: f32 = path
                            .windows(2)
                            .map(|w| {
                                let a = g.nodes[w[0]];
                                let b = g.nodes[w[1]];
                                ((a[0] - b[0]).powi(2)
                                    + (a[1] - b[1]).powi(2)
                                    + (a[2] - b[2]).powi(2))
                                .sqrt()
                            })
                            .sum();
                        tracing::info!(
                            "  path (in largest): {}→{}: {} hops / {} nodes, {:.0} units  ({} ms)",
                            s,
                            farthest,
                            path.len() - 1,
                            path.len(),
                            len,
                            t0.elapsed().as_millis(),
                        );
                    }
                    None => tracing::info!("  no path in largest component (this is a bug!)"),
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Learn {
            map: _,
            addr: _,
            output: _,
        } => {
            tracing::info!("learning nav graph (stub)");
            // TODO: Implement learning logic
            tracing::warn!("Learn command not yet implemented - using grid sampling instead");
            ExitCode::FAILURE
        }
        Cmd::SpawnToSpawn {
            map,
            addr,
            name,
            count,
            max_secs,
            lift_penalty,
            spacing,
            mode,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::FarthestSpawn,
                count,
                max_secs,
                lift_penalty,
                spacing,
                mode,
            )
            .await
        }
        Cmd::SpawnToWeapon {
            weapon_name,
            map,
            addr,
            name,
            count,
            max_secs,
            lift_penalty,
            spacing,
            mode,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::Weapon(weapon_name),
                count,
                max_secs,
                lift_penalty,
                spacing,
                mode,
            )
            .await
        }
        Cmd::NavDebug { map, pairs } => nav_debug(&cfg, &map, pairs),
        Cmd::GenerateMapCache {
            map,
            jobs,
            out_dir,
            spacing,
        } => generate_map_cache(&cfg, &map, jobs, &out_dir, spacing),
        Cmd::BspInfo { map } => match world::Bsp::load(&cfg.paths.baseq2, &map) {
            Ok(bsp) => {
                tracing::info!(
                    "{}: v{} | {} planes, {} nodes, {} leafs, {} brushes, {} brushsides, {} leafbrushes, {} models",
                    map,
                    bsp.version,
                    bsp.planes.len(),
                    bsp.nodes.len(),
                    bsp.leafs.len(),
                    bsp.brushes.len(),
                    bsp.brushsides.len(),
                    bsp.leafbrushes.len(),
                    bsp.models.len(),
                );
                // Entity-class histogram — reveals area-connecting entities
                // (teleporters, lifts, doors) the nav graph must account for.
                let mut hist: std::collections::BTreeMap<&str, usize> =
                    std::collections::BTreeMap::new();
                for e in &bsp.entities {
                    *hist.entry(e.classname.as_str()).or_default() += 1;
                }
                for (class, n) in &hist {
                    tracing::info!("  {n:>3}  {class}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const FLUSH: Duration = Duration::from_secs(5);

    #[test]
    fn first_event_emits_no_coda() {
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
    }

    #[test]
    fn consecutive_repeats_suppress_then_coda_on_change() {
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
        // Two repeats (same key): suppressed, no periodic coda yet.
        assert_eq!(
            d.observe("D msg", Duration::from_millis(100), FLUSH),
            DedupAction::Suppress { coda: None }
        );
        assert_eq!(
            d.observe("D msg", Duration::from_millis(200), FLUSH),
            DedupAction::Suppress { coda: None }
        );
        // A different line closes the run with a coda of 2 (the suppressed repeats).
        assert_eq!(
            d.observe("I other", Duration::from_millis(300), FLUSH),
            DedupAction::Emit { prev_coda: Some(2) }
        );
    }

    #[test]
    fn periodic_flush_emits_coda_and_resets_count() {
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
        // Repeats under the flush window: suppressed, no coda.
        for _ in 0..3 {
            assert_eq!(
                d.observe("D msg", Duration::from_secs(1), FLUSH),
                DedupAction::Suppress { coda: None }
            );
        }
        // Once the elapsed since first_seen exceeds the flush window, a periodic
        // coda fires (4 suppressed so far) and the count resets.
        assert_eq!(
            d.observe("D msg", Duration::from_secs(6), FLUSH),
            DedupAction::Suppress { coda: Some(4) }
        );
        // The run continues; the next repeat is suppressed again with no coda.
        assert_eq!(
            d.observe("D msg", Duration::from_secs(7), FLUSH),
            DedupAction::Suppress { coda: None }
        );
    }

    #[test]
    fn different_levels_do_not_merge() {
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
        // Same message text but a different level letter is a different key → emits.
        assert_eq!(
            d.observe("I msg", Duration::from_millis(100), FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
    }

    #[test]
    fn zero_repeat_run_emits_no_coda() {
        // A line that never repeats (count stays 0) produces no coda when the next
        // different line arrives.
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D a", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
        assert_eq!(
            d.observe("D b", Duration::from_millis(100), FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
    }
}

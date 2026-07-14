//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` connects a single bot and keeps it alive; `run`
//! launches the full fleet (Plan 09). Server address and on-disk Q2 paths come from
//! `config.yaml`. The fleet supervisor + per-bot task live in [`supervisor`].

/// Log a final diagnostic at the synthetic FATAL level (rendered as a bold-red
/// `F` by the qbots formatter, see [`AbbreviatedFormat`]) and terminate the
/// process with exit code 1.
///
/// `tracing` has no level above `ERROR`, so we synthesize FATAL: emit an `ERROR`
/// event tagged `target: "FATAL"` (which the formatter renders as `F`), flush
/// stdout, then `std::process::exit(1)`. Call this **after** any diagnostic dump
/// — the dump lines must print first. This macro never returns.
#[macro_export]
macro_rules! fatal {
    ($($arg:tt)*) => {{
        tracing::error!(target: "FATAL", $($arg)*);
        {
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        std::process::exit(1);
    }};
}

mod beacon;
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
/// representations behind one flag (`--navmode`); the steering loop is identical for both.
/// Each variant's canonical CLI token is its short code (`as`, `rc`, …) — the same code
/// the competition scoreboard/bot-names use — with the long kebab name kept as an accepted
/// alias, so both `--navmode rc` and `--navmode hybrid-race` work. The `--help` line leads
/// with the short code and names the long form in its description.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NavMode {
    /// astar — Waypoint-graph backend: A* over grid-sampled nodes (the default, proven backend).
    #[value(name = "as", alias = "astar")]
    Astar,
    /// navmesh — Navmesh backend: A* over walkable polygons + funnel (Recast-style).
    #[value(name = "nm", alias = "navmesh")]
    Navmesh,
    /// hybrid-fallback — Hybrid: A* primary, navmesh takes over the segment on a hard-stuck (Plan 20).
    #[value(name = "fb", alias = "hybrid-fallback")]
    HybridFallback,
    /// hybrid-race — Hybrid: plan both backends per goal, run the cheaper-scoring one to completion.
    #[value(name = "rc", alias = "hybrid-race")]
    HybridRace,
    /// hybrid-hier — Hybrid: navmesh picks the corridor, A* executes a sliding local sub-goal.
    #[value(name = "hr", alias = "hybrid-hier")]
    HybridHier,
    /// hybrid-segment — Hybrid: navmesh routes open space, A* owns jump-link segments only.
    #[value(name = "sg", alias = "hybrid-segment")]
    HybridSegment,
    /// xon-goal — Xonotic route texture over A*: travel-time costs, live danger pricing,
    /// chase cutover, goal-progress watchdog (Plan 61).
    #[value(name = "xg", alias = "xon-goal")]
    XonGoal,
}

impl NavMode {
    /// True for the backends that need a navmesh built (so the factory can skip building it
    /// for pure `astar`, whose construction is graph-only).
    fn needs_mesh(self) -> bool {
        !matches!(self, NavMode::Astar | NavMode::XonGoal)
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
        NavMode::XonGoal => Box::new(brain::XonNavDriver::new(graph)),
        NavMode::Navmesh => Box::new(brain::NavmeshDriver::new(mesh.unwrap(), AGENT_RADIUS)),
        NavMode::HybridFallback => Box::new(brain::hybrid::HybridFallback::new(
            graph,
            mesh.unwrap(),
            AGENT_RADIUS,
        )),
        NavMode::HybridRace => Box::new(brain::hybrid::HybridRace::new(
            graph,
            mesh.unwrap(),
            AGENT_RADIUS,
        )),
        NavMode::HybridHier => Box::new(brain::hybrid::HybridHier::new(
            graph,
            mesh.unwrap(),
            AGENT_RADIUS,
        )),
        NavMode::HybridSegment => Box::new(brain::hybrid::HybridSegment::new(
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
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Brain (decision plugin): `main` (default), `sentry`, `runtester`, or `q3` (the
        /// Quake 3-derived brain). Independent of `--navmode`.
        #[arg(long, value_enum, default_value_t = brain::BrainKind::Main)]
        brain: brain::BrainKind,
        /// Q3 personality (only for `--brain q3`): `grunt`/`major`/`sarge`/`camper`. Absent →
        /// the skill-derived default character.
        #[arg(long, value_enum)]
        char: Option<brain::CharPreset>,
        /// Persona (only for `--brain main`): `rusher`/`sniper`/`scavenger`/`guard`. Absent →
        /// the behavior-preserving default persona.
        #[arg(long)]
        persona: Option<String>,
        /// Xonotic personality (only for `--brain xon`): `rus`/`shp`/`trt`/`nob` (Plan 60).
        /// Absent → a neutral XonSkill at the master skill level.
        #[arg(long, value_enum)]
        xonchar: Option<brain::XonCharPreset>,
    },
    /// Launch the full bot fleet from the config's `[fleet]` roster.
    Run {
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Navigation backend for the whole fleet: `astar` (waypoint graph, default) or
        /// `navmesh` (polygon mesh + funnel). The navmesh backend requires the map's nav
        /// cache to be present (`generate-map-cache --map <m>`).
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Brain (decision plugin) for the whole fleet: `main` (default), `sentry`, `runtester`,
        /// or `q3`. Overrides `[fleet].brain`. Independent of `--navmode`.
        #[arg(long, value_enum)]
        brain: Option<brain::BrainKind>,
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
        /// Q3 personality for the whole fleet (only for `--brain q3`):
        /// `grunt`/`major`/`sarge`/`camper`. Overrides `[fleet].char`. Pins each bot's skin to
        /// the character's. Absent → the skill-derived default character.
        #[arg(long, value_enum)]
        char: Option<brain::CharPreset>,
        /// Proceed with warnings instead of failing when a bot can't join (e.g. the
        /// server's `maxclients` is full). Default: any join failure aborts the fleet
        /// with a non-zero exit.
        #[arg(long)]
        loose_botcap: bool,
    },
    /// Spawn N bots for EACH `--navmode` × `--brains` group at once in one process (shared nav
    /// cache), each group wearing a distinct skin, and print a per-group frag scoreboard. Bots are
    /// named `<brain>_<navmode>[_<char>]_<i>` using short codes so the name fits Q2's 15-char limit
    /// (e.g. `mai_as_1`, `q3_rc_1`, `q3_rc_gru_1`; a code→full-name legend is logged at launch).
    /// `runtester` (non-combat) is rejected. Ctrl-C ends the competition and prints the final board.
    Competition {
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
        /// Bots to spawn **per group** (default 8), a group = one (navmode, brain) pair. Total =
        /// navmodes × brains × count, clamped by `[fleet].max_bots` (server maxclients headroom).
        #[arg(long, default_value = "8")]
        count: usize,
        /// Nav backends to include, comma-separated (default: all). See possible values below.
        /// e.g. `--navmodes astar,navmesh,hybrid-race`.
        #[arg(long = "navmodes", value_enum, value_delimiter = ',')]
        modes: Vec<NavMode>,
        /// Brains to include, comma-separated (default: `main`; `runtester` is rejected).
        /// Spawns the full `{modes} × {brains}` cross product. e.g. `--brains main,q3`.
        #[arg(long = "brains", value_enum, value_delimiter = ',')]
        brains: Vec<brain::BrainKind>,
        /// Q3 personalities to field for the `q3` brain, comma-separated (e.g.
        /// `--chars grunt,major,sarge,camper`). Each becomes its own group/skin. Ignored by
        /// non-`q3` brains. Absent → one default-character `q3` group.
        #[arg(long = "chars", value_enum, value_delimiter = ',')]
        chars: Vec<brain::CharPreset>,
        /// Xonotic personalities to field for the `xon` brain, comma-separated (e.g.
        /// `--xonchars rus,shp,trt,nob`). Each becomes its own group/skin. Ignored by
        /// non-`xon` brains. Absent → one neutral `xon` group.
        #[arg(long = "xonchars", value_enum, value_delimiter = ',')]
        xonchars: Vec<brain::XonCharPreset>,
        /// Base qport; group `g` bot `i` uses `base + g*count + i` (disjoint per-group blocks,
        /// group = a (mode,brain[,char]) tuple). Per-process default if omitted.
        #[arg(long)]
        qport_base: Option<u16>,
        /// Proceed with warnings instead of failing when a bot can't join (e.g. the
        /// server's `maxclients` is full). Default: any join failure aborts the
        /// competition with a non-zero exit.
        #[arg(long)]
        loose_botcap: bool,
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
    /// Drive one bot from spawn to the farthest DM spawn point; log movement; stop.
    /// The measurement lens for movement quality (Plan 10).
    SpawnToSpawn {
        /// Map to load. Autodetected from the server's `status` reply if omitted;
        /// pass `--map` only to override (it must match the server's current map).
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
        /// Grid spacing (units) of the nav graph to use. Each spacing has its own cache
        /// dir (`data/mapcache/<spacing>/`); generate it first with `generate-map-cache
        /// --spacing <n>`. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-navmesh --map <m>` first.
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Decision brain: `runtester` (default — the pure pathfinder) or `main` to A/B the
        /// live combat brain's pathing (combat is forced off either way). Independent of `--navmode`.
        #[arg(long, value_enum, default_value_t = brain::BrainKind::RunTester)]
        brain: brain::BrainKind,
    },
    /// Drive one bot from spawn to a named weapon's BSP origin; log movement; stop.
    SpawnToWeapon {
        /// Weapon to reach, e.g. `rocketlauncher` (resolved as `weapon_<name>`).
        weapon_name: String,
        /// Which matching entity to target when a map has several (0-based). q2dm3 has
        /// two `weapon_railgun`: 0 = `(-368,-64,352)`, 1 = `(768,816,208)` (the loop-train
        /// + elevator one). The resolver logs all candidates.
        #[arg(long, default_value = "0")]
        instance: usize,
        /// Map to load. Autodetected from the server's `status` reply if omitted;
        /// pass `--map` only to override (it must match the server's current map).
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
        /// Grid spacing (units) of the nav graph to use. Each spacing has its own cache
        /// dir (`data/mapcache/<spacing>/`); generate it first with `generate-map-cache
        /// --spacing <n>`. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-navmesh --map <m>` first.
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Decision brain: `runtester` (default — the pure pathfinder) or `main` to A/B the
        /// live combat brain's pathing (combat is forced off either way). Independent of `--navmode`.
        #[arg(long, value_enum, default_value_t = brain::BrainKind::RunTester)]
        brain: brain::BrainKind,
    },
    /// Drive one bot from spawn to a named item's BSP origin; log movement; stop.
    /// Item names are resolved through aliases (e.g. `quaddamage` → `item_quad`).
    SpawnToItem {
        /// Item to reach, e.g. `quaddamage` (alias of `item_quad`), `mega`, `invuln`.
        /// Anything already prefixed `item_` is used verbatim.
        item_name: String,
        /// Which matching entity to target when a map has several (0-based). The resolver
        /// logs all candidates so you can pick.
        #[arg(long, default_value = "0")]
        instance: usize,
        /// Map to load. Autodetected from the server's `status` reply if omitted;
        /// pass `--map` only to override (it must match the server's current map).
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
        /// Grid spacing (units) of the nav graph to use. Each spacing has its own cache
        /// dir (`data/mapcache/<spacing>/`); generate it first with `generate-map-cache
        /// --spacing <n>`. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (waypoint graph, default) or `navmesh` (polygon
        /// mesh + funnel). The navmesh backend requires `generate-navmesh --map <m>` first.
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Decision brain: `runtester` (default — the pure pathfinder) or `main` to A/B the
        /// live combat brain's pathing (combat is forced off either way). Independent of `--navmode`.
        #[arg(long, value_enum, default_value_t = brain::BrainKind::RunTester)]
        brain: brain::BrainKind,
    },
    /// Drive a bot to an ARBITRARY world coordinate (Plan 35 T3). Isolates a single nav
    /// feature — e.g. `spawn-to-point 191 -329 216` drives to the q2dm3 *10 board ledge —
    /// so route-reliability and ride-correctness can be measured apart from the full item route.
    SpawnToPoint {
        /// Target world X.
        x: f32,
        /// Target world Y.
        y: f32,
        /// Target world Z.
        z: f32,
        /// Map to load. Autodetected from the server's `status` reply if omitted.
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
        /// Grid spacing (units) of the nav graph to use. Default 24.
        #[arg(long, default_value = "24")]
        spacing: f32,
        /// Navigation backend: `astar` (default) or `navmesh`.
        #[arg(long = "navmode", value_enum, default_value_t = NavMode::Astar)]
        mode: NavMode,
        /// Decision brain: `runtester` (default) or `main`.
        #[arg(long, value_enum, default_value_t = brain::BrainKind::RunTester)]
        brain: brain::BrainKind,
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
        /// Exit 0 even if some maps fail, as long as at least one cached. Caches every
        /// good map and reports the failures; lets a `q2dm*` batch succeed while one map
        /// (e.g. q2dm3) is a known-broken exception. Without it, any failure exits non-zero.
        #[arg(long)]
        allow_failures: bool,
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

/// Abbreviate tracing level to a single letter: T, D, I, W, E.
///
/// When `ansi` is true, each letter is wrapped in the same ANSI color
/// tracing-subscriber paints the long level names with
/// (`fmt/format/mod.rs::FmtLevel`): TRACE=purple(35), DEBUG=blue(34),
/// INFO=green(32), WARN=yellow(33), ERROR=red(31). The color+letter+reset
/// runs are precomputed so this stays a cheap `&'static str` lookup.
fn abbreviate_level(level: tracing::Level, ansi: bool) -> &'static str {
    if ansi {
        match level {
            tracing::Level::TRACE => "\x1b[35mT\x1b[0m",
            tracing::Level::DEBUG => "\x1b[34mD\x1b[0m",
            tracing::Level::INFO => "\x1b[32mI\x1b[0m",
            tracing::Level::WARN => "\x1b[33mW\x1b[0m",
            tracing::Level::ERROR => "\x1b[31mE\x1b[0m",
        }
    } else {
        match level {
            tracing::Level::TRACE => "T",
            tracing::Level::DEBUG => "D",
            tracing::Level::INFO => "I",
            tracing::Level::WARN => "W",
            tracing::Level::ERROR => "E",
        }
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
                // `elapsed` is sampled before the layer's lock is taken, so a thread can arrive
                // here with a value slightly *earlier* than a `first_seen` that another thread
                // already advanced under the lock. Saturating-sub yields 0 in that race (just
                // "not time to flush yet") instead of underflowing the Duration and panicking.
                if elapsed.saturating_sub(r.first_seen) >= flush {
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
    /// Color the abbreviated level letter with ANSI codes (only when the sink
    /// is a terminal, so redirected/piped output stays plain text).
    ansi: bool,
}

impl AbbreviatedFormat {
    fn new(start_time: Instant, ansi: bool) -> Self {
        Self {
            start_time,
            dedup: Arc::new(Mutex::new(Deduper::default())),
            ansi,
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
        // The `fatal!` macro tags its event `target: "FATAL"`; render it as a
        // bold-bright-red `F` (there is no FATAL level in `tracing`). Everything
        // else uses the standard level letter.
        let level = if event.metadata().target() == "FATAL" {
            if self.ansi {
                "\x1b[1;31mF\x1b[0m"
            } else {
                "F"
            }
        } else {
            abbreviate_level(*event.metadata().level(), self.ansi)
        };

        // Prefix the enclosing span chain's fields (e.g. the per-bot `bot{name=…}` span)
        // so fleet logs are per-bot attributable (Plan 09 T3 — the fields were recorded
        // but never printed until Plan 63's telemetry work needed them).
        let mut spans = String::new();
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let ext = span.extensions();
                if let Some(f) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    if !f.is_empty() {
                        use std::fmt::Write as _;
                        let _ = write!(spans, "[{f}] ");
                    }
                }
            }
        }

        // Format the event's fields into a buffer (the dedup key + the emit body).
        let mut fields = String::new();
        ctx.field_format().format_fields(
            tracing_subscriber::fmt::format::Writer::new(&mut fields),
            event,
        )?;
        let key = format!("{level} {spans}{fields}");

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
                writeln!(writer, "{secs:04}.{millis:03} {level} {spans}{fields}")
            }
        }
    }
}

/// Send a connectionless `status` query and parse the reply (Plan 09). Times out
/// after 2 s — a down server or a dropped packet must not hang the CLI.
pub(crate) async fn query_status(addr: SocketAddr) -> std::io::Result<status::StatusReport> {
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

/// Detect the server's current map and validate that usable nav data exists for it —
/// **before any bot connects**. This is the single up-front gate so we never spawn a
/// fleet that each discovers a missing/garbage nav cache mid-run (one task at a time,
/// staggered, surfacing the error seconds in). Every server-connecting subcommand calls
/// this first.
///
/// Returns the resolved map name on success, or `Err(ExitCode::FAILURE)` after logging
/// the precise reason (no map in status reply, map mismatch, missing cache, or a nav
/// connectivity bug). `map_override` is the optional `--map` flag (skips autodetection);
/// `spacing` must match what the bots will load with.
async fn preflight_map(
    cfg: &Config,
    addr: SocketAddr,
    map_override: Option<&str>,
    spacing: f32,
    // When true (the movement-test harness), a not-fully-connected nav graph is a WARNING
    // rather than a fatal abort — a scenario only needs the bot's spawn to reach its pinned
    // goal (checked per-spawn in `run_scenario`). Lets us exercise q2dm3's quad/railgun while
    // the broad floor-connectivity work (Plan 35) is still in progress. The fleet path passes
    // `false` and keeps the strict gate.
    allow_partial: bool,
) -> Result<String, ExitCode> {
    // 1. Resolve the map: explicit `--map` override, else autodetect via OOB `status`.
    let map = match map_override {
        Some(m) => {
            tracing::info!(map = %m, "using --map override");
            m.to_string()
        }
        None => match query_status(addr).await {
            Ok(report) => match report.map {
                Some(m) => {
                    tracing::info!(map = %m, "autodetected server map");
                    m
                }
                None => {
                    tracing::error!("server status reply carried no map; pass --map to override");
                    return Err(ExitCode::FAILURE);
                }
            },
            Err(e) => {
                tracing::error!("couldn't query server for its map ({e}); pass --map to override");
                return Err(ExitCode::FAILURE);
            }
        },
    };

    // 2. Validate the nav cache loads NOW (fatal on miss/stale/garbage) so the failure
    //    is immediate and once, not per-bot at +Ns. This loads the BSP, builds the CM,
    //    and loads the cached graph exactly as the bots will.
    let cache_dir = std::path::Path::new("data/mapcache");
    let built =
        world::cached_map_nav(&cfg.paths.baseq2, &map, Some(cache_dir), spacing).map_err(|e| {
            tracing::error!("{e}");
            ExitCode::FAILURE
        })?;
    if let Err(diag) = world::check_spawn_connectivity(&built) {
        if !allow_partial {
            tracing::error!("{diag}");
            crate::fatal!(map = %map, "aborting: nav connectivity bug — all spawns must be reachable");
        }
        tracing::warn!(map = %map, "nav graph not fully spawn-connected; movement-test harness continues: {diag}");
    }
    tracing::info!(map = %map, "preflight ok: server map detected and nav cache validated");
    Ok(map)
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
    // Plan 66: relays `sv.framenum` to qctrl. `None` when the beacon is disabled (the
    // default), in which case this bot behaves exactly as it did before.
    beacon: Option<&beacon::Beacon>,
    mode: NavMode,
    brain_kind: brain::BrainKind,
    char: Option<brain::CharPreset>,
    persona: Option<brain::persona::Persona>,
    xonchar: Option<brain::XonCharPreset>,
) -> std::io::Result<()> {
    use brain::perception::Worldview;
    // `Brain` is the plugin trait (its methods resolve on the `Box<dyn Brain>` the factory
    // returns); `build_brain`/`BrainKind` select the implementation, mirroring `build_navigator`.
    use brain::{
        build_brain, BotSkill, Brain, BrainConfig, BrainContext, BrainMap, MovementController,
        Navigator,
    };
    use client::{Conn, ConnState};
    use q2proto::Usercmd;
    use std::time::Duration;
    use tokio::net::UdpSocket;
    use tokio::time;

    // T1 diagnostic toggle: log live brush-model (`*N`) entity origins each frame (read-only).
    let observe_movers = std::env::var("QBOTS_OBSERVE_MOVERS").is_ok();
    // VWep inspection (Plan 28): with QBOTS_P28_DEBUG set, dump each player entity's VWep wield
    // model (`modelindex2` → CS_MODELS) to verify whether the server sends the enemy's held weapon
    // over the wire. Finding (2026-07-10, this yquake2 server): players carry `modelindex2 = 255`
    // (a sentinel — CS slot 255 is empty), so enemy-weapon inference is NOT available here.
    let p28_debug = std::env::var("QBOTS_P28_DEBUG").is_ok();

    // Per-bot span attribution (Plan 09 T3) is applied by the CALLERS via
    // `Future::instrument` (supervisor.rs) — an inline `span.enter()` here leaked its
    // guard across `.await` points, stacking other bots' spans onto whatever task the
    // thread polled next (observed live: three `[name=…]` prefixes on one event).

    // Register this bot with the fleet tally so it appears in the report even
    // if it never frags/dies (Plan 09 observability).
    stats.register(name);

    // Mutable since Plan 64: a hard server restart (svc_reconnect) rebinds to a fresh
    // local port so stale packets from the dead connection can't poison the new one.
    let mut sock = UdpSocket::bind("0.0.0.0:0").await?;
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
    // Plan 64: consecutive ticks spent frozen in intermission (PM_FREEZE playerstate).
    let mut intermission_ticks: u32 = 0;
    // Plan 64: consecutive ticks spent in Connecting — paces handshake resends.
    let mut connecting_ticks: u32 = 0;
    // Plan 64: whether this task ever reached Active — a later handshake timeout is then
    // a map-change re-handshake (retryable), not a failed initial join (fatal).
    let mut was_active = false;
    // Plan 64: per-bot rejoin stagger. A map change hits the whole fleet in the same
    // instant; 40 simultaneous configstring pumps overflow the server's per-client
    // reliable channel (SV_SendDisconnect, sv_send.c:577 — observed live as mass bare
    // svc_disconnect). A deterministic 0–8 s name-hash jitter breaks the herd, matching
    // the initial join's supervisor-side stagger.
    let rejoin_jitter = Duration::from_millis(
        name.bytes()
            .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64))
            % 8000,
    );
    // While Some, the re-handshake is held back until the instant inside.
    let mut rejoin_hold: Option<time::Instant> = None;

    let mut move_ctrl = MovementController::new();
    // The decision layer (Plan 22): owns combat/FSM/dodge/steering/recovery/skill/roam.
    // Built early; learns the nav graph at map load via `set_map`. The `Navigator` is
    // injected into `brain.tick` each frame — the brain uses nav, never owns it.
    let mut brain: Box<dyn Brain + Send> = build_brain(
        brain_kind,
        BotSkill::default(),
        BrainConfig::default(),
        char,
        persona,
        xonchar,
    );
    // Boxed behind the `Navigator` trait so the tick loop is backend-agnostic: `--navmode`
    // picks A* (waypoint graph) or navmesh (polygons + funnel) at map load. `+ Send`
    // because this future is spawned on tokio and holds the driver across awaits.
    let mut nav_driver: Option<Box<dyn Navigator + Send>> = None;
    // Collision model the nav graph was built from — for LOS gating (Plan 11) and
    // reactive wall probes (Plan 13). Set when the map loads.
    let mut collision: Option<Arc<world::CollisionModel>> = None;
    let mut map_loaded = false;
    // Plan 64: the servercount the loaded map belongs to. Every SV_SpawnServer bumps it,
    // so a mismatch against the live serverdata means the server changed (or restarted)
    // the level and all per-map state below is stale.
    let mut map_servercount: Option<i32> = None;
    // Plan 66: the map name we attribute beacon frames to. Set when configstring 33 lands,
    // and CLEARED on a level change — a frame from the new level must never be published
    // against the old level's map name.
    let mut beacon_map = String::new();
    // Counts this bot as Active in the beacon for as long as it holds a session. Armed on
    // the first frame; dropped (and decremented) on every exit path, panic included.
    let mut beacon_active: Option<beacon::ActiveBot> = None;
    let mut last_serverframe: Option<i32> = None;
    let mut last_health: Option<i32> = None; // Track health across frames for damage detection
    let mut last_armor: Option<i32> = None; // Track armor for pickup detection (Plan 67)
    let mut last_pickup_cs: Option<i16> = None; // Track STAT_PICKUP_STRING for weapon pickups (Plan 68)
    let mut last_frags: Option<i32> = None; // Track frags for kill detection

    // Plan 08: per-bot danger/popularity heatmap observer + the origin we were
    // at last time we were alive (death attribution, before the respawn teleport).
    let mut heatmap_obs: Option<brain::HeatmapObserver> = None;
    let mut last_alive_pos: Option<Vec3> = None;

    // Plan 51: wall-press/stall episode detector — one `EVT wall_press` line per
    // sustained intent-vs-motion mismatch (brain-agnostic, observational only).
    let mut stall_mon = brain::StallMonitor::new();

    // Plan 53: connect-phase deadline. A bot that never reaches `Active` within this
    // window (e.g. a silently-dropped handshake the reject parse can't classify) fails
    // its join instead of hanging forever. Per bot_task invocation, so it resets on each
    // reconnect attempt.
    // Mutable since Plan 64: a mid-game map change drops us back into the handshake, and
    // the re-handshake gets a fresh deadline (the original one passed long ago).
    let mut connect_deadline =
        time::Instant::now() + Duration::from_millis(cfg.fleet.connect_timeout_ms);

    // Plan 65: Active-state frame-stall watchdog. The Plan 53 deadline above is gated to
    // `state != Active`, so a bot whose slot silently died — a hard map change whose
    // unreliable svc_reconnect copies (SV_FinalMessage) were all lost — used to stay
    // Active forever, feeding clc_move into a recycled slot the server ignores (no
    // netchan match ⇒ zero bytes back, bot_task never exits, the supervisor never
    // retries; observed live as gradual fleet attrition across rotations). An Active
    // client receives svc_frame at 10 Hz, intermission included: `stall_timeout_ms`
    // without one new serverframe means the slot is dead, and the recovery is the same
    // retryable ConnectionReset re-handshake the supervisor already provides.
    let stall_timeout = Duration::from_millis(cfg.fleet.stall_timeout_ms);
    let mut last_frame_seen = time::Instant::now();

    // Plan 57: ack-on-frame send re-phasing. The 10 Hz timer below no longer owns the
    // send; it builds the decision and caches it in `last_cmd`, and the recv arm sends
    // that cmd the instant a new server frame arrives (acking it). `last_send` gates the
    // timer down to a keepalive that only fires when frames stall (no send in ~90 ms).
    // `send_timing` measures the resulting frame-arrival→ack phase delay (`EVT send_timing`).
    const KEEPALIVE_GAP: Duration = Duration::from_millis(90);
    let mut last_cmd: Option<Usercmd> = None;
    let mut last_send = Instant::now();
    let mut send_timing = client::SendTiming::new();

    loop {
        if shutdown.requested() {
            // Plan 64: also send the clean disconnect while merely Connected (mid
            // map-change re-handshake) — otherwise our slot lingers server-side as a
            // CNCT ghost until the server times it out, eating into maxclients.
            if matches!(conn.state(), ConnState::Active | ConnState::Connected) {
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
                // Plan 57: remember which frame we held before parsing, so we can detect a
                // freshly-decoded snapshot below and ack it on arrival.
                let prev_sf = conn.frame.as_ref().map(|f| f.serverframe);
                let prev_state = conn.state();
                was_active |= prev_state == ConnState::Active;
                let mut reply = conn.on_recv(&buf[..n]);
                // Plan 64: the server pulled us out of Active — either the soft
                // map-change stufftext flow ("changing"/"reconnect" → Connected, netchan
                // kept) or a hard svc_reconnect (rcon `map X` restarts the game and wipes
                // our slot → Connecting, full handshake). Re-arm the Plan 53 connect
                // deadline — the original one expired long ago, so without this the very
                // next tick would classify the re-handshake as a timed-out join.
                let now_state = conn.state();
                if prev_state == ConnState::Active
                    && matches!(now_state, ConnState::Connected | ConnState::Connecting)
                {
                    tracing::info!(state = ?now_state, jitter_ms = rejoin_jitter.as_millis() as u64, "map change: re-handshaking");
                    // Deadline covers the anti-herd jitter PLUS the normal handshake.
                    connect_deadline = time::Instant::now()
                        + rejoin_jitter
                        + Duration::from_millis(cfg.fleet.connect_timeout_ms);
                    rejoin_hold = Some(time::Instant::now() + rejoin_jitter);
                    // Hard restart: the whole old connection is dead, but its final
                    // packets are still in flight — SV_FinalMessage sends staggered
                    // copies of `svc_print "Server restarted" + svc_disconnect`, and we
                    // rejoin fast enough (~20 ms) that a copy lands AFTER the new netchan
                    // is up, gets accepted as a huge sequence jump, and makes the bot
                    // abandon a perfectly live slot (observed live: 32/40 bots dropped,
                    // ghost slots piled up to "Server is full."). A fresh local port
                    // makes the stale copies undeliverable, like a real client restart.
                    if now_state == ConnState::Connecting {
                        sock = UdpSocket::bind("0.0.0.0:0").await?;
                        sock.connect(addr).await?;
                        // Hold the getchallenge too — the ticker sends it (and its
                        // 2 s resends) once the jitter window passes.
                        reply = None;
                    }
                }
                if let Some(pkt) = reply {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    // Surface any buffered server prints — the drop reason (e.g.
                    // "Server restarted", rate-limit kicks) arrives as svc_print.
                    for text in conn.drain_prints() {
                        tracing::warn!(text = %text.trim_end(), "server print at disconnect");
                    }
                    tracing::info!("disconnected");
                    return Ok(());
                }
                // Plan 53: the server refused our join (Server is full., Bad challenge., …).
                // Surface it as a fatal, non-retryable error so the fleet can fail loudly.
                if conn.state() == ConnState::Rejected {
                    let reason = conn
                        .reject_reason
                        .clone()
                        .unwrap_or_else(|| "connection refused".to_string());
                    tracing::error!(%reason, was_active, "server rejected join");
                    // Plan 64: a rejection on the REJOIN after a map change (e.g. a
                    // transient "Server is full." while ghost slots time out) is
                    // retryable; only the initial join stays fatal (Plan 53).
                    let kind = if was_active {
                        std::io::ErrorKind::ConnectionReset
                    } else {
                        std::io::ErrorKind::ConnectionRefused
                    };
                    return Err(std::io::Error::new(kind, format!("join rejected: {reason}")));
                }

                // Plan 57: ack the just-arrived server frame immediately. The server
                // measures ping as (recv of our clc_move acking frame N) − (senttime of
                // frame N), so replying on arrival instead of on the free-running 100 ms
                // timer collapses the reported ping to ≈ true RTT. We re-send the last
                // timer-built cmd (same msec) — the server emits frames at 10 Hz, so this
                // is still ~10 sends/sec: we re-phase the send, we do not speed it up.
                if conn.state() == ConnState::Active {
                    if let Some(sf) = conn.frame.as_ref().map(|f| f.serverframe) {
                        if Some(sf) != prev_sf {
                            last_frame_seen = time::Instant::now();
                            let now = Instant::now();
                            send_timing.on_frame(sf, now);

                            // Plan 66: relay this frame to the fleet beacon. Already exactly
                            // once per distinct frame per bot, so this is the natural hook.
                            // 31 of 32 bots take the no-op path inside `fold` — see beacon.rs.
                            if let Some(b) = beacon {
                                if beacon_active.is_none() {
                                    beacon_active = Some(b.bot_active());
                                }
                                if let Some(sc) =
                                    conn.serverdata.as_ref().map(|sd| sd.servercount)
                                {
                                    b.on_frame(sc, sf, &beacon_map, now);
                                }
                            }

                            if let Some(cmd) = last_cmd {
                                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                                    let _ = sock.send(&pkt).await;
                                    send_timing.on_ack_sent(sf, now);
                                    last_send = now;
                                }
                            }
                        }
                    }
                }
            }

            _ = ticker.tick() => {
                ticks = ticks.wrapping_add(1);

                // Plan 53: connect-phase timeout. Once Active this never trips; before
                // that, a bot stuck in the handshake past the deadline fails its join so
                // the supervisor can react instead of the task hanging silently forever.
                if conn.state() != ConnState::Active && time::Instant::now() >= connect_deadline {
                    tracing::error!(
                        timeout_ms = cfg.fleet.connect_timeout_ms,
                        state = ?conn.state(),
                        was_active,
                        "connect handshake timed out"
                    );
                    // Plan 64: a timeout on the re-handshake AFTER a map change is not a
                    // join failure — the slot existed moments ago and the server is just
                    // slow coming back. ConnectionReset makes the supervisor retry with
                    // backoff; TimedOut (initial join) stays fatal per Plan 53.
                    let kind = if was_active {
                        std::io::ErrorKind::ConnectionReset
                    } else {
                        std::io::ErrorKind::TimedOut
                    };
                    return Err(std::io::Error::new(kind, "connect handshake timed out"));
                }

                // Plan 65: frame-stall watchdog (see `last_frame_seen` above). While not
                // Active the connect deadline owns hang detection, so keep the baseline
                // fresh; while Active, a full `stall_timeout` without one new serverframe
                // means the slot is dead — return the retryable ConnectionReset so the
                // supervisor re-handshakes on a fresh socket.
                if conn.state() != ConnState::Active {
                    last_frame_seen = time::Instant::now();
                } else if time::Instant::now() >= last_frame_seen + stall_timeout {
                    tracing::error!(
                        stall_timeout_ms = cfg.fleet.stall_timeout_ms,
                        "no server frames while Active — slot presumed dead, re-handshaking"
                    );
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "server frames stalled while Active",
                    ));
                }

                let (frame_opt, cs) = (conn.frame.clone(), conn.configstrings().clone());
                let state = conn.state();
                was_active |= state == ConnState::Active;

                // Plan 64: pace the map-change re-handshake. The hold is the per-bot
                // anti-herd jitter; once it passes, the hard path sends getchallenge
                // (re-sent every ~2 s — a hard restart drops packets while the level
                // loads, and real clients resend too: CL_CheckForResend, cl_main.c) and
                // the soft path queues the deferred "new".
                let hold_just_cleared = match rejoin_hold {
                    Some(t) if time::Instant::now() >= t => {
                        rejoin_hold = None;
                        true
                    }
                    _ => false,
                };
                if state == ConnState::Connecting {
                    connecting_ticks = connecting_ticks.wrapping_add(1);
                    // First send fires the tick the hold clears; then every 2 s.
                    if rejoin_hold.is_none()
                        && (hold_just_cleared || connecting_ticks.is_multiple_of(20))
                    {
                        if let Some(pkt) = conn.resend_connect() {
                            tracing::info!("(re)sending getchallenge");
                            let _ = sock.send(&pkt).await;
                        }
                    }
                } else {
                    connecting_ticks = 0;
                    if rejoin_hold.is_none() && conn.rejoin_pending() {
                        tracing::info!("soft map change: sending new");
                        conn.send_new();
                    }
                }
                let playernum = conn.serverdata.as_ref().map(|sd| sd.playernum).unwrap_or(0);

                // Track health across frames for damage detection
                let mut dmg_this_tick: i32 = 0; // Plan 51: fed to the stall monitor below
                if let Some(ref frame) = frame_opt {
                    let view = Worldview::from_frame(frame, &cs, playernum);
                    let current_health = view.self_state().health;
                    if current_health > 0 {
                        if let Some(prev) = last_health {
                            if prev > 0 && current_health < prev {
                                let damage = prev - current_health;
                                dmg_this_tick = damage;
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
                                // A health gain while alive is a pickup by definition in
                                // stock DM (no regen); respawns are excluded by `prev > 0`
                                // and megahealth rot is a decrease (Plan 67).
                                let healed = current_health - prev;
                                tracing::debug!(kind = "health", amount = healed, "EVT pickup");
                                stats.record_health_pickup(name, healed as u64);
                            }
                            // Same rule for armor: only pickups increase it while alive
                            // (damage absorption + the respawn reset are decreases).
                            let current_armor = view.self_state().armor;
                            if let Some(prev_armor) = last_armor {
                                if current_armor > prev_armor {
                                    let gained = current_armor - prev_armor;
                                    tracing::debug!(
                                        kind = "armor",
                                        amount = gained,
                                        "EVT pickup"
                                    );
                                    stats.record_armor_pickup(name, gained as u64);
                                }
                            }
                            last_armor = Some(current_armor);
                        }
                        last_health = Some(current_health);
                    }

                    // Weapon pickups (Plan 68): the server flashes every item touch through
                    // STAT_PICKUP_STRING as a CS_ITEMS configstring index for 3 s
                    // (g_items.c:1163; shared.h:1138). A *transition* to a gun's pickup_name
                    // is a weapon pickup — our plain `use <name>` switching never writes this
                    // stat (only weapnext/cycleweap do, which we never send).
                    const STAT_PICKUP_STRING: usize = 8; // vendor shared.h:1138
                    let pickup_cs = frame.playerstate.stats[STAT_PICKUP_STRING];
                    if last_pickup_cs.is_some_and(|prev| prev != pickup_cs) && pickup_cs != 0 {
                        if let Some(item) = cs.get(pickup_cs as usize) {
                            if brain::weapons::is_weapon_pickup_name(item) {
                                tracing::debug!(kind = "weapon", item, "EVT pickup");
                                stats.record_weapon_pickup(name);
                            }
                        }
                    }
                    last_pickup_cs = Some(pickup_cs);
                }

                // Plan 64: a servercount other than the one the loaded map came from means
                // the server respawned the level (rcon `map X`, fraglimit rotation, or a
                // same-map restart). Drop every per-map structure; the `!map_loaded` block
                // below reloads from the NEW level's configstrings once they arrive.
                let servercount = conn.serverdata.as_ref().map(|sd| sd.servercount);
                if map_loaded && servercount != map_servercount {
                    tracing::info!(
                        old = ?map_servercount,
                        new = ?servercount,
                        "server changed level — resetting per-map state"
                    );
                    map_loaded = false;
                    map_servercount = None;
                    // Plan 66: the new level's frames are already arriving, but CS 33 hasn't
                    // been re-parsed yet. Publishing them against the OLD map name would make
                    // qctrl attribute the new map's age to the previous map — so say nothing
                    // until we know where we are. `encode` skips a beacon with no map.
                    beacon_map.clear();
                    nav_driver = None;
                    collision = None;
                    heatmap_obs = None;
                    last_serverframe = None;
                    last_health = None;
                    last_armor = None;
                    last_pickup_cs = None;
                    last_frags = None;
                    last_alive_pos = None;
                    stall_mon = brain::StallMonitor::new();
                    send_timing = client::SendTiming::new();
                    // Same semantics as the respawn teleport: clears enemy/goal/FSM state
                    // that would otherwise reference the old map. `set_map` below re-feeds
                    // the graph/items when the new nav graph loads.
                    brain.on_death();
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
                            map_servercount = servercount;
                            beacon_map = map.clone(); // Plan 66
                            tracing::info!(map, bsp = %bsp_path, "loading nav graph");
                            // Shared across the fleet: built once per map, reused as Arc.
                            if let Some(map_nav) = nav_cache.get_or_build(cfg, &map) {
                                // The navmesh backend can't path to a bare A* node index,
                                // so it resolves roam goals to world positions instead.
                                brain.set_map(BrainMap {
                                    roam_nodes: map_nav.roam_nodes.clone(),
                                    nav_graph: Arc::clone(&map_nav.graph),
                                    roam_as_position: matches!(mode, NavMode::Navmesh),
                                    items: map_nav.items.clone(),
                                });
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

                // Plan 64: intermission — the fraglimit/timelimit scoreboard. Every
                // client is PM_FREEZE-frozen and the game only exits to the next level
                // when some client presses a button ≥5 s in (game/player/client.c:2122).
                // On an all-bot server WE are that client: idle the brain while frozen,
                // then hold ATTACK past the gate (~6 s at the 10 Hz tick) to advance.
                let intermission = state == ConnState::Active
                    && frame_opt
                        .as_ref()
                        .is_some_and(|f| f.playerstate.pmove.pm_type == q2proto::PM_FREEZE);
                if !intermission {
                    intermission_ticks = 0;
                }

                let cmd = if intermission {
                    intermission_ticks = intermission_ticks.saturating_add(1);
                    if intermission_ticks == 1 {
                        tracing::info!("intermission — frozen, will press ATTACK to advance the level");
                    }
                    let buttons = if intermission_ticks > 60 {
                        brain::move_ctrl::BUTTON_ATTACK | brain::move_ctrl::BUTTON_ANY
                    } else {
                        0
                    };
                    Usercmd {
                        msec: 33,
                        buttons,
                        ..Default::default()
                    }
                } else if state == ConnState::Active {
                    if let Some(frame) = frame_opt {
                        let view = Worldview::from_frame(&frame, &cs, playernum);

                        // T1 (diagnostic): with QBOTS_OBSERVE_MOVERS set, log MOVING non-player
                        // entities each frame — their live wire origin + per-frame delta. Brush
                        // models (func_train/plat/door) arrive with modelindex=0 (our delta-decode
                        // drops it) but a moving non-zero origin, so we key on motion, not model.
                        // Lets us MEASURE a func_train's actual wire origin/motion (vs the assumed
                        // `corner - mins`) and, with the model bounds, its standable top. Read-only.
                        if observe_movers {
                            const CS_MODELS: usize = 32;
                            for e in &frame.entities {
                                let moved = e.origin != e.old_origin;
                                let nonzero = e.origin != [0.0, 0.0, 0.0];
                                if !(moved && nonzero) {
                                    continue; // skip static + null [0,0,0] entities
                                }
                                let name = if e.modelindex > 0 {
                                    cs.get(CS_MODELS + e.modelindex as usize).unwrap_or("?")
                                } else {
                                    "*?(brush)"
                                };
                                let d = [
                                    e.origin[0] - e.old_origin[0],
                                    e.origin[1] - e.old_origin[1],
                                    e.origin[2] - e.old_origin[2],
                                ];
                                tracing::info!(
                                    sf = frame.serverframe,
                                    ent = e.number,
                                    mi = e.modelindex,
                                    model = name,
                                    origin = ?[e.origin[0] as i32, e.origin[1] as i32, e.origin[2] as i32],
                                    dorigin = ?[d[0] as i32, d[1] as i32, d[2] as i32],
                                    "MOVER"
                                );
                            }
                        }

                        if p28_debug && ticks.is_multiple_of(15) {
                            const CS_MODELS: usize = 32;
                            for e in &frame.entities {
                                if e.modelindex == 255 {
                                    let wield = if e.modelindex2 > 0 {
                                        cs.get(CS_MODELS + e.modelindex2 as usize).unwrap_or("?")
                                    } else {
                                        "(modelindex2=0)"
                                    };
                                    tracing::info!(
                                        ent = e.number,
                                        mi2 = e.modelindex2,
                                        wield,
                                        "P28 PLAYER-ENT"
                                    );
                                }
                            }
                        }

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
                                    // next frame's playerstate re-feed both. `on_death`
                                    // also eases the Eraser auto-skill down.
                                    brain.on_death();
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
                                brain.on_kill();
                                stats.record_kill(name);
                            }
                        }
                        last_frags = Some(current_frags);

                        // Plan 61 (`xg`): push this frame's PVS threats into the navigator's
                        // danger pricing (defaulted no-op on every other backend). Rockets/
                        // grenades price hot lines; visible enemies price contested ground.
                        if let Some(nav) = nav_driver.as_mut() {
                            use brain::EntityClass;
                            let dangers: Vec<brain::DangerSource> = view
                                .entities()
                                .filter(|e| !e.is_stale)
                                .filter_map(|e| match e.class {
                                    EntityClass::ProjectileRocket
                                    | EntityClass::ProjectileGrenade => {
                                        Some(brain::DangerSource {
                                            pos: e.origin,
                                            rating: 300.0,
                                        })
                                    }
                                    EntityClass::EnemyPlayer => Some(brain::DangerSource {
                                        pos: e.origin,
                                        rating: 150.0,
                                    }),
                                    _ => None,
                                })
                                .collect();
                            if !dangers.is_empty() {
                                nav.note_dangers(&dangers);
                            }
                        }

                        // Drain server prints once per frame. First classify our own
                        // environmental suicides (lava/slime/drown/squish/…) from the
                        // obituary — the wire carries no means-of-death, only the print —
                        // then hand the same lines to the heatmap observer below.
                        let prints = conn.drain_prints();
                        for text in &prints {
                            // Raw print visibility (debug): obituary grammar differs per
                            // server/mod — this is the lens for verifying classification.
                            tracing::debug!(text = %text.trim_end(), "svc_print");
                            if let Some(kind) = brain::classify_env_death(text, name) {
                                tracing::warn!(kind = kind.name(), "EVT env_suicide");
                                stats.record_env_suicide(name, kind);
                            }
                        }

                        // Plan 08 heatmap: observe this frame (presence + obituary
                        // prints), advance decay, and refresh the risk overlay the
                        // nav driver consumes when it next plans a goal. This is the
                        // strategic layer; the tactical projectile dodge (below)
                        // composes by overriding movement for a single frame.
                        if let Some(obs) = heatmap_obs.as_mut() {
                            const HEATMAP_DT: f32 = 0.1; // 10 Hz client tick
                            obs.tick(HEATMAP_DT);
                            obs.sample_presence(&view, &cs, HEATMAP_DT, frame.serverframe);
                            for text in &prints {
                                obs.on_print(text, name, frame.serverframe);
                            }
                            let (w_danger, w_pop) = brain.heatmap_weights();
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

                        // Measured frame delta for turn-rate limiting (Open Q1, Plan 12).
                        let current_sf = frame.serverframe;
                        let dt = if let Some(prev_sf) = last_serverframe {
                            let sf_delta = (current_sf - prev_sf).max(0) as f32;
                            (sf_delta * 0.1).clamp(0.02, 0.3)
                        } else {
                            0.1
                        };
                        last_serverframe = Some(current_sf);

                        // The brain owns all per-frame decisions (Plan 22): combat, FSM,
                        // goal selection, steering, stuck recovery, jump-edge, dodge. The
                        // nav driver is injected (used, never owned); the brain returns a
                        // MovementIntent + an optional weapon switch.
                        let out = brain.tick(BrainContext {
                            view: &view,
                            nav: nav_driver.as_deref_mut().map(|n| n as &mut dyn Navigator),
                            cm: collision.as_deref(),
                            dt,
                            ticks,
                            // The live fleet brain drives its own FSM/item/roam goal ladder.
                            goal_override: None,
                        });

                        // Plan 51: feed the wall-press/stall detector. The wall probe
                        // only runs on hindered ticks (pushing but not moving).
                        {
                            let ss = view.self_state();
                            let intent = &out.intent;
                            let intent_mag =
                                (intent.forward * intent.forward + intent.side * intent.side)
                                    .sqrt();
                            let speed_h = ss.velocity.truncate().length();
                            let hindered = intent_mag > brain::stall::INTENT_MIN
                                && speed_h < brain::stall::SPEED_STALL;
                            let wall_blocked = hindered
                                && collision.as_deref().is_some_and(|cmod| {
                                    brain::stall::wish_blocked(
                                        cmod,
                                        ss.origin,
                                        intent.yaw,
                                        intent.forward,
                                        intent.side,
                                    )
                                });
                            // Nearest other player in PVS — ~33 u = hulls in contact,
                            // i.e. the stall is a bot-vs-bot block, not world geometry.
                            let nearest_player = view
                                .entities()
                                .filter(|e| e.class == brain::EntityClass::EnemyPlayer)
                                .map(|e| (e.origin - ss.origin).length())
                                .fold(f32::INFINITY, f32::min);
                            if let Some(ep) = stall_mon.tick(brain::StallSample {
                                pos: ss.origin,
                                speed_h,
                                intent_mag,
                                attacking: intent.attack,
                                wall_blocked,
                                damage: dmg_this_tick,
                                alive: ss.health > 0,
                                dt,
                                nearest_player,
                            }) {
                                let pp = if ep.min_player_dist.is_finite() {
                                    ep.min_player_dist as i32
                                } else {
                                    -1
                                };
                                tracing::debug!(
                                    bot = name,
                                    secs = %format!("{:.1}", ep.secs),
                                    speed = ep.mean_speed as i32,
                                    ticks = ep.ticks,
                                    atk = ep.attack_ticks,
                                    wall = ep.wall_ticks,
                                    dmg = ep.damage,
                                    pp,
                                    died = ep.died,
                                    x = ep.start_pos.x as i32,
                                    y = ep.start_pos.y as i32,
                                    z = ep.start_pos.z as i32,
                                    "EVT wall_press"
                                );
                            }
                        }

                        // Request a weapon switch via `use <name>` stringcmd (Q2 ignores
                        // impulse). Queued as a reliable message; flushed on transmit.
                        if let Some(w) = out.weapon_request {
                            conn.queue_stringcmd(&format!("use {}", w.name()));
                        }

                        move_ctrl.set_msec(dt);
                        move_ctrl.build_cmd(out.intent)
                    } else {
                        Usercmd::default()
                    }
                } else {
                    Usercmd::default()
                };

                // Plan 57: cache this decision so the recv arm can ack the next frame with
                // it, and demote the timer to a keepalive. When Active we only transmit
                // here if no frame-triggered send happened in the last ~90 ms (frames
                // stalled); while handshaking we always send so queued reliables flush.
                last_cmd = Some(cmd);
                let now = Instant::now();
                let keepalive_due = conn.state() != ConnState::Active
                    || now.duration_since(last_send) >= KEEPALIVE_GAP;
                if keepalive_due {
                    if let Some(pkt) = conn.transmit_cmd(&cmd) {
                        let _ = sock.send(&pkt).await;
                        last_send = now;
                    }
                }

                if ticks.is_multiple_of(10) {
                    match conn.frame.as_ref() {
                        Some(f) => {
                            let o = f.playerstate.pmove.origin_f32();

                            tracing::debug!(
                                state = ?conn.state(),
                                frame = f.serverframe,
                                ents = f.entities.len(),
                                "origin=({:.1},{:.1},{:.1}) fsm={}",
                                o[0], o[1], o[2],
                                brain.status()
                            );
                        }
                        None => tracing::debug!(state = ?conn.state(), "(no frame yet)"),
                    }

                    // Plan 57: report the self-inflicted frame-arrival→ack phase delay.
                    // Near-zero ema/max here means the ack-on-frame path is working; the
                    // server's scoreboard ping should track ≈ true RTT. `late` counts acks
                    // that slipped past ~40 ms (a starved loop / a stalled frame stream).
                    let st = send_timing.snapshot();
                    if st.sends > 0 {
                        tracing::debug!(
                            bot = name,
                            ema = %format!("{:.1}", st.ema_ms),
                            max = %format!("{:.1}", st.max_ms),
                            sends = st.sends,
                            late = st.late,
                            "EVT send_timing"
                        );
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
    spacing: f32,
    mode: NavMode,
    brain: brain::BrainKind,
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

    // Detect the server's map AND validate its nav cache up front — once, before any
    // bot is spawned. Without this, each of the N staggered bot tasks would discover a
    // missing cache independently, surfacing the error seconds into the run. `--map` is
    // only an override (a mismatch produces garbage navigation, per AGENTS.md).
    let map = match preflight_map(cfg, addr, map.as_deref(), spacing, true).await {
        Ok(m) => Some(m),
        Err(code) => return code,
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
                spacing,
                mode,
                brain,
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

    let built = match world::generate_map_nav(&cfg.paths.baseq2, map, world::GRID_SPACING) {
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
#[allow(clippy::too_many_arguments)]
fn generate_map_cache(
    cfg: &Config,
    map_arg: &str,
    jobs: Option<usize>,
    out_dir: &str,
    spacing: f32,
    allow_failures: bool,
) -> ExitCode {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

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
    // (map, reason) for every map that did NOT cache — printed in the end summary so a
    // batch failure names exactly which maps broke and why.
    let failures: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

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
            let failures = &failures;
            let baseq2 = cfg.paths.baseq2.clone();
            let out_path = out_path.to_path_buf();
            handles.push(scope.spawn(move || {
                for map in &chunk {
                    let t0 = std::time::Instant::now();
                    let built = match world::generate_map_nav(
                        &baseq2,
                        map,
                                    spacing,
                    ) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!(map, "generate failed: {e}");
                            failed.fetch_add(1, Ordering::Relaxed);
                            failures
                                .lock()
                                .unwrap()
                                .push((map.clone(), "generate failed".into()));
                            continue;
                        }
                    };
                    // Don't cache a broken graph — fail the map so the caller knows. EXCEPT
                    // under --allow-failures: still write the cache (with a loud warning) so a
                    // partially-connected map (e.g. q2dm3 pending full Plan 35 connectivity) is
                    // usable by the movement-test harness, which only needs spawn→goal reach.
                    if let Err(diag) = world::check_spawn_connectivity(&built) {
                        if !allow_failures {
                            tracing::error!(map, "{diag}");
                            failed.fetch_add(1, Ordering::Relaxed);
                            let reason = format!(
                                "spawns not all reachable ({}/{} in largest component)",
                                built.in_largest, built.total_spawns
                            );
                            failures.lock().unwrap().push((map.clone(), reason));
                            continue;
                        }
                        tracing::warn!(
                            map,
                            "caching partially-connected graph under --allow-failures ({}/{} spawns in largest component): {diag}",
                            built.in_largest,
                            built.total_spawns
                        );
                    }
                    let fp =
                        world::Fingerprint::from_bsp(&built.bsp, spacing);
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
                            failures
                                .lock()
                                .unwrap()
                                .push((map.clone(), format!("save failed: {e}")));
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
    let total = ok + err;
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    tracing::info!(ok, err, total, "generate-map-cache complete");
    // Name every failed map + reason so a batch failure is actionable, not a bare count.
    for (map, reason) in &failures {
        tracing::warn!(map = %map, "FAILED: {reason}");
    }

    // Exit semantics: by default any failure is non-zero (CI / single-map strictness). With
    // --allow-failures, a multi-map batch succeeds as long as at least one map cached — so a
    // `q2dm*` run isn't sunk by one known-broken map while the others cache fine.
    if err == 0 {
        ExitCode::SUCCESS
    } else if allow_failures && ok > 0 {
        tracing::warn!(
            ok,
            err,
            "completed with --allow-failures: {ok} cached, {err} failed (see FAILED lines above)"
        );
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing subscriber with elapsed time formatting and abbreviated levels
    let start_time = Instant::now();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Color the level letters only when writing to a terminal; piped/redirected
    // output (e.g. `> run.log`) stays free of ANSI escapes.
    let ansi = std::io::IsTerminal::is_terminal(&std::io::stdout());
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_timer(ElapsedFormatter(start_time))
        .with_target(false)
        .with_thread_ids(false)
        // Span fields (the `[name=… qport=…]` prefix) are formatted at span-record time
        // by the LAYER's field formatter — align its ANSI mode with the event formatter's
        // or piped logs get escape codes inside the brackets.
        .with_ansi(ansi)
        .event_format(AbbreviatedFormat::new(start_time, ansi))
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
            brain,
            char,
            persona,
            xonchar,
        } => {
            // Resolve the persona name (Plan 27) → a preset; unknown names are a hard error so a
            // typo isn't silently ignored.
            let persona = match persona.as_deref().map(brain::persona::Persona::preset) {
                Some(Some(p)) => Some(p),
                Some(None) => {
                    tracing::error!(
                        "unknown --persona (want one of: {})",
                        brain::persona::Persona::PRESET_NAMES.join(", ")
                    );
                    return ExitCode::FAILURE;
                }
                None => None,
            };
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
            // Detect + validate the server's map before connecting (fatal on miss).
            if let Err(code) = preflight_map(&cfg, addr, None, world::GRID_SPACING, false).await {
                return code;
            }
            tracing::info!("connecting '{name}' to {addr} (qport {qport})…  Ctrl-C to stop.");

            match supervisor::run_single(
                &cfg, addr, &name, qport, mode, brain, char, persona, xonchar,
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
        Cmd::Run {
            addr,
            mode,
            brain,
            name,
            count,
            qport_base,
            skin,
            skin_random_male,
            skin_random_female,
            char,
            loose_botcap,
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
            // CLI `--brain` overrides `[fleet].brain` (which defaults to `main`).
            let brain = brain.unwrap_or_else(|| cfg.fleet.brain_kind());
            tracing::info!(brain = brain::brain_tag(brain), "fleet brain selection");
            // CLI `--char` overrides `[fleet].char`; only meaningful for `--brain q3`.
            let char = char.or_else(|| cfg.fleet.char_preset());
            if let Some(q) = char {
                tracing::info!(char = q.tag(), "fleet q3 character");
            }
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            // Detect + validate the server's map before spawning the fleet (fatal on
            // miss) — one up-front gate instead of N bots each failing to build nav.
            if let Err(code) = preflight_map(&cfg, addr, None, world::GRID_SPACING, false).await {
                return code;
            }
            match supervisor::run_fleet(
                Arc::new(cfg),
                addr,
                mode,
                brain,
                name,
                count,
                qport_base,
                skin_sel,
                char,
                loose_botcap,
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
        Cmd::Competition {
            addr,
            count,
            modes,
            brains,
            chars,
            xonchars,
            qport_base,
            loose_botcap,
        } => {
            if count == 0 {
                tracing::error!("--count must be >= 1");
                return ExitCode::FAILURE;
            }
            // clap already validated each token against the enums and rendered the possible
            // values in `--help`; here we only apply the empty-list defaults and reject the
            // one brain that can't compete.
            let modes: Vec<NavMode> = if modes.is_empty() {
                NavMode::value_variants().to_vec()
            } else {
                modes
            };
            let brains: Vec<brain::BrainKind> = if brains.is_empty() {
                vec![brain::BrainKind::Main]
            } else {
                // `runtester` is the combat-free movement-scenario brain — it never fires or
                // frags, so it's meaningless on a frag scoreboard.
                if brains.contains(&brain::BrainKind::RunTester) {
                    tracing::error!(
                        "'runtester' is a non-combat brain and cannot compete (drop it from --brains)"
                    );
                    return ExitCode::FAILURE;
                }
                brains
            };
            // Q3 personality roster (only fields the `q3` brain). Empty → one default-character group.
            // One distinct skin per mode so the fleets are tellable apart on sight.
            let mut rng = skins::Rng::new();
            let skins_per_mode: Vec<Option<String>> =
                skins::distinct_skins(&cfg.paths.baseq2, modes.len(), &mut rng)
                    .into_iter()
                    .map(Some)
                    .collect();
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            // Detect + validate the server's map before launching the competing fleets.
            if let Err(code) = preflight_map(&cfg, addr, None, world::GRID_SPACING, false).await {
                return code;
            }
            match supervisor::run_competition(
                Arc::new(cfg),
                addr,
                modes,
                brains,
                chars,
                xonchars,
                count,
                qport_base,
                skins_per_mode,
                loose_botcap,
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
        Cmd::SpawnToSpawn {
            map,
            addr,
            name,
            count,
            max_secs,
            spacing,
            mode,
            brain,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::FarthestSpawn,
                count,
                max_secs,
                spacing,
                mode,
                brain,
            )
            .await
        }
        Cmd::SpawnToWeapon {
            weapon_name,
            instance,
            map,
            addr,
            name,
            count,
            max_secs,
            spacing,
            mode,
            brain,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::Weapon {
                    name: weapon_name,
                    instance,
                },
                count,
                max_secs,
                spacing,
                mode,
                brain,
            )
            .await
        }
        Cmd::SpawnToItem {
            item_name,
            instance,
            map,
            addr,
            name,
            count,
            max_secs,
            spacing,
            mode,
            brain,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::Item {
                    name: item_name,
                    instance,
                },
                count,
                max_secs,
                spacing,
                mode,
                brain,
            )
            .await
        }
        Cmd::SpawnToPoint {
            x,
            y,
            z,
            map,
            addr,
            name,
            count,
            max_secs,
            spacing,
            mode,
            brain,
        } => {
            run_scenario_cmd(
                &cfg,
                addr,
                name,
                map,
                scenario::ScenarioGoal::Point { x, y, z },
                count,
                max_secs,
                spacing,
                mode,
                brain,
            )
            .await
        }
        Cmd::NavDebug { map, pairs } => nav_debug(&cfg, &map, pairs),
        Cmd::GenerateMapCache {
            map,
            jobs,
            out_dir,
            spacing,
            allow_failures,
        } => generate_map_cache(&cfg, &map, jobs, &out_dir, spacing, allow_failures),
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
    fn navmode_accepts_short_code_and_long_alias() {
        use clap::ValueEnum;
        // Canonical CLI token is the short code…
        assert_eq!(NavMode::from_str("rc", true), Ok(NavMode::HybridRace));
        assert_eq!(NavMode::from_str("as", true), Ok(NavMode::Astar));
        assert_eq!(NavMode::from_str("sg", true), Ok(NavMode::HybridSegment));
        // …and the long kebab name still parses as an alias.
        assert_eq!(
            NavMode::from_str("hybrid-race", true),
            Ok(NavMode::HybridRace)
        );
        assert_eq!(NavMode::from_str("astar", true), Ok(NavMode::Astar));
        assert_eq!(
            NavMode::from_str("hybrid-segment", true),
            Ok(NavMode::HybridSegment)
        );
        assert!(NavMode::from_str("nope", true).is_err());
    }

    #[test]
    fn first_event_emits_no_coda() {
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::ZERO, FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
    }

    #[test]
    fn stale_earlier_elapsed_does_not_underflow() {
        // Under concurrent logging, `elapsed` is sampled before the lock, so a thread can
        // observe a value earlier than the `first_seen` another thread already advanced.
        // The subtraction must saturate to 0, not panic ("overflow when subtracting durations").
        let mut d = Deduper::default();
        assert_eq!(
            d.observe("D msg", Duration::from_secs(10), FLUSH),
            DedupAction::Emit { prev_coda: None }
        );
        // Same key, but an earlier timestamp than first_seen (10s) → must just suppress.
        assert_eq!(
            d.observe("D msg", Duration::from_secs(3), FLUSH),
            DedupAction::Suppress { coda: None }
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

//! Movement-quality scenarios — drive one bot to a known goal and record it.
//!
//! Plan 10's measurement lens: `spawn-to-spawn` / `spawn-to-weapon` connect a single
//! bot like `connect-one`, but pin its nav goal to a scenario target, **disable
//! combat**, and feed every server frame to a [`MovementRecorder`]. The run stops on
//! goal-reach (settled), a `max_secs` cap, or a disconnect, then dumps a structured
//! log + prints the SUMMARY line that Plans 11–14 must beat.
//!
//! This deliberately reuses the brain's nav/steering primitives (it does **not**
//! duplicate combat/aim logic) — only the connect + tick scaffolding is mirrored
//! from [`crate::bot_task`]. It never sets velocity or teleports the bot, so a log
//! showing sustained > ~320 u/s grounded speed flags a physics bug, not a feature.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use tokio::net::UdpSocket;

use brain::nav::NavGoal;
use brain::perception::{ModelTable, MotionTracker, Worldview};
use brain::recorder::{CmWallProbe, MovementRecorder, Sample, WallProbe};
use brain::{
    build_brain, BotSkill, Brain, BrainConfig, BrainContext, BrainKind, BrainMap,
    MovementController, Navigator,
};
use client::{Conn, ConnState};
use q2proto::{Usercmd, PM_FREEZE};
use world::NavGraph;

use crate::config::Config;
use crate::supervisor::{spawn_signal_listener, Shutdown};

/// `PMF_ON_GROUND` (`shared.h:646`) — the bot's pmove grounded bit.
const PMF_ON_GROUND: u32 = 4;
/// Within this 3D distance of the goal, the bot has "reached" it.
const GOAL_TOL: f32 = brain::recorder::GOAL_TOL;
/// A reach only counts once held this long (filters fly-through jitter).
const GOAL_SETTLE: f32 = 0.5;

/// What a scenario drives toward.
#[derive(Clone)]
pub enum ScenarioGoal {
    /// The DM spawn point farthest (3D) from where the bot spawns.
    FarthestSpawn,
    /// A named weapon's BSP origin (e.g. `rocketlauncher` → `weapon_rocketlauncher`).
    /// `instance` selects among multiple matches (q2dm3 has two `weapon_railgun`).
    Weapon { name: String, instance: usize },
    /// A named item's BSP origin, resolved through [`item_classname`] aliases
    /// (e.g. `quaddamage` → `item_quad`). `instance` selects among multiple matches.
    Item { name: String, instance: usize },
    /// An arbitrary world coordinate — used to ISOLATE a single nav feature (e.g. drive to a
    /// func_train's board ledge) without the full item route, so route-reliability and
    /// ride-correctness can be measured separately (Plan 35 T3).
    Point { x: f32, y: f32, z: f32 },
}

/// Resolve a friendly item name to its Q2 entity classname.
///
/// The classnames the engine uses don't always match what an operator types — the quad is
/// `item_quad`, not `item_quaddamage`. This maps common aliases; anything already prefixed
/// `item_` passes through, and anything else gets an `item_` prefix.
pub fn item_classname(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        "quad" | "quaddamage" | "quad_damage" => "item_quad",
        "invuln" | "invulnerability" | "invulnerable" => "item_invulnerability",
        "mega" | "megahealth" | "mega_health" => "item_health_mega",
        "redarmor" | "bodyarmor" | "red_armor" | "body_armor" => "item_armor_body",
        "yellowarmor" | "combatarmor" | "combat_armor" => "item_armor_combat",
        "greenarmor" | "jacketarmor" | "jacket_armor" => "item_armor_jacket",
        "silencer" => "item_silencer",
        "adrenaline" => "item_adrenaline",
        "bandolier" => "item_bandolier",
        "pack" | "ammopack" | "ammo_pack" => "item_pack",
        other if other.starts_with("item_") => return other.to_string(),
        other => return format!("item_{other}"),
    }
    .to_string()
}

/// Why a run measured NOTHING about locomotion: the server, not the bot, decided
/// the outcome. A Deferred run must never be counted as a failure — and never as a
/// success either; its data is void (the goal coordinates belong to a map we left,
/// the freeze window had no locomotion in it). Deliberately only two variants: a
/// bot that never went Active or produced no frames is a SetupError (possibly OUR
/// broken client), and routing that into "the server voided it" would bury a real
/// bug under someone else's excuse.
#[derive(Debug, Clone, PartialEq)]
pub enum DeferredReason {
    /// The level went away mid-run: server broadcast `reconnect` / fresh
    /// `svc_serverdata` with a different `servercount` / netchan reset (which nulls
    /// `serverdata` before the new one lands — `None` is the same event) / hard
    /// disconnect after we were Active. Vendor: `sv_init.c:264` bumps
    /// `svs.spawncount` per level spawn, `:654` broadcasts `reconnect`.
    LevelChange,
    /// Frozen by an intermission (`PM_FREEZE`). Under the STOCK yquake2 gamecode a
    /// scenario bot can ONLY freeze in intermission: `PM_DEAD` on death (`client.c:767`),
    /// `PM_NORMAL` on respawn (`:2158`), `PM_FREEZE` only while `level.intermissiontime`
    /// (`:2119`) or at the intermission teleport (`hud.c:47`) — and unlike the fleet
    /// (Plan 64) the scenario bot never presses a button, so per `client.c:2122` it
    /// stays frozen to the cap. A third writer exists, `g_chase.c:120`, excluded by
    /// construction: chase cam needs a `clc_stringcmd`, which scenarios never send.
    /// A MODDED gamecode could freeze for another reason and would be tagged
    /// `intermission` — the run is still correctly voided, only the tag misattributes.
    /// Hence ANY freeze frame voids (no ratio); `t_secs` is first-observed onset, for
    /// diagnosis only.
    Intermission { t_secs: f32 },
}

impl DeferredReason {
    /// Machine-readable tag for the `# RESULT` line and the aggregate, so grepping
    /// `logs/` days later never has to parse prose.
    fn tag(&self) -> &'static str {
        match self {
            Self::LevelChange => "level-change",
            Self::Intermission { .. } => "intermission",
        }
    }

    /// Extra `key=value` detail for the `# RESULT` line only; the aggregate groups
    /// by [`Self::tag`], so the onset time never fragments a tally.
    fn detail(&self) -> String {
        match self {
            Self::Intermission { t_secs } => format!(" frozen_from_t={t_secs:.1}"),
            Self::LevelChange => String::new(),
        }
    }
}

/// The one-word verdict of a single scenario run. Replaces the old boolean
/// reached/fail: a run that learned nothing no longer shares an outcome (or an exit
/// code) with a run that legitimately failed to move.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenarioVerdict {
    /// Held the goal within tolerance for the settle window. Genuine success — and
    /// immune to the contamination sources polled here (the run breaks the instant
    /// this is true, so no later server event can retroactively void it). Frame
    /// STALLS are not among the polled sources: a link repeating a stale in-tolerance
    /// playerstate could still credit this — follow-up recorded in `context/pitfalls.md`.
    Reached,
    /// A full, uncontaminated window that ended without reaching. The only outcome
    /// that counts against movement.
    Failed,
    /// Measurement void — see [`DeferredReason`].
    Deferred(DeferredReason),
    /// Setup/IO error (bad config, nav cache, never-Active connection). Terminal
    /// failure, distinct from Deferred: this is OUR bug surface, not the server's.
    SetupError,
}

impl ScenarioVerdict {
    /// Machine tag for per-bot log lines (`reached|failed|deferred|setup-error`).
    pub fn log_tag(&self) -> &'static str {
        match self {
            Self::Reached => "reached",
            Self::Failed => "failed",
            Self::Deferred(_) => "deferred",
            Self::SetupError => "setup-error",
        }
    }

    /// Suffix baked into the log FILENAME so `ls logs/` alone distinguishes a voided
    /// run from a failed one (a stale archive must be self-explaining).
    pub fn filename_tag(&self) -> &'static str {
        match self {
            Self::Reached => "reached",
            Self::Failed => "failed",
            Self::Deferred(_) => "deferred",
            Self::SetupError => "error",
        }
    }
}

/// Cross-bot tally for a `--count N` run. Exit precedence is computed from these
/// counters, never from arrival order: `SetupError > 3 > 2 > 0` — the old
/// "adopt the first non-success code" rule made a 24-bot exit a coin flip of which
/// bot finished first.
#[derive(Debug, Default)]
pub struct BatchTally {
    pub reached: usize,
    pub failed: usize,
    /// Per-tag deferred counts (first-seen tag order for display).
    pub deferred: Vec<(DeferredReason, usize)>,
    pub setup_errors: usize,
}

impl BatchTally {
    pub fn add(&mut self, v: &ScenarioVerdict) {
        match v {
            ScenarioVerdict::Reached => self.reached += 1,
            ScenarioVerdict::Failed => self.failed += 1,
            ScenarioVerdict::SetupError => self.setup_errors += 1,
            ScenarioVerdict::Deferred(r) => {
                // Group by TAG, not full reason equality: two intermission deferrals
                // at different onset times are ONE bucket here (the per-bot log line
                // keeps its own exact onset). Matching on `PartialEq` would fragment a
                // 24-bot batch into a dozen "deferred" entries and read like chaos.
                match self.deferred.iter_mut().find(|(k, _)| k.tag() == r.tag()) {
                    Some((_, n)) => *n += 1,
                    None => self.deferred.push((r.clone(), 1)),
                }
            }
        }
    }

    pub fn deferred_total(&self) -> usize {
        self.deferred.iter().map(|(_, n)| n).sum()
    }

    /// Runs that produced a USABLE locomotion datapoint (reached or failed — a
    /// deferred run counted against neither). Printing this beats making a reader
    /// mentally subtract from N: `13/16` is the number that means "movement",
    /// `13/24` is the number that got contaminated.
    pub fn effective_denominator(&self) -> usize {
        self.reached + self.failed
    }

    /// Batch exit code. Precedence ladder: a setup error dominates (our bug); else
    /// zero-usable-samples ⇒ 3 (nothing was learned — CI must distinguish
    /// "unmeasurable" from "broken"); else any genuine failure ⇒ 2 (measured); else
    /// 0. A run with reached>0 AND failed>0 is "the goal is reachable, some bots
    /// didn't make it" ⇒ 2, same as the old contract.
    pub fn exit_code(&self) -> ExitCode {
        if self.setup_errors > 0 {
            return ExitCode::FAILURE;
        }
        if self.effective_denominator() == 0 {
            return ExitCode::from(3);
        }
        if self.failed > 0 {
            return ExitCode::from(2);
        }
        ExitCode::SUCCESS
    }
}

impl std::fmt::Display for BatchTally {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Effective fraction FIRST: it is the answer to "did movement regress?", and
        // `acceptance.rs::parse_reached` parses it as the token after `): `. Prose last.
        let ed = self.effective_denominator();
        if ed > 0 {
            write!(
                f,
                "effective {}/{} ({:.0}%)",
                self.reached,
                ed,
                100.0 * self.reached as f64 / ed as f64
            )?;
        } else {
            write!(f, "NO effective runs — nothing learned about movement")?;
        }
        write!(f, " | {} reached, {} failed", self.reached, self.failed)?;
        let dt = self.deferred_total();
        if dt > 0 {
            write!(f, ", {dt} deferred by SERVER (")?;
            for (i, (r, n)) in self.deferred.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{n} {}", r.tag())?;
            }
            write!(f, ")")?;
        }
        if self.setup_errors > 0 {
            // The line must never read clean while the process exits FAILURE.
            write!(f, ", {} setup-error", self.setup_errors)?;
        }
        Ok(())
    }
}

/// Run a movement scenario: connect one bot, drive it to `goal`, record + dump.
/// Returns the run's [`ScenarioVerdict`]: `Reached`/`Failed` are real measurements;
/// `Deferred(reason)` means the server voided the datapoint (intermission, level
/// change, or a post-Active drop) and it must not be counted either way;
/// `SetupError` is a setup/IO failure including a bot that never became Active.
#[allow(clippy::too_many_arguments)]
pub async fn run_scenario(
    cfg: &Config,
    addr: SocketAddr,
    name: &str,
    map_arg: Option<&str>,
    goal_kind: ScenarioGoal,
    max_secs: f32,
    qport: u16,
    // Grid spacing of the nav graph to load/build (`--spacing`); cached per-spacing.
    spacing: f32,
    // Navigation backend (`--navmode`): the `astar` waypoint graph or the `navmesh` polygon mesh.
    mode: crate::NavMode,
    // Decision plugin (`--brain`): `runtester` (default — the lifted scenario pathfinder) or
    // `main` for an A/B against the live combat brain (combat is forced off here regardless).
    brain_kind: BrainKind,
) -> std::io::Result<ScenarioVerdict> {
    // The caller (`run_scenario_cmd`) autodetects the server's map and passes it here;
    // a `None` at this point means autodetection was skipped/failed, which is a bug,
    // not a reason to silently guess a map (a wrong map produces garbage navigation).
    let map = map_arg
        .ok_or_else(|| io_err("no map resolved for scenario (autodetect failed)".to_string()))?
        .to_string();

    // 1. Load BSP + build collision model + nav graph (cache-first, then live).
    let cache_dir = std::path::Path::new("data/mapcache");
    let built = world::cached_map_nav(&cfg.paths.baseq2, &map, Some(cache_dir), spacing)
        .map_err(|e| io_err(format!("can't build nav for '{map}': {e}")))?;

    // All Q2 dm maps guarantee full spawn reachability, so a fragmented graph is a nav bug.
    // For the *movement-test harness* this is a WARNING, not a fatal abort: a scenario only
    // needs the bot's spawn to reach the pinned goal (checked per-spawn below via A*), and we
    // want to be able to exercise goal-reaching (e.g. q2dm3's quad/railgun) while the broad
    // floor-connectivity work (Plan 35) is still in progress. The fleet/production path keeps
    // its own stricter gates.
    if let Err(diag) = world::check_spawn_connectivity(&built) {
        tracing::warn!(%map, "nav graph not fully spawn-connected (movement-test harness continues): {diag}");
    }

    let cm = Arc::clone(&built.cm);
    let bsp_spawns = built.spawn_origins.clone();
    let seeded = built.seeded;
    let added_jumps = built.added_jumps;
    let in_largest = built.in_largest;
    let total_spawns = built.total_spawns;
    let bsp = built.bsp;
    let mut graph = built.graph;

    // 2. Resolve the scenario label + (when known up front) the goal origin + the
    //    spawn origins for the lazy farthest-spawn pick. A weapon origin is known
    //    now; the farthest spawn is picked once we know where we spawned.
    //    Resolved early (before wrapping graph in Arc) so we can seed the goal.
    let (scenario_name, goal_origin, goal_label, spawn_origins) =
        resolve_goal(&bsp, &map, &goal_kind)?;

    // For a weapon goal, the scenario is only possible if some spawn can reach
    // the goal node. An isolated goal (stranded in a disconnected nav component)
    // makes the run impossible — a nav-graph bug per the all-locations-mutually-
    // reachable invariant. Default true; the spawn→goal A* sweep below sets it
    // for weapon goals (spawn goals are always reachable, so they stay true).
    let mut goal_reachable_from_spawn = true;

    // Seed the scenario goal position as an exact nav node when it isn't already one
    // of the DM spawns (T2). For FarthestSpawn the goal is always one of bsp_spawns
    // (already seeded); for Weapon the goal is a single weapon origin that may lie
    // between grid nodes, causing A* to snap to an imprecise neighbor on the wrong
    // side of a doorway or stair lip.
    if let Some(origin) = goal_origin {
        let extra = graph.seed_spawns(&cm, &[origin]);
        if extra > 0 {
            tracing::info!("seeded scenario goal into nav graph (+{extra} node(s))");
        }
        // Ensure the goal node is connected to the main component. The normal
        // BRIDGE_HDIST=128 may not reach a weapon node in an isolated floor pocket
        // (e.g. a high platform accessible via a staircase that is >128u horizontal
        // from the weapon origin itself). Run a wider-radius bridge from the goal
        // node specifically — walkable_stair still filters false connections.
        if let Some(goal_idx) = graph.nearest(&origin) {
            let bridged = graph.connect_node_to_nearby(&cm, goal_idx, 384.0);
            if bridged > 0 {
                tracing::info!(
                    goal_idx,
                    bridged,
                    "extended bridge: connected scenario goal to nearby nodes"
                );
            }
            // Quick connectivity check: can A* reach goal from spawn?
            let comps_check = graph.components();
            let goal_comp = comps_check
                .iter()
                .position(|c| c.contains(&goal_idx))
                .unwrap_or(999);
            tracing::info!(goal_idx, goal_comp, "weapon goal component after bridging");
            // Diagnostic: log adj neighbors of goal node (pos + count)
            let goal_pos = graph.nodes[goal_idx];
            let adj_count = graph.adj_count(goal_idx);
            let neighbor_zs = graph.adj_neighbor_z_levels(goal_idx);
            tracing::info!(
                goal_idx,
                goal_pos = ?[goal_pos[0] as i32, goal_pos[1] as i32, goal_pos[2] as i32],
                adj_count,
                neighbor_z_levels = ?neighbor_zs,
                "goal node adj info"
            );
            // Check A* from each spawn to goal; remember if *any* spawn reaches it.
            let mut any_reach = false;
            let mut logged_kinds = false;
            for (i, sp) in bsp_spawns.iter().enumerate() {
                if let Some(sp_idx) = graph.nearest(sp) {
                    let path = graph.path(sp_idx, goal_idx);
                    let can_reach = path.is_some();
                    any_reach |= can_reach;
                    let sp_pos = graph.nodes[sp_idx];
                    tracing::info!(
                        spawn = i,
                        sp_idx,
                        sp_pos = ?[sp_pos[0] as i32, sp_pos[1] as i32, sp_pos[2] as i32],
                        can_reach,
                        "spawn→goal A* check"
                    );
                    // Once, log the edge-kind composition of a winning path — tells us which
                    // special traversals (Ride/Jump/Swim) the brain must execute to arrive.
                    if let (false, Some(p)) = (logged_kinds, path) {
                        let (mut walk, mut jump, mut swim, mut ride, mut teleport) =
                            (0, 0, 0, 0, 0);
                        for w in p.windows(2) {
                            match graph.edge_kind(w[0], w[1]) {
                                world::EdgeKind::Walk => walk += 1,
                                world::EdgeKind::Jump { .. } => jump += 1,
                                world::EdgeKind::Swim => swim += 1,
                                world::EdgeKind::Ride => ride += 1,
                                world::EdgeKind::Teleport => teleport += 1,
                            }
                        }
                        tracing::info!(
                            spawn = i,
                            nodes = p.len(),
                            walk,
                            jump,
                            swim,
                            ride,
                            teleport,
                            "goal path edge-kind composition"
                        );
                        // Dump each ride edge's endpoints (board→dismount) so we can see the
                        // exact lift/train/ladder hops the route takes (Plan 35 quad debugging).
                        for w in p.windows(2) {
                            if matches!(graph.edge_kind(w[0], w[1]), world::EdgeKind::Ride) {
                                if let Some(ri) = graph.ride_info(w[0], w[1]) {
                                    let f = graph.nodes[w[0]];
                                    let t = graph.nodes[w[1]];
                                    tracing::info!(
                                        from = ?[f[0] as i32, f[1] as i32, f[2] as i32],
                                        to = ?[t[0] as i32, t[1] as i32, t[2] as i32],
                                        ladder = ri.ladder,
                                        vertical = ri.vertical,
                                        "  ride hop"
                                    );
                                }
                            }
                        }
                        logged_kinds = true;
                    }
                }
            }
            goal_reachable_from_spawn = any_reach;
        }
    }

    tracing::info!(count = bsp_spawns.len(), "bsp spawn points collected");
    for (i, sp) in bsp_spawns.iter().enumerate() {
        tracing::info!("  spawn[{}]: ({}, {}, {})", i, sp[0], sp[1], sp[2]);
    }

    // Diagnostic component logging.
    let comps = graph.components();
    if comps.len() > 1 {
        tracing::warn!(
            count = comps.len(),
            "nav graph has multiple disconnected components - THIS IS A BUG"
        );
        for (i, c) in comps.iter().take(5).enumerate() {
            let (mut mnx, mut mny, mut mnz) = (f32::MAX, f32::MAX, f32::MAX);
            let (mut mxx, mut mxy, mut mxz) = (f32::MIN, f32::MIN, f32::MIN);
            for &ni in c {
                let p = graph.nodes[ni];
                mnx = mnx.min(p[0]);
                mxx = mxx.max(p[0]);
                mny = mny.min(p[1]);
                mxy = mxy.max(p[1]);
                mnz = mnz.min(p[2]);
                mxz = mxz.max(p[2]);
            }
            tracing::warn!(
                "  component[{}]: {} nodes bbox x={:.0}..{:.0} y={:.0}..{:.0} z={:.0}..{:.0}",
                i,
                c.len(),
                mnx,
                mxx,
                mny,
                mxy,
                mnz,
                mxz
            );
        }
        for (i, sp) in bsp_spawns.iter().enumerate() {
            if let Some(nearest_idx) = graph.nearest(sp) {
                let comp_idx = comps
                    .iter()
                    .position(|c| c.contains(&nearest_idx))
                    .unwrap_or(999);
                tracing::info!(
                    "  spawn[{}] at ({}, {}, {}) -> nearest node {} -> component {}",
                    i,
                    sp[0],
                    sp[1],
                    sp[2],
                    nearest_idx,
                    comp_idx
                );
            }
        }
    } else {
        tracing::info!("nav graph is fully connected (single component)");
    }
    tracing::info!(
        map,
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        seeded,
        added_jumps,
        in_largest,
        total_spawns,
        "scenario nav graph"
    );

    // Hard abort: if no spawn can reach the goal, the scenario is impossible.
    // All Q2 dm map locations are mutually reachable by design — an isolated
    // goal node is a bug in BSP parsing / collision / nav generation, never a
    // legitimate map property. Diagnostics above have already been dumped.
    if !goal_reachable_from_spawn {
        crate::fatal!(
            %map,
            goal = %goal_label,
            "scenario goal unreachable from every spawn — nav graph bug (goal node isolated); aborting before connecting"
        );
    }

    let graph = Arc::new(graph);

    // 3. Connect the bot (the same handshake `connect-one` uses).
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);
    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }
    tracing::info!(%name, %map, %addr, qport, "scenario bot connected; driving to goal");

    // 4. Decision brain (Plan 26) + recorder scaffolding.
    let mut move_ctrl = MovementController::new();
    // The scenario runs a `Box<dyn Brain>` (default `runtester` — the lifted pathfinder).
    // Combat is forced off; the goal is pinned per-tick via `BrainContext::goal_override`.
    let mut brain: Box<dyn Brain + Send> = build_brain(
        brain_kind,
        BotSkill::default(),
        BrainConfig {
            combat_enabled: false,
        },
        None, // scenarios don't select a Q3 personality (no combat)
        None, // ...nor a main persona (combat off)
        None, // ...nor an xon character (Plan 60; neutral skill is fine goal-driven)
    );

    // Drive through the `Navigator` trait so the tick loop is backend-agnostic. `+ Send`
    // because this future is spawned on tokio and holds the driver across awaits.
    // Reuse one process-wide navmesh across all bots (built from the same collision model the
    // A* graph used). The factory builds it lazily — only for the navmesh + hybrid modes.
    let mut nav_driver: Box<dyn Navigator + Send> =
        crate::build_navigator(mode, Arc::clone(&graph), || {
            let model = &bsp.models[0];
            crate::supervisor::get_or_build_navmesh(&map, &cm, (model.mins, model.maxs))
        });
    // `runtester` ignores the map (it drives the injected nav); `--brain main` uses it for the
    // navmesh roam-as-position flag (its roam ladder is moot here — `goal_override` always wins).
    brain.set_map(BrainMap {
        roam_nodes: Vec::new(),
        nav_graph: Arc::clone(&graph),
        roam_as_position: matches!(mode, crate::NavMode::Navmesh),
        // Static item table (Plan 30) — populated for `--brain main` A/B runs; combat is off in
        // scenarios so health-seek never fires, but keeping it consistent avoids a divergent path.
        items: brain::items::build_map_items(&bsp, &graph),
    });
    let mut last_serverframe: Option<i32> = None;
    // Monotonic tick counter for `BrainContext` (drives jitter/roam in the `--brain main` A/B;
    // the runtester ignores it).
    let mut tick_count: u32 = 0;
    let shutdown = Shutdown::new();
    let _signals = spawn_signal_listener(shutdown.clone());

    let now = time::OffsetDateTime::now_utc();
    let unix_ts = now.unix_timestamp().max(0) as u64;
    // Build the ISO-8601 label from components (avoids the time crate's
    // feature-gated `format_description` path).
    let started_iso = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    );
    let probe: Arc<dyn WallProbe> = Arc::new(CmWallProbe::new(Arc::clone(&cm)));

    let mut recorder: Option<MovementRecorder> = None;
    let mut buf = vec![0u8; 4096];
    // Plan 57 opt-out: this movement harness deliberately keeps the free-running 100 ms
    // send (no ack-on-frame re-phasing). The Plan 10–14 baselines in
    // `10_movement_test_harness_tracker.md` were recorded against this exact cadence, so
    // re-phasing the send here could shift mean_speed/elapsed and invalidate them. Ping
    // is irrelevant to movement measurement; the fleet loop (`main.rs`) carries the fix.
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    let start = Instant::now();
    let mut goal_settle_start: Option<f32> = None;
    // Farthest-spawn goal, resolved lazily on the first active frame.
    let mut resolved_goal: Option<[f32; 3]> = goal_origin;
    let mut reached = false;
    // Set only on the shutdown break below: an operator-interrupted run is OUR teardown,
    // not the bot's locomotion or the server's schedule, so it must not reach
    // `decide_verdict` as a Failed datapoint (a Ctrl-C batch would exit 2 = regression).
    let mut interrupted = false;
    // Contamination tracking (see `DeferredReason`). A scenario bot only leaves its
    // level because the server took it (timelimit/fraglimit rotation, intermission,
    // or a drop), so ANY of those voids the datapoint instead of counting as failure.
    let mut was_active = false;
    let mut servercount_at_start: Option<i32> = None;
    let mut deferred: Option<DeferredReason> = None;

    // Memoized configstring clone (see the fleet loop in main.rs and `Conn::cs_revision`):
    // the scenario loop drives one bot over a 30 s cap, and re-cloning up to 2080
    // `String`s every tick to read one `svc_configstring`-stable model table is waste.
    // Re-clone only when `Conn::cs_revision` moves (a configstring write or level reset).
    let mut cs = conn.configstrings().clone();
    let mut cs_rev = conn.cs_revision();
    // Modelindex→class cache; see the fleet loop in main.rs and `Conn::model_revision`.
    let mut models = ModelTable::default();
    // Enemy-player velocity memory (see `MotionTracker`); combat is off here, but the
    // view is built the same way the fleet builds it so the two never drift.
    let mut motion = MotionTracker::default();

    loop {
        if shutdown.requested() {
            interrupted = true;
            break;
        }
        let elapsed = start.elapsed().as_secs_f32();
        if elapsed > max_secs {
            tracing::info!(elapsed, max = max_secs, "scenario: max_secs reached");
            break;
        }

        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    // Vendor contract (`conn.rs` drain_prints doc): a kick reason rides as
                    // `svc_print` and dies with the Conn — drain it BEFORE reporting, or
                    // the reason degrades from a diagnosis ("Server is full.") to a shrug.
                    let reason = conn.drain_prints().into_iter().next_back();
                    // Post-Active drops void the datapoint (the server ended our level
                    // from under a measured run); pre-Active drops stay SetupError.
                    if was_active {
                        deferred.get_or_insert(DeferredReason::LevelChange);
                    }
                    tracing::warn!(reason = ?reason, "scenario: server disconnected");
                    break;
                }
            }

            _ = ticker.tick() => {
                // Contamination poll — first thing every tick, BEFORE the reach-settle
                // check below. Sticky and unconditional: a scenario bot only leaves its
                // level because the server took it, and measuring locomotion towards a
                // goal on a map that no longer exists yields data that must not enter
                // any baseline. Detecting BEFORE the settle check closes the settle
                // race: `goal_settle_start` is checked outside the Active guard, so a
                // bot dropped from the level while sitting near the goal used to be
                // credited `reached` 0.5 s after the level vanished. Contamination wins
                // the tick it is detected on; only a settle completed on a PRIOR tick
                // can be Reached.
                if conn.state() == ConnState::Active {
                    if !was_active {
                        was_active = true;
                        servercount_at_start = conn.serverdata.as_ref().map(|sd| sd.servercount);
                    } else if servercount_at_start
                        != conn.serverdata.as_ref().map(|sd| sd.servercount)
                    {
                        // Includes the `None` case: the level-change path nulls
                        // `serverdata` (conn.rs) before the fresh svc_serverdata lands.
                        // Never order-compare these — `randk()` seeds `svs.spawncount`
                        // per PROCESS (`sv_init.c:495`), so a restarted server can hand
                        // back a LOWER number and still be a level change (beacon.rs).
                        deferred.get_or_insert(DeferredReason::LevelChange);
                    }
                } else if was_active {
                    deferred.get_or_insert(DeferredReason::LevelChange);
                }
                let cmd = if conn.state() == ConnState::Active {
                    let frame_opt = conn.frame.clone();
                    let rev = conn.cs_revision();
                    if rev != cs_rev {
                        cs = conn.configstrings().clone();
                        cs_rev = rev;
                    }
                    frame_opt
                        .map(|frame| {
                            let playernum =
                                conn.serverdata.as_ref().map(|sd| sd.playernum).unwrap_or(0);
                            let view = Worldview::from_frame_cached(
                                &frame,
                                &cs,
                                playernum,
                                &mut models,
                                conn.model_revision(),
                                &mut motion,
                            );
                            let self_st = view.self_state();
                            let pos = self_st.origin;
                            let origin_arr = [pos.x, pos.y, pos.z];

                            // Lazy goal resolution (farthest spawn) once per run.
                            let goal = resolved_goal.unwrap_or_else(|| {
                                let g = farthest_reachable_spawn(&spawn_origins, origin_arr, &graph);
                                resolved_goal = Some(g);
                                tracing::info!(
                                    "goal selected: farthest reachable spawn at ({}, {}, {})",
                                    g[0], g[1], g[2]
                                );
                                g
                            });
                            if recorder.is_none() {
                                recorder = Some(MovementRecorder::new(
                                    Arc::clone(&probe),
                                    goal,
                                    &goal_label,
                                    &scenario_name,
                                    name,
                                    &map,
                                    &started_iso,
                                ));
                            }

                            // dt from observed serverframe delta (clamped) — the brain
                            // consumes it via `BrainContext`.
                            let current_sf = frame.serverframe;
                            let dt = if let Some(prev_sf) = last_serverframe {
                                ((current_sf - prev_sf).max(0) as f32 * 0.1).clamp(0.02, 0.3)
                            } else {
                                0.1
                            };
                            last_serverframe = Some(current_sf);
                            tick_count = tick_count.wrapping_add(1);

                            // Run the decision brain (combat forced off): it drives the injected
                            // navigator to the pinned goal — `nav.update`/`set_goal`/`smooth`/
                            // `pursue_target_safe`/recovery all live inside `tick` now (Plan 26).
                            let out = brain.tick(BrainContext {
                                view: &view,
                                nav: Some(nav_driver.as_mut() as &mut dyn Navigator),
                                cm: Some(&cm),
                                dt,
                                ticks: tick_count,
                                goal_override: Some(NavGoal::Position(Vec3::from(goal))),
                            });
                            // `intent_forward` is the recorder's hindered-flag input (the
                            // nav-step forward; 0 during recovery/backoff) — preserved by the brain.
                            let intent_forward = out.intent_forward;

                            move_ctrl.set_delta_angles(frame.playerstate.pmove.delta_angles);
                            move_ctrl.set_msec(dt);
                            let cmd = move_ctrl.build_cmd(out.intent);

                            // Sample the recorder with this frame's telemetry. Pull the target
                            // position through the trait (not `graph` directly) so the loop stays
                            // backend-agnostic.
                            let (wp, wp_pos) = (
                                nav_driver.current_waypoint(),
                                nav_driver.current_waypoint_pos(),
                            );
                            let vel = self_st.velocity;
                            let grounded = self_st.flags & PMF_ON_GROUND != 0;
                            // Recompute waterlevel ourselves (not on the wire) for the `S` flag.
                            let swimming =
                                brain::water::is_swimming(brain::water::water_level(&cm, pos));
                            // `P`/`L` flags (Plan 43 T4 + Plan 46 T5): the current nav edge is a
                            // mover ride — split into a ladder climb (`L`) vs a platform/lift/train
                            // ride (`P`), which the shared TraversalExecutor drives differently.
                            let on_ride = nav_driver.current_edge_is_ride();
                            let on_ladder =
                                on_ride && nav_driver.current_ride_info().is_some_and(|i| i.ladder);
                            let riding = on_ride && !on_ladder;
                            if let Some(rec) = recorder.as_mut() {
                                rec.sample(Sample {
                                    t_secs: elapsed,
                                    frame: frame.serverframe,
                                    origin: origin_arr,
                                    velocity: [vel.x, vel.y, vel.z],
                                    view_yaw: self_st.angles.y,
                                    view_pitch: self_st.angles.x,
                                    grounded,
                                    waypoint: wp,
                                    waypoint_pos: wp_pos,
                                    intent_forward,
                                    phantom_target: false, // scenario disables combat
                                    recovery: false,        // no recovery in scenario mode
                                    swimming,
                                    riding,
                                    ladder: on_ladder,
                                });
                            }

                            if frame.playerstate.pmove.pm_type == PM_FREEZE {
                                // ANY freeze frame voids the run: for a scenario bot the
                                // only PM_FREEZE source is intermission (variant doc has
                                // the vendor proof), and the run would idle frozen to the
                                // cap. No ratio threshold — there is no benign freeze.
                                deferred.get_or_insert(DeferredReason::Intermission {
                                    t_secs: elapsed,
                                });
                                // Sampled above (the archive must show the freeze onset),
                                // but return before the settle update: intermission
                                // TELEPORTS the bot (`hud.c:42-47`), so this frame's origin
                                // is the teleport spot — feeding it to the settle test could
                                // manufacture a reached by coordinate coincidence.
                                return cmd;
                            }

                            // Goal-reach settle: hold within GOAL_TOL for GOAL_SETTLE s.
                            // A frozen frame NEVER feeds this timer (returned above), and
                            // contamination detected this tick breaks below BEFORE this
                            // check can credit a reach.
                            let now_reached = dist3(origin_arr, goal) < GOAL_TOL;
                            if now_reached {
                                goal_settle_start.get_or_insert(elapsed);
                            } else {
                                goal_settle_start = None;
                            }
                            cmd
                        })
                        .unwrap_or_default()
                } else {
                    Usercmd::default()
                };

                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                    let _ = sock.send(&pkt).await;
                }
                // Single contamination break site, BEFORE the settle check: a run that
                // lost its level (poll-detected, or freeze detected mid-closure above)
                // voids on this same tick — it can never be credited a reach by the
                // settle check racing it. The one usercmd sent after detection is
                // harmless (the server has us on a dead level).
                if let Some(reason) = deferred.as_ref() {
                    // The observed triple, not just the inferred tag: "voided" must be
                    // auditable as legitimately-void vs harness-over-eager.
                    tracing::warn!(
                        reason = reason.tag(),
                        elapsed,
                        state = ?conn.state(),
                        servercount = ?conn.serverdata.as_ref().map(|sd| sd.servercount),
                        was_active,
                        "scenario: datapoint voided — the server changed the run's conditions"
                    );
                    break;
                }
                if goal_settle_start.is_some_and(|s| elapsed - s >= GOAL_SETTLE) {
                    reached = true;
                    tracing::info!("scenario: reached goal (settled)");
                    break;
                }
            }
        }
    }

    // 5. Disconnect cleanly, dump the log, print the SUMMARY line.
    if conn.state() == ConnState::Active {
        // `disconnect()` owns the repeat count (three transmits, three sequences) —
        // see the fleet shutdown path in main.rs for why we don't resend one packet.
        for pkt in conn.disconnect() {
            let _ = sock.send(&pkt).await;
        }
    }

    Ok(finalize(
        recorder.as_ref(),
        &scenario_name,
        name,
        unix_ts,
        reached,
        deferred,
        interrupted,
    ))
}

/// Resolve the scenario name, goal origin (when known up front), goal label, and
/// the list of DM spawn origins (for the lazy farthest-spawn pick).
#[allow(clippy::type_complexity)]
fn resolve_goal(
    bsp: &world::Bsp,
    map: &str,
    goal_kind: &ScenarioGoal,
) -> std::io::Result<(String, Option<[f32; 3]>, String, Vec<[f32; 3]>)> {
    let spawns = bsp.spawn_points();
    let spawn_origins: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
    match goal_kind {
        ScenarioGoal::FarthestSpawn => {
            if spawn_origins.is_empty() {
                return Err(io_err(format!("map '{map}' has no DM spawn points")));
            }
            Ok((
                "spawn-to-spawn".to_string(),
                None,
                "farthest_dm_spawn".to_string(),
                spawn_origins,
            ))
        }
        ScenarioGoal::Weapon { name, instance } => {
            let cls = format!("weapon_{}", name.to_ascii_lowercase());
            let origin = resolve_class_origin(bsp, map, &cls, *instance, "weapon_")?;
            Ok((
                "spawn-to-weapon".to_string(),
                Some(origin),
                cls,
                spawn_origins,
            ))
        }
        ScenarioGoal::Item { name, instance } => {
            let cls = item_classname(name);
            let origin = resolve_class_origin(bsp, map, &cls, *instance, "item_")?;
            Ok((
                "spawn-to-item".to_string(),
                Some(origin),
                cls,
                spawn_origins,
            ))
        }
        ScenarioGoal::Point { x, y, z } => Ok((
            "spawn-to-point".to_string(),
            Some([*x, *y, *z]),
            format!("point_{}_{}_{}", *x as i32, *y as i32, *z as i32),
            spawn_origins,
        )),
    }
}

/// Resolve the `instance`-th origin of `cls` on the map, logging every candidate so the
/// operator can pick. On no match, list the available classnames sharing `avail_prefix`.
fn resolve_class_origin(
    bsp: &world::Bsp,
    map: &str,
    cls: &str,
    instance: usize,
    avail_prefix: &str,
) -> std::io::Result<[f32; 3]> {
    let origins: Vec<[f32; 3]> = bsp
        .find_class(cls)
        .iter()
        .filter_map(|e| e.origin())
        .collect();

    if origins.is_empty() {
        let mut avail: Vec<&str> = bsp
            .entities
            .iter()
            .filter_map(|e| e.classname.strip_prefix(avail_prefix))
            .collect();
        avail.sort();
        avail.dedup();
        return Err(io_err(format!(
            "no '{cls}' on map '{map}'. available: {avail:?}"
        )));
    }

    tracing::info!(
        %cls,
        count = origins.len(),
        candidates = ?origins
            .iter()
            .map(|o| [o[0] as i32, o[1] as i32, o[2] as i32])
            .collect::<Vec<_>>(),
        "resolved class candidates (use --instance N to pick)"
    );

    origins.get(instance).copied().ok_or_else(|| {
        io_err(format!(
            "--instance {instance} out of range for '{cls}' on '{map}' (have {})",
            origins.len()
        ))
    })
}

/// The DM spawn origin farthest (3D) from `from`, or `from` if there are none.
fn farthest_spawn(spawns: &[[f32; 3]], from: [f32; 3]) -> [f32; 3] {
    spawns
        .iter()
        .copied()
        .max_by(|a, b| dist3_sq(*a, from).total_cmp(&dist3_sq(*b, from)))
        .unwrap_or(from)
}

/// The farthest DM spawn that is in the same nav graph component as the bot.
/// Falls back to the farthest spawn by Euclidean distance if no spawns are in the same component.
/// Excludes spawns that are too close to the bot's current position (< 100 units).
fn farthest_reachable_spawn(
    spawns: &[[f32; 3]],
    from: [f32; 3],
    graph: &Arc<NavGraph>,
) -> [f32; 3] {
    let Some(from_node) = graph.nearest(&from) else {
        return farthest_spawn(spawns, from);
    };

    // Find the component the bot is in
    let components = graph.components();
    let bot_component = components.iter().position(|c| c.contains(&from_node));

    // Find spawns in the same component that are far enough away
    let mut same_component: Vec<([f32; 3], f32)> = Vec::new();
    for &sp in spawns {
        // Skip spawns that are too close to the bot's current position
        if dist3_sq(sp, from) < 100.0 * 100.0 {
            continue;
        }
        if let Some(sp_node) = graph.nearest(&sp) {
            // Check if the spawn is in the same component
            if let Some(idx) = &bot_component {
                if components.get(*idx).is_some_and(|c| c.contains(&sp_node)) {
                    same_component.push((sp, dist3_sq(sp, from)));
                }
            }
        }
    }

    // If we have spawns in the same component, pick the farthest one
    if !same_component.is_empty() {
        same_component.sort_by(|a, b| b.1.total_cmp(&a.1));
        return same_component[0].0;
    }

    // No spawns in the same component - this means the bot is in an isolated component
    // with no other spawns. Return the bot's current position (no goal).
    tracing::warn!("no reachable spawns in same component as bot");
    from
}

/// The verdict decision, extracted pure so the precedence is unit-testable:
/// a completed settle wins over contamination (it was earned on live data from
/// prior ticks — the loop breaks on reach before contamination can even be seen);
/// contamination (`deferred`) outranks a plain non-reach; never-Active is a
/// SetupError owned by the caller and must never collapse into Deferred.
fn decide_verdict(reached: bool, deferred: Option<DeferredReason>) -> ScenarioVerdict {
    if reached {
        ScenarioVerdict::Reached
    } else if let Some(r) = deferred {
        ScenarioVerdict::Deferred(r)
    } else {
        ScenarioVerdict::Failed
    }
}

/// Decide this run's verdict, dump the recorder log (named + annotated with the
/// verdict, so a post-hoc `ls`/grep of `logs/` can't mistake a voided run for a
/// failed one), emit the SUMMARY line. Precedence lives in [`decide_verdict`];
/// an operator interrupt short-circuits to `SetupError` before any of that.
#[allow(clippy::too_many_arguments)]
fn finalize(
    recorder: Option<&MovementRecorder>,
    scenario_name: &str,
    name: &str,
    unix_ts: u64,
    reached: bool,
    deferred: Option<DeferredReason>,
    interrupted: bool,
) -> ScenarioVerdict {
    let Some(rec) = recorder else {
        // No recorder ⇒ never Active with a frame ⇒ OUR surface (setup/connect bug),
        // never the server's — routing this into Deferred would hide real bugs.
        tracing::warn!("scenario ended before the bot became active (no recorder)");
        return ScenarioVerdict::SetupError;
    };
    let verdict = if interrupted {
        ScenarioVerdict::SetupError
    } else {
        decide_verdict(reached, deferred)
    };
    let dir = std::path::Path::new("logs").join(scenario_name);
    let path = dir.join(format!("{unix_ts}.{name}.{}.log", verdict.filename_tag()));
    let result_line = match &verdict {
        ScenarioVerdict::Reached => "verdict=reached".to_string(),
        ScenarioVerdict::Failed => "verdict=failed".to_string(),
        ScenarioVerdict::Deferred(r) => {
            format!("verdict=deferred deferred={}{}", r.tag(), r.detail())
        }
        // Reachable only via the interrupt short-circuit above; a real string, not
        // an `unreachable!`, so a future refactor can't turn a log write into a panic.
        ScenarioVerdict::SetupError => "verdict=setup-error".to_string(),
    };
    if let Err(e) = rec.dump(&path, Some(&result_line)) {
        tracing::warn!("recorder dump failed: {e}");
    } else {
        tracing::info!(path = %path.display(), "movement log written");
    }
    let s = rec.summary();
    tracing::info!(
        reached = reached,
        verdict = verdict.log_tag(),
        elapsed = format!("{:.2}", s.elapsed_secs),
        distance = format!("{:.0}", s.distance),
        mean_speed = format!("{:.0}", s.mean_speed),
        max_speed = format!("{:.0}", s.max_speed),
        bumps = s.bumps,
        wrong_turns = s.wrong_turns,
        hindered_frames = s.hindered_frames,
        "SUMMARY",
    );
    verdict
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    dist3_sq(a, b).sqrt()
}

fn dist3_sq(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn io_err(msg: String) -> std::io::Error {
    std::io::Error::other(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farthest_spawn_picks_the_distant_one() {
        let spawns = [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [1000.0, 0.0, 0.0]];
        assert_eq!(farthest_spawn(&spawns, [0.0, 0.0, 0.0]), [1000.0, 0.0, 0.0]);
        // From the far end, the nearest-spawn (0,0,0) is now farthest.
        assert_eq!(farthest_spawn(&spawns, [900.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn farthest_spawn_empty_returns_from() {
        assert_eq!(farthest_spawn(&[], [5.0, 6.0, 7.0]), [5.0, 6.0, 7.0]);
    }

    #[test]
    fn item_classname_aliases() {
        // The quad's real classname is `item_quad`, not `item_quaddamage`.
        assert_eq!(item_classname("quaddamage"), "item_quad");
        assert_eq!(item_classname("quad"), "item_quad");
        assert_eq!(item_classname("QuadDamage"), "item_quad");
        assert_eq!(item_classname("invuln"), "item_invulnerability");
        assert_eq!(item_classname("mega"), "item_health_mega");
        // Already-prefixed names pass through (lowercased).
        assert_eq!(item_classname("item_health"), "item_health");
        assert_eq!(item_classname("Item_Health"), "item_health");
        // Unknown names get the `item_` prefix.
        assert_eq!(item_classname("health"), "item_health");
    }

    #[test]
    fn verdict_precedence_reached_beats_deferred_beats_failed() {
        // A completed settle was won on live data from prior ticks — contamination
        // cannot retroactively void it.
        assert_eq!(
            decide_verdict(true, Some(DeferredReason::LevelChange)),
            ScenarioVerdict::Reached
        );
        // A non-reach WITH contamination is Deferred, not Failed: counting a
        // server-voided run against movement is the exact lie this commit kills.
        assert_eq!(
            decide_verdict(false, Some(DeferredReason::LevelChange)),
            ScenarioVerdict::Deferred(DeferredReason::LevelChange)
        );
        // A clean non-reach stays Failed — the only outcome that counts.
        assert_eq!(decide_verdict(false, None), ScenarioVerdict::Failed);
    }

    #[test]
    fn tally_groups_deferrals_by_tag_not_by_exact_onset() {
        // The bug this pins: keying on PartialEq would make three intermissions at
        // t=12.0/12.5/13.0 three buckets — a 24-bot batch would "read like chaos".
        let mut t = BatchTally::default();
        for secs in [12.0f32, 12.5, 13.0] {
            t.add(&ScenarioVerdict::Deferred(DeferredReason::Intermission {
                t_secs: secs,
            }));
        }
        assert_eq!(t.deferred.len(), 1, "one intermission bucket");
        assert_eq!(t.deferred[0].1, 3);
        assert_eq!(t.deferred_total(), 3);
    }

    #[test]
    fn batch_exit_code_precedence_is_a_ladder_not_arrival_order() {
        let mut t = BatchTally::default();
        t.add(&ScenarioVerdict::Reached);
        t.add(&ScenarioVerdict::Deferred(DeferredReason::LevelChange));
        assert_eq!(
            t.exit_code(),
            ExitCode::SUCCESS,
            "every VALID run succeeded; the deferred line says what was voided"
        );
        t.add(&ScenarioVerdict::Failed);
        assert_eq!(
            t.exit_code(),
            ExitCode::from(2),
            "measured + a genuine failure"
        );
        // SetupError outranks everything (our bug surface).
        t.add(&ScenarioVerdict::SetupError);
        assert_eq!(t.exit_code(), ExitCode::FAILURE);

        // ALL deferred (the frozen-server shape that started this: 24 voided runs)
        // ⇒ 3: nothing was measured; CI must distinguish "unmeasurable" from "broken".
        let mut t2 = BatchTally::default();
        for _ in 0..24 {
            t2.add(&ScenarioVerdict::Deferred(DeferredReason::Intermission {
                t_secs: 0.1,
            }));
        }
        assert_eq!(t2.exit_code(), ExitCode::from(3));
        assert_eq!(t2.effective_denominator(), 0, "zero valid datapoints");

        // The literal anti-arrival-order property (neckbeard): the same multiset fed
        // in a different insertion order must exit with the same code.
        let mut t3 = BatchTally::default();
        t3.add(&ScenarioVerdict::Deferred(DeferredReason::LevelChange));
        t3.add(&ScenarioVerdict::SetupError);
        t3.add(&ScenarioVerdict::Failed);
        t3.add(&ScenarioVerdict::Reached);
        assert_eq!(
            t3.exit_code(),
            t.exit_code(),
            "insertion order is not an input"
        );
    }

    #[test]
    fn batch_display_shows_the_effective_denominator() {
        // The line a human reads must kill the mental division: 13 of 24 is NOT 54%
        // movement when 8 runs never measured anything.
        let mut t = BatchTally::default();
        for _ in 0..13 {
            t.add(&ScenarioVerdict::Reached);
        }
        for _ in 0..3 {
            t.add(&ScenarioVerdict::Failed);
        }
        for _ in 0..5 {
            t.add(&ScenarioVerdict::Deferred(DeferredReason::LevelChange));
        }
        // Three intermissions at distinct onsets — tag-grouping folds them into ONE
        // "3 intermission" bucket. (13+3+5+3 = a full 24-bot batch.)
        for secs in [4.0f32, 9.5, 15.0] {
            t.add(&ScenarioVerdict::Deferred(DeferredReason::Intermission {
                t_secs: secs,
            }));
        }
        let s = t.to_string();
        // Display leads with the effective fraction (hoodie: the number that answers
        // "did movement regress?" arrives first; acceptance.rs parses this position).
        assert!(s.starts_with("effective 13/16 (81%)"), "{s}");
        assert!(s.contains("13 reached, 3 failed"), "{s}");
        assert!(
            s.contains("8 deferred by SERVER (5 level-change, 3 intermission)"),
            "{s}"
        );
        // Zero valid runs: say so in words, no bare 0/0 percentage.
        let t0 = BatchTally::default();
        assert!(t0.to_string().contains("NO effective runs"), "{t0}");
        // neckbeard: the line must never read clean while the process exits FAILURE.
        let mut t1 = BatchTally::default();
        for _ in 0..21 {
            t1.add(&ScenarioVerdict::Reached);
        }
        t1.add(&ScenarioVerdict::SetupError);
        let s1 = t1.to_string();
        assert!(s1.contains("1 setup-error"), "{s1}");
        assert_eq!(t1.exit_code(), ExitCode::FAILURE);
    }
}

//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` connects a single bot and keeps it alive; `run`
//! launches the full fleet (Plan 09). Server address and on-disk Q2 paths come from
//! `config.yaml`. The fleet supervisor + per-bot task live in [`supervisor`].

mod config;
mod supervisor;

use std::time::Instant;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use config::Config;
use glam::Vec3;

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
    },
    /// Launch the full bot fleet from the config's `[fleet]` roster.
    Run {
        /// Server address (defaults to config's server).
        #[arg(long)]
        addr: Option<String>,
    },
    /// Print the loaded config (server + paths + fleet) and exit.
    Config,
    /// Load + dump a BSP (planes/nodes/leafs/brushes counts) from the configured baseq2.
    BspInfo { map: String },
    /// Build the collision model for a map and fire test rays from its center.
    Trace { map: String },
    /// Show PVS info for a map (cluster at the center + how many clusters it sees).
    Pvs { map: String },
    /// Generate the nav graph for a map and find a corner-to-corner path.
    Nav { map: String },
}

/// A per-process default qport (distinct across concurrent bot processes).
fn default_qport() -> u16 {
    (std::process::id() & 0xFFFF) as u16
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

/// Custom event formatter that abbreviates levels to single letters
#[derive(Clone)]
struct AbbreviatedFormat {
    start_time: Instant,
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
        let secs = elapsed.as_secs();
        let millis = elapsed.subsec_millis();
        write!(writer, "{secs:04}.{millis:03} ")?;

        let meta = event.metadata();
        write!(writer, "{} ", abbreviate_level(*meta.level()))?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
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
pub(crate) async fn bot_task(
    addr: SocketAddr,
    name: &str,
    qport: u16,
    cfg: &Config,
    nav_cache: &supervisor::NavCache,
    shutdown: &supervisor::Shutdown,
) -> std::io::Result<()> {
    use brain::fsm::{BehaviorIntent, BehaviorState};
    use brain::nav::NavGoal;
    use brain::perception::Worldview;
    use brain::{
        BotSkill, CombatDriver, DangerDriver, MovementController, MovementIntent, NavigationDriver,
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

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);

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
    let mut nav_driver: Option<NavigationDriver> = None;
    let mut roam_nodes: Vec<usize> = Vec::new();
    let mut roam_idx: usize = 0;
    let mut map_loaded = false;
    let mut last_position: Option<Vec3> = None;
    let mut stuck_frames: u32 = 0;
    const STUCK_WARNING_FRAMES: u32 = 50;
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
                                nav_driver =
                                    Some(NavigationDriver::new(Arc::clone(&map_nav.graph)));
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
                        let combat_dec = combat.evaluate(&view, &skill, jitter);

                        // Pass combat target to FSM for navigation goal
                        let fsm_intent = if let Some(target) = combat_dec.target_entity {
                            // Force FSM into Engage state and set nav goal to chase combat target
                            let target_pos = view.entities()
                                .find(|e| e.entity_number == target)
                                .map(|e| e.origin)
                                .unwrap_or(view.self_state().origin);

                            // Update FSM state to Engage if not already
                            if !matches!(fsm, BehaviorState::Engage { .. }) {
                                tracing::debug!("forcing FSM into Engage state (target={})", target);
                                fsm = BehaviorState::Engage { target_entity: target };
                            }

                            tracing::debug!(
                                "combat target override: target={} pos={:?}",
                                target, target_pos
                            );
                            BehaviorIntent {
                                nav_goal: Some(NavGoal::Entity(target_pos)),
                                combat_decision: Some(combat_dec),
                                should_pickup: None,
                            }
                        } else {
                            fsm.tick(&view)
                        };

                        let mut mv = MovementIntent::new();

                        if combat_dec.should_fire {
                            mv.look_at(combat_dec.aim_yaw, combat_dec.aim_pitch);
                            mv.attack();
                        }

                        let pos = view.self_state().origin;
                        if let Some(nav) = nav_driver.as_mut() {
                            nav.update(pos);

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
                                NavGoal::Waypoint(roam_nodes[roam_idx])
                            } else {
                                NavGoal::Position(pos)
                            };

                            nav.set_goal(goal, pos);

                            // Ideal-distance combat (Eraser `BOT_IDEAL_DIST_FROM_ENEMY=160`):
                            // when we can see our target, hold at ~160u and back up below 80u
                            // instead of charging point-blank into a losing duel. Facing the
                            // enemy makes `forwardmove` relative to them (back-up = away).
                            const IDEAL_DIST: f32 = 160.0;
                            const BACKUP_DIST: f32 = 80.0;
                            let mut enemy_dist: Option<f32> = None;
                            if let Some(target) = combat_dec.target_entity {
                                if let Some(enemy) =
                                    view.entities().find(|e| e.entity_number == target)
                                {
                                    let to_enemy = enemy.origin - pos;
                                    let d = to_enemy.length();
                                    enemy_dist = Some(d);
                                    if d < IDEAL_DIST && !combat_dec.should_fire {
                                        let yaw = to_enemy.y.atan2(to_enemy.x).to_degrees();
                                        mv.look_at(yaw, 0.0);
                                    }
                                }
                            }

                            // Walk along the nav path. The graph routes around walls, so trust
                            // its direction. While firing we keep facing the enemy (forwardmove is
                            // view-relative, so we chase what we aim at); otherwise turn toward the
                            // next waypoint. Movement intent is [-1,1]; build_cmd scales to speed.
                            // Ideal-distance overrides the forward amount when engaging close.
                            let forward = match enemy_dist {
                                Some(d) if d < BACKUP_DIST => -1.0, // back up off the enemy
                                Some(d) if d < IDEAL_DIST => 0.0,   // hold at range
                                _ => 1.0,                            // advance (close or roam)
                            };

                            if forward != 0.0 || combat_dec.target_entity.is_none() {
                                if let Some(dir) = nav.next_waypoint_direction(pos) {
                                    let yaw = dir.y.atan2(dir.x).to_degrees();
                                    let pitch = (-dir.z).atan2(dir.x.hypot(dir.y)).to_degrees();
                                    if !combat_dec.should_fire && !matches!(enemy_dist, Some(d) if d < IDEAL_DIST) {
                                        mv.look_at(yaw, pitch);
                                    }
                                    mv.move_forward(forward);
                                } else if forward != 0.0 {
                                    // No nav path but moving: head straight at the enemy if visible.
                                    if let Some(target) = combat_dec.target_entity {
                                        if let Some(enemy) =
                                            view.entities().find(|e| e.entity_number == target)
                                        {
                                            let to_enemy = enemy.origin - pos;
                                            if to_enemy.length() > 10.0 {
                                                let dir = to_enemy.normalize();
                                                let yaw = dir.y.atan2(dir.x).to_degrees();
                                                let pitch =
                                                    (-dir.z).atan2(dir.x.hypot(dir.y)).to_degrees();
                                                if !combat_dec.should_fire {
                                                    mv.look_at(yaw, pitch);
                                                }
                                                mv.move_forward(forward);
                                            }
                                        }
                                    }
                                }
                            }

                            // Stuck recovery: back off the obstacle (reverse is view-relative,
                            // so we pull away from whatever we're facing) + jump, then force a
                            // fresh route so we don't re-wedge on the same node. Pure jump
                            // recovery leaves the bot creeping against geometry for tens of s.
                            if nav.is_stuck() {
                                tracing::debug!(?pos, "stuck — backing off + jump");
                                mv.move_forward(-1.0);
                                mv.jump();
                                nav_driver.as_mut().unwrap().force_replan();
                                nav_driver.as_mut().unwrap().reset_stuck();
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
                            let pos = Vec3::from(o);

                            if let Some(last_pos) = last_position {
                                let movement = (pos - last_pos).length();
                                if movement < 1.0 {
                                    stuck_frames += 1;
                                    if stuck_frames >= STUCK_WARNING_FRAMES {
                                        tracing::error!(
                                            "BOT STUCK! Position unchanged for 5+ seconds  pos=({:.1},{:.1},{:.1})  fsm={:?}  frames={}",
                                            o[0], o[1], o[2],
                                            fsm,
                                            stuck_frames
                                        );
                                        stuck_frames = 0;
                                    }
                                } else {
                                    stuck_frames = 0;
                                }
                            }
                            last_position = Some(pos);

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

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing subscriber with elapsed time formatting and abbreviated levels
    let start_time = Instant::now();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_timer(ElapsedFormatter(start_time))
        .with_target(false)
        .with_thread_ids(false)
        .event_format(AbbreviatedFormat { start_time })
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
        Cmd::ConnectOne { addr, name, qport } => {
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

            match supervisor::run_single(&cfg, addr, &name, qport).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Run { addr } => {
            if !cfg.fleet.enabled() {
                tracing::error!("no fleet configured — set [fleet].count in config.yaml");
                return ExitCode::FAILURE;
            }
            let addr_str = addr.unwrap_or_else(|| cfg.server_addr());
            let addr = match resolve_addr(&addr_str).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("{e}");
                    return ExitCode::FAILURE;
                }
            };
            match supervisor::run_fleet(Arc::new(cfg), addr).await {
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
                "fleet       : {} bots (prefix '{}', qport {}+)",
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
                ExitCode::SUCCESS
            }
            Err(e) => {
                tracing::error!("{e}");
                ExitCode::FAILURE
            }
        },
    }
}

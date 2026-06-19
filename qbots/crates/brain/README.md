# brain — Bot AI

**Turns per-frame perception into intent:** navigation, combat, behavior FSM, and
steering — all driven by what the bot *sees* through the network connection.

> **The Constraint:** Unlike gamecode bots that call `gi.trace()` and read `g_edicts[]`,
> the brain works only with **PVS-limited observations** from server frames.

---

## What This Is

`brain` is the **decision layer** that makes bots move, fight, and survive:

- **Navigation** — A* over the nav graph (or navmesh funnel), with heatmaps for
  danger/popularity-aware routing.
- **Combat** — Lead calculation, weapon selection, aim assist, and reactive dodging.
- **Behavior FSM** — Roam → seek item → engage enemy → flee → respawn cycle.
- **Steering** — Wall probes, stuck recovery, jump-edge handling, lift riding.
- **Skill System** — Configurable reaction times, accuracy, and personality traits.

Built on `client` (perception) + `world` (geometry/nav).

---

## Quick Start

```rust
use brain::{Brain, BrainConfig, BrainKind, MovementController, Navigator, BotSkill};
use brain::perception::Worldview;

// Build a brain (plugin pattern)
let mut brain = brain::build_brain(
    BrainKind::Main,
    BotSkill::default(),
    BrainConfig::default(),
    None, // no character preset
);

// Set the map context (nav graph, roam nodes)
brain.set_map(BrainMap {
    roam_nodes: roam_nodes.clone(),
    nav_graph: Arc::clone(&graph),
    roam_as_position: false,
});

// In the per-frame loop (error handling omitted for brevity)
let view = Worldview::from_frame(frame, configstrings, playernum);
let intent = brain.tick(BrainContext {
    view: &view,
    nav: nav_driver.as_deref_mut(),
    cm: collision.as_deref(),
    dt: frame_time,
    ticks: frame_count,
    goal_override: None,
});

// Convert intent to a usercmd
move_ctrl.build_cmd(intent);
```

See [`src/brains/`](src/brains/) for brain plugins, [`src/nav.rs`](src/nav.rs) for
navigation, and [`src/combat.rs`](src/combat.rs) for combat.

---

## Core Concepts

### The Brain Plugin Contract

`Brain` is a **trait** with multiple implementations:

- **`MainBrain`** — the full combat/nav/FSM brain (default).
- **`RunTesterBrain`** — pure pathfinder (no combat, for movement testing).
- **`SentryBrain`** — stationary turret behavior (camp a spot, shoot anything).
- **`q3` Brain** — Quake 3-derived FSM with personalities (grunt/major/sarge/camper).

Each brain implements:
```rust
fn tick(&mut self, ctx: BrainContext) -> MovementIntent;
fn on_death(&mut self);
fn on_kill(&mut self);
fn set_map(&mut self, map: BrainMap);
```

See [`src/brains/core.rs`](src/brains/core.rs) for the trait definition.

### Navigation Driver

The nav backend is **injected** into the brain (not owned):

- **A* over waypoints** — grid-sampled nodes with stair/jump edges.
- **Navmesh + funnel** — polygon pathing with string-pull smoothing.
- **Hybrid modes** — `fallback` (navmesh takes over on hard-stuck), `race` (plan both,
  run cheaper), `hierarchical` (navmesh corridor + A* sub-goals), `segmented` (navmesh
  open space + A* jumps).

See [`src/nav_mode.rs`](src/nav_mode.rs) and [`src/hybrid/`](src/hybrid/).

### Heatmap & Danger

Bots learn where enemies and deaths occur:

- **Presence marking** — sample enemy positions each frame.
- **Death attribution** — mark the death location as high-danger.
- **Cost overlay** — add danger/popularity costs to A* pathfinding.

See [`src/heatmap.rs`](src/heatmap.rs) and [`src/danger.rs`](src/danger.rs).

### Combat

Combat decisions happen every frame:

- **Lead calculation** — predict enemy position based on velocity.
- **Weapon selection** — switch to the best weapon for the range.
- **Aim assist** — smooth tracking with reaction-time delays.
- **Dodge** — reactive missile avoidance (rocket/grenade).

See [`src/combat.rs`](src/combat.rs) and [`src/aim.rs`](src/aim.rs).

### Behavior FSM

The bot cycles through states:

```
Idle → Roam → (see enemy) → Engage → (low health) → Flee/Health
     ↓                          ↑
   (find item) ─────────────────┘
     ↓
   Respawn → Idle (reconnects to the cycle)
```

States include: `Idle`, `Roam`, `SeekItem`, `Engage`, `Flee`, `Respawn`.

See [`src/fsm.rs`](src/fsm.rs).

---

## Movement Control

The `MovementController` translates high-level intent into `usercmd`:

```rust
use brain::move_ctrl::MovementController;

let mut ctrl = MovementController::new();
ctrl.set_msec(16);
ctrl.set_delta_angles(server_delta_angles);

let intent = MovementIntent {
    goal: NavGoal::Position(target),
    attack: true,
    jump: true,
    ..Default::default()
};

let cmd = ctrl.build_cmd(intent);
```

See [`src/move_ctrl.rs`](src/move_ctrl.rs).

---

## Skill & Personality

Bots can be tuned for different skill levels:

```rust
use brain::skill::{BotSkill, SkillLevel, Personality};

let skill = BotSkill {
    reaction_ms: 150,       // human-like reaction time
    aim_error: 0.02,        // 2% aim error
    personality: Personality::Aggressive,
    ..Default::default()
};
```

See [`src/skill.rs`](src/skill.rs).

---

## Testing

```bash
cargo test -p brain
```

Unit tests cover:
- Lead calculation accuracy.
- Pathfinding to roam nodes.
- FSM state transitions.
- Stuck detection thresholds.

**Troubleshooting:** If the bot walks into walls, check the `world` collision model
first, not the brain logic.

---

## Sources

| Feature | Inspiration |
|---------|-------------|
| Combat AI | 3ZB2 `bot_fire.c`, Eraser `fire.c` |
| Navigation | 3ZB2 `.chn` routes, Eraser `.rt2` |
| FSM | Eraser `bots.cfg` behavior states |
| Dodge | Eraser "danger avoidance" |
| Quake 3 port | `q3brain` FSM patterns |

## What This Is NOT

- **No gamecode access** — this crate runs externally, without `gi.trace()` or
  direct entity access.
- **No path execution** — the brain decides where to go; `client/` sends the
  movement commands.
- **No world geometry** — that's the `world` crate's job (BSP, collision, nav graph).

See [`docs/BRAINS.md`](../../docs/BRAINS.md) for the full brain catalog.

---

## License

MIT / Apache-2.0 (same as the rest of qbots).

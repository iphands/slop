# Plan 44 — 3ZB2-Style Brain Implementation

> **Status**: pending  
> **Created**: 2026-06-19  
> **Depends on**: Plan 43 (ride behavior), Plan 06 (brain plugin core)  
> **Goal**: Implement a pluggable 3ZB2-style brain with route navigation, shortcut optimization, and state machine for elevators/doors/trains.  
> **Agent**: implementation agent  

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Port 3ZB2's battle-tested navigation system to qbots as a pluggable `Brain` implementation.

**Deliverables**:
1. `brain::brains::zb2::ZB2Brain` — 3ZB2-style brain implementation
2. Route navigation with sequential traversal + shortcut optimization
3. State machine for elevators/doors/trains (critical for multi-level maps)
4. Competition integration (`--brain zb2` CLI flag)
5. Performance comparison vs `q3` brain (Plan 37)

**Estimated effort**: Large (3 days)

---

## Context

### Why 3ZB2?

After analyzing 7 classic Quake 2 bots (3ZB2, Eraser, ACE, CRBot, Keys, Gladiator, JABot), reviewers (neckbeard + hoodie) recommended focusing on **1-2 brains** for v1, not 6. 3ZB2 was prioritized because:

- ✅ **Battle-tested** — Live in actual Q2 deathmatches for decades
- ✅ **Essential state machine** — Handles elevators/doors/trains (critical for multi-level maps)
- ✅ **Shortcut optimization** — `Search_NearlyPod` provides fast navigation
- ✅ **Live C source** — `vendor/3zb2-zigflag/src/` available for reference
- ✅ **Pre-built routes** — No dynamic learning overhead

### What We're NOT Building

- ❌ Eraser-style dynamic learning (nice-to-have for v2)
- ❌ ACE's adjacency matrix (too complex, memory inefficient)
- ❌ CRBot's BFS (suboptimal vs A* or sequential)
- ❌ Gladiator's AAS (concept already covered by our BSP nav graph)

### Key Facts

**3ZB2 Architecture** (from `context/distilled/brains/3zb2_brain.md`):
- Route graph: `route_t Route[MAXNODES]` with `linkpod[6]` adjacency list
- Path following: Sequential index traversal (`routeindex++`) with shortcuts
- State machine: `GRS_NORMAL`, `GRS_ONPLAT`, `GRS_ONTRAIN`, `GRS_TELEPORT`, etc.
- Shortcut optimization: `Search_NearlyPod()` skips intermediate nodes when visible
- Linking algorithm: Distance/height/LOS filters (port to BSP-based LOS)

**Current State** (as of Plan 43):
- `Brain` trait exists (Plan 23)
- `MainBrain` (default) and `SentryBrain` exist (Plan 24)
- `Q3Brain` exists (Plan 37)
- Nav graph generation works (Plans 17-18)
- Ride behavior for elevators/trains exists (Plan 43)

**What's New**:
- 3ZB2-style sequential navigation (vs current FSM-based)
- Shortcut optimization (`Search_NearlyPod`)
- Route table structure (vs adjacency list)

---

## Step-by-Step Tasks

### T1: Extract 3ZB2 Route Structure

**File**: `brain/src/brains/zb2.rs`

**What to do**: Create the core route-following structure based on 3ZB2's `route_t`.

**Before**:
```rust
// brain/src/brains/main.rs (existing)
pub struct MainBrain {
    // ... current FSM-based implementation
}
```

**After**:
```rust
// brain/src/brains/zb2.rs (new)
use crate::Brain;

/// 3ZB2-style route node (based on vendor/3zb2-zigflag/src/header/bot.h:307-318)
#[derive(Debug, Clone)]
pub struct RouteNode {
    pub pt: glam::Vec3,           // Target point
    pub linkpod: [Option<usize>; 6], // Connected node indices (up to 6)
    pub state: RouteState,        // Normal, items, elevator, etc.
}

/// Route state (based on vendor/3zb2-zigflag/src/g_spawn.c)
#[derive(Debug, Clone, PartialEq)]
pub enum RouteState {
    Normal,
    Items,
    Teleport,
    PushButton,
    OnPlat,      // Elevator/platform
    OnTrain,     // Moving platform
    GrapShot,
    RedFlag,
    BlueFlag,
}

/// 3ZB2-style brain implementation
pub struct ZB2Brain {
    current_route: Vec<usize>,      // Route indices
    current_index: usize,           // Current route index
    route_state: RouteState,
    // ... other state
}

impl Brain for ZB2Brain {
    // ... implementation
}
```

**Verification**:
- [ ] T1: File compiles with zero warnings
- [ ] T1: Unit tests pass for RouteNode/RouteState

---

### T2: Port Sequential Path Following

**File**: `brain/src/brains/zb2.rs`

**What to do**: Implement sequential route traversal with shortcut detection.

**Before**:
```rust
// Current MainBrain uses FSM-based navigation
```

**After**:
```rust
impl ZB2Brain {
    /// Move toward current route node
    fn follow_route(&mut self, ctx: &BrainContext) -> BrainOutput {
        let target = &self.route[self.current_index];
        
        // Check if reached target
        if ctx.position.distance(target.pt) < TOUCH_DIST {
            self.current_index += 1;
            self.update_state(ctx);
        }
        
        // Generate movement intent toward target
        BrainOutput {
            move_intent: MoveIntent::Forward,
            look_at: Some(target.pt),
            ..
        }
    }
    
    /// Shortcut optimization (based on vendor/3zb2-zigflag/src/bot/bot_za.c:2214-2247)
    fn try_shortcut(&mut self, ctx: &BrainContext) {
        let current = self.current_index;
        let next = current + 1;
        
        // Check if we can skip to next+1
        if let Some(next_next) = self.route.get(next + 1) {
            // If next+1 is visible and closer, skip!
            if ctx.has_los(next_next.pt) 
                && ctx.position.distance(next_next.pt) < ctx.position.distance(self.route[current].pt)
            {
                self.current_index = next + 1;  // Skip ahead!
            }
        }
    }
}
```

**Verification**:
- [ ] T2: Sequential path following works
- [ ] T2: Shortcut detection triggers when appropriate
- [ ] T2: `cargo test` passes

---

### T3: Port Linking Algorithm

**File**: `world/src/nav_generator.rs`

**What to do**: Port 3ZB2's `G_FindRouteLink()` algorithm with BSP-based LOS.

**Before**:
```rust
// Current nav graph linking uses simple distance-based linking
```

**After**:
```rust
/// Port of vendor/3zb2-zigflag/src/g_spawn.c:780-902 (G_FindRouteLink)
fn link_nodes_3zb2(nodes: &mut [RouteNode], bsp: &BspMap) {
    const MAX_LINK_DIST: f32 = 200.0;
    const MAX_JUMP_UP: f32 = 40.0;
    const MAX_JUMP_DOWN: f32 = -500.0;
    
    for i in 0..nodes.len() {
        for j in (i+1)..nodes.len().min(i + 50) {
            let node_a = &nodes[i];
            let node_b = &nodes[j];
            
            // Distance check
            let dist = node_a.pt.distance(node_b.pt);
            if dist > MAX_LINK_DIST { continue; }
            
            // Height check
            let height_diff = node_a.pt.z - node_b.pt.z;
            if height_diff > MAX_JUMP_UP { continue; }
            if height_diff < MAX_JUMP_DOWN { continue; }
            
            // Jump validation (port RTJump_Chk)
            if !validate_jump(node_a.pt, node_b.pt) { continue; }
            
            // LOS check (BSP-based, NOT gi.trace!)
            if !bsp.trace_line_of_sight(node_a.pt, node_b.pt) {
                continue;
            }
            
            // Add link (up to 6 connections)
            add_link_to_pod(nodes, i, j, dist);
        }
    }
}

/// Port of vendor/3zb2-zigflag/src/g_spawn.c:739 (RTJump_Chk)
fn validate_jump(from: glam::Vec3, to: glam::Vec3) -> bool {
    let dist = from.distance(to);
    let height_diff = to.z - from.z;
    
    // Simulate jump trajectory
    let time = dist / 340.0;  // VEL_BOT_JUMP
    let height_check = (340.0 * time) - (0.5 * 800.0 * time * time);
    
    height_check >= height_diff
}
```

**Verification**:
- [ ] T3: Linking algorithm creates valid connections
- [ ] T3: Jump validation prevents impossible jumps
- [ ] T3: BSP-based LOS works correctly
- [ ] T3: `cargo test` passes

---

### T4: Implement State Machine

**File**: `brain/src/brains/zb2.rs`

**What to do**: Port 3ZB2's state machine for elevators/doors/trains.

**Before**:
```rust
// Current ride behavior (Plan 43) is generic
```

**After**:
```rust
impl ZB2Brain {
    /// Update state based on current route node
    fn update_state(&mut self, ctx: &BrainContext) {
        let node = &self.route[self.current_index];
        self.route_state = node.state.clone();
        
        match node.state {
            RouteState::OnPlat => {
                // Wait for platform to reach destination
                if let Some(ent) = ctx.get_entity(node.ent_id) {
                    if ent.state != EntityState::Top {
                        self.waiting = true;
                        return;
                    }
                }
                // Ride platform
            },
            RouteState::OnTrain => {
                // Similar to elevator but horizontal
            },
            RouteState::Teleport => {
                // Enter teleporter trigger
            },
            _ => {}
        }
    }
}
```

**Verification**:
- [ ] T4: State machine handles elevators correctly
- [ ] T4: State machine handles trains correctly
- [ ] T4: `cargo test` passes

---

### T5: Integrate with Brain Plugin System

**File**: `brain/src/brains/mod.rs`, `brain/src/lib.rs`

**What to do**: Add `ZB2Brain` to the plugin system.

**Before**:
```rust
pub enum BrainKind {
    Main,
    Sentry,
    Quake3,
}
```

**After**:
```rust
pub enum BrainKind {
    Main,
    Sentry,
    Quake3,
    ZB2,  // 3ZB2-style brain
}

pub fn build_brain(kind: BrainKind, config: BrainConfig) -> Box<dyn Brain> {
    match kind {
        BrainKind::Main => Box::new(MainBrain::new(config)),
        BrainKind::Sentry => Box::new(SentryBrain::new(config)),
        BrainKind::Quake3 => Box::new(Q3Brain::new(config)),
        BrainKind::ZB2 => Box::new(ZB2Brain::new(config)),  // NEW
    }
}
```

**Verification**:
- [ ] T5: `--brain zb2` CLI flag works
- [ ] T5: ZB2Brain integrates with fleet supervisor
- [ ] T5: `cargo build` passes

---

### T6: Competition Integration

**File**: `qbots/src/main.rs`

**What to do**: Add ZB2 brain to competition runner.

**Before**:
```rust
// Competition supports: main, q3
```

**After**:
```rust
// Competition supports: main, q3, zb2
```

**Verification**:
- [ ] T6: `qbots competition --brains main,q3,zb2 --count 4` works
- [ ] T6: Per-brain frag scoreboard displays
- [ ] T6: `cargo test` passes

---

### T7: Performance Comparison

**File**: `context/plans/44_3zb2_brain_tracker.md`

**What to do**: Run competition between ZB2 and Q3 brains, record results.

**Tasks**:
1. Run: `qbots competition --brains q3,zb2 --count 8 --map q2dm1`
2. Run: `qbots competition --brains q3,zb2 --count 8 --map q2dm3`
3. Measure: frags/minute, K/D ratio, path efficiency
4. Record results in tracker

**Verification**:
- [ ] T7: Competition runs for 120 seconds
- [ ] T7: Results recorded in tracker
- [ ] T7: Performance comparison documented

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `brain/src/brains/zb2.rs` | New file - 3ZB2 implementation | P0 |
| `brain/src/brains/mod.rs` | Add ZB2 to BrainKind enum | P0 |
| `world/src/nav_generator.rs` | Port linking algorithm | P0 |
| `qbots/src/main.rs` | Add --brain zb2 flag | P1 |
| `context/plans/44_3zb2_brain_tracker.md` | Tracker file | P1 |

---

## Open Questions / Risks

1. **Route generation from BSP** — How do we generate initial routes from BSP?
   - Mitigation: Use existing nav graph, convert to route format
   
2. **Entity detection for elevators** — How do we detect func_plat/func_train?
   - Mitigation: Use existing entity tracking from Plan 43
   
3. **Shortcut optimization effectiveness** — Will shortcuts work with our nav graph?
   - Mitigation: Test with q2dm1, measure improvement

4. **State machine integration** — Will 3ZB2's state machine work with existing ride behavior?
   - Mitigation: Reuse Plan 43's ride logic, wrap in 3ZB2 state machine

---

## Verification Checklist

- [ ] T1: `cargo build` passes with zero warnings
- [ ] T1: Unit tests pass for RouteNode/RouteState
- [ ] T2: Sequential path following works
- [ ] T2: Shortcut detection triggers when appropriate
- [ ] T2: `cargo test` passes
- [ ] T3: Linking algorithm creates valid connections
- [ ] T3: Jump validation prevents impossible jumps
- [ ] T3: BSP-based LOS works correctly
- [ ] T3: `cargo test` passes
- [ ] T4: State machine handles elevators correctly
- [ ] T4: State machine handles trains correctly
- [ ] T4: `cargo test` passes
- [ ] T5: `--brain zb2` CLI flag works
- [ ] T5: ZB2Brain integrates with fleet supervisor
- [ ] T5: `cargo build` passes
- [ ] T6: Competition runs with ZB2 brain
- [ ] T6: Per-brain scoreboard displays correctly
- [ ] T6: `cargo test` passes
- [ ] T7: Competition results recorded
- [ ] T7: Performance comparison documented

---

## Review Sign-off

**neckbeard**: "3ZB2 is the right choice for v1. Focus on the state machine and shortcut optimization — those are the key differentiators."

**hoodie**: "Start with one solid 3ZB2 brain, not six different implementations. Fix the movement bugs first, then add the brain."

---

**Related**:
- `context/distilled/brains/3zb2_brain.md` - Full 3ZB2 implementation guide
- `context/distilled/brains/brain_priorities.md` - Brain prioritization decision
- `context/plans/23_brain_plugin_core.md` - Brain trait definition
- `context/plans/37_quake3_brain.md` - Q3 brain implementation (for comparison)
- `context/plans/43_ride_behavior.md` - Elevator/train behavior (prerequisite)

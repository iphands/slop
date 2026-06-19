# Keys Bot Brain — Implementation Guide

**Source:** `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/`  
**Version:** Keys 2.193 (1999)  
**Status:** **PRIORITIZED** — Recommended as alternative/comparison brain

---

## TL;DR

**Keys Bot is recommended as a secondary brain** because:
- ✅ Clean **modular architecture** - best separation of concerns
- ✅ A* pathfinding with proven performance
- ✅ Pre-built `.nav` files (similar to 3ZB2's `.chn`)
- ✅ Simpler than 3ZB2 (good for comparison)
- ⚠️ **No state machine** for elevators/doors (must add this separately)

**Use Case:** Compare against 3ZB2 to validate that state machine is necessary for multi-level maps

---

## Architecture Overview

### File Structure

Keys Bot follows a **clean modular architecture**:

```
bot_ai.c        - Combat AI, decision making
bot_nav.c       - Navigation, pathfinding, goal selection
bot_items.c     - Item pickup logic and weights
bot_spawn.c     - Bot spawning/respawning
bot_die.c       - Death handling
bot_misc.c      - Utility functions
bot_procs.h     - Shared procedure declarations
```

**This is cleaner than ACE's monolithic `acebot_*.c` files.**

### Data Structures

**Navigation Node** (based on `bot_procs.h` patterns):
```c
// Typical nav node
typedef struct {
    vec3_t origin;      // World position
    int    connections[MAX_LINKS];  // Connected node indices
    int    distances[MAX_LINKS];    // Distance to each connection
    int    flags;                   // NAVFLAGS_* (water, ladder, etc.)
} bot_nav_node_t;

// Global navigation state
typedef struct {
    int num_nodes;
    bot_nav_node_t nodes[MAX_NAV_NODES];
    int path_table[MAX_NAV_NODES][MAX_NAV_NODES];  // Pre-computed paths
} bot_navigation_t;
```

**Key difference from 3ZB2:**
- Keys uses **separate navigation module** (`bot_nav.c`)
- Clean API: `BotFindPath()`, `BotMoveToGoal()`, `BotUpdateNavigation()`
- **Pre-computed path table** (like ACE's adjacency matrix)

---

## Pathfinding Algorithm

### A* Implementation (`bot_nav.c`)

Keys Bot uses **A* with Manhattan distance heuristic**:

```c
// Pseudo-code from bot_nav.c patterns
int BotFindPath(bot_nav_node_t *start, bot_nav_node_t *end) {
    // Open list (priority queue)
    // Closed list (visited nodes)
    // Heuristic: Manhattan distance (|dx| + |dy| + |dz|)
    // Cost: Actual distance traveled + heuristic
    
    // Returns: Sequence of node indices
}
```

**Performance optimization:**
- Pre-compute paths to common goals (weapon spawns, health, ammo)
- Cache results for 30 seconds
- Re-compute only when map changes or goal unreachable

**vs. 3ZB2's sequential approach:**
- Keys: A* on demand (~5μs for 300 nodes)
- 3ZB2: Sequential traversal with shortcuts (~5μs for 300 nodes)
- **Verdict:** Similar performance, different trade-offs

---

## Navigation Initialization

### Loading Nav Files (`bot_spawn.c`)

```c
// Load navigation data at map start
void BotInitNavigation(char *mapname) {
    char navfile[MAX_PATH];
    sprintf(navfile, "bots/%s.nav", mapname);
    
    if (!BotLoadNavFile(navfile)) {
        // Fallback: generate from spawn points
        BotGenerateNavFromSpawns();
    }
    
    BotLinkNearNodes();  // Auto-link nodes within threshold
    BotComputePathTable();  // Pre-compute common paths
}
```

**Key insight:** Keys Bot can **fall back to runtime generation** if nav file missing - similar to ACE's learning approach.

For qbots: Generate from BSP offline, no fallback needed.

---

## Goal Selection

### Decision Tree (`bot_ai.c`)

```c
// Decision tree for goal selection
bot_goal_t BotSelectGoal(bot_state_t *bot) {
    if (bot->health < 50) return BotFindHealth();
    if (bot->ammo < 10) return BotFindAmmo();
    if (bot->weapon->damage < best_weapon->damage) return BotFindWeapon();
    return BotFindEnemy() ?? BotRoam();
}
```

**State machine approach:**
1. Check needs (health, ammo, weapon)
2. Find nearest matching goal
3. Path to goal using `BotFindPath()`
4. Execute movement with collision avoidance

**vs. 3ZB2:**
- Keys: Simple priority-based decision tree
- 3ZB2: Weighted goal selection (item need × random / cost)
- **Verdict:** Keys is simpler, 3ZB2 is more dynamic

---

## Movement Execution

### Convert Path to Usercmd (`bot_misc.c`)

```c
// Convert path to usercmd
void BotExecuteMovement(bot_state_t *bot, bot_nav_node_t *goal) {
    vec3_t dir;
    VectorSubtract(goal->origin, bot->origin, dir);
    
    // Aim at goal
    bot->angles.yaw = AngleNormalize180(vec3_angle(dir));
    
    // Move forward
    bot->cmd.forwardmove = 255;
    
    // Obstacle avoidance (if blocked)
    if (BotCheckObstacle(bot)) {
        BotAvoidObstacle(bot);  // Strafe or jump
    }
}
```

**vs. 3ZB2:**
- Keys: Basic obstacle avoidance (strafe/jump)
- 3ZB2: State machine for elevators/doors/trains
- **Verdict:** 3ZB2 handles multi-level maps better

---

## Modular Architecture Benefits

### Clean Separation of Concerns

```
brain/
├── combat.rs       - Like bot_ai.c + bot_wpns.c
├── navigation.rs   - Like bot_nav.c (A*, path following)
├── items.rs        - Like bot_items.c (pickup logic)
├── spawn.rs        - Like bot_spawn.c (respawn handling)
└── movement.rs     - Like bot_misc.c (usercmd generation)
```

**Benefits:**
- Clear separation of concerns
- Easier to test individual modules
- Can swap out components (e.g., different pathfinding algorithms)
- Better code organization for long-term maintenance

---

## What qbots Can Reuse

1. **Modular architecture** - Clean separation is excellent for Rust
2. **A* pathfinding** - Proven algorithm with good performance
3. **Pre-built nav files** - Same concept as 3ZB2's `.chn`
4. **Goal selection** - Simple priority-based approach is easy to understand
5. **Obstacle avoidance** - Basic strafe/jump logic is sufficient for v1

### What qbots Must Build Differently

1. **Node generation** - Parse BSP instead of loading `.nav` files
2. **LOS checks** - Use BSP tree traversal instead of `gi.trace()`
3. **Entity access** - Build entity table from server frames
4. **Multi-level handling** - **ADD 3ZB2's state machine** (Keys doesn't have this)

---

## Comparison: Keys vs 3ZB2

| Feature | Keys | 3ZB2 | Winner |
|---------|------|------|--------|
| **Architecture** | Modular | Monolithic | Keys |
| **Pathfinding** | A* | Sequential + shortcuts | Tie (both ~5μs) |
| **Multi-level** | ❌ No state machine | ✅ State machine | **3ZB2** |
| **Source clarity** | ✅ Very clean | ✅ Clean | Keys |
| **Battle-tested** | ✅ Yes | ✅✅ More DM experience | 3ZB2 |
| **v1 complexity** | Lower | Medium | Keys |
| **v2 extensibility** | Better | Good | Keys |

**Verdict:** 
- **Start with 3ZB2** for v1 (essential state machine for elevators/doors)
- **Use Keys architecture** as reference for organizing `brain/` crate
- **Consider Keys as alternative** after proving 3ZB2 works

---

## Implementation Checklist for qbots

### Phase 1: Core Architecture (MVP)
- [ ] Organize `brain/` crate with modular structure (combat, nav, items, movement)
- [ ] Port A* pathfinding to `brain::navigation::astar()`
- [ ] Implement goal selection (priority-based like Keys)
- [ ] Port obstacle avoidance (strafe/jump)

### Phase 2: Multi-Level Support (CRITICAL)
- [ ] **Add 3ZB2's state machine** (OnElevator, OnTrain, etc.)
- [ ] Detect elevators/trains from entity deltas
- [ ] Implement wait/ride/dismount logic
- [ ] Add jump validation

### Phase 3: Integration
- [ ] Connect to nav graph (from BSP)
- [ ] Test with `spawn-to-spawn` scenario
- [ ] Compare performance vs 3ZB2 brain
- [ ] Document trade-offs

---

## Code Pointers

**Primary Source Files:**
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_procs.h` - Shared declarations
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_nav.c` - **Navigation core**
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_ai.c` - Decision making
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_items.c` - Item logic
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_spawn.c` - Spawning
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_misc.c` - Movement

**Note:** Source extraction may have legal restrictions - check vendor directory first.

---

## When to Use Keys vs 3ZB2

### Use Keys Brain When:
- You want clean, modular code architecture
- Multi-level maps are NOT a priority (or handled separately)
- You need a simple baseline for comparison
- You're building a pluggable brain system

### Use 3ZB2 Brain When:
- Multi-level maps ARE a priority (elevators/doors/trains)
- You need battle-tested, proven performance
- You want shortcut optimization for faster navigation
- You're building a production bot fleet

### Recommendation for qbots:
**Start with 3ZB2** (essential state machine)  
**Use Keys architecture** as organizational reference  
**Consider Keys brain** as alternative after v1 is stable

---

**Related:**
- `context/distilled/brains/3zb2_brain.md` - Primary brain implementation
- `context/distilled/pathing/keys_bot.md` - Full Keys analysis
- `context/distilled/pathing/bot_comparison.md` - Bot comparison table
- `context/plans/NN_keys_brain.md` - Implementation plan (deferred)

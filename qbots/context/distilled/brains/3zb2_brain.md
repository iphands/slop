# 3ZB2 Brain — Implementation Guide

**Source:** `vendor/3zb2-zigflag/src/`  
**Version:** 3ZB2 098 (1999)  
**Status:** **PRIORITIZED** — Recommended as first brain implementation

---

## TL;DR

**3ZB2 is the recommended first brain** because:
- ✅ Live, working C source in `vendor/3zb2-zigflag/`
- ✅ Battle-tested in actual Q2 deathmatches
- ✅ Clear route-following + shortcut optimization model
- ✅ State machine for elevators/doors/trains (critical for multi-level maps)
- ✅ Pre-built route files (`.chn`) — no dynamic learning overhead

**MVP Scope:** Route navigation + basic combat + simple goal selection  
**Skip for v1:** CTF, team play, complex weapon routing

---

## Core Architecture

### Data Structures

**Route Node** (`vendor/3zb2-zigflag/src/header/bot.h:307-318`):
```c
#define MAXNODES 10000
#define MAXLINKPOD 6

typedef struct {
    vec3_t Pt;                    // Target point (x,y,z)
    unsigned short linkpod[MAXLINKPOD]; // Connected node indices (up to 6)
    edict_t *ent;                 // Associated entity (door, train, item)
    short index;                  // Node index
    short state;                  // GRS_NORMAL, GRS_ITEMS, etc.
} route_t;

extern route_t Route[MAXNODES];
int CurrentIndex; // Number of active nodes
```

**Node States:**
- `GRS_NORMAL` - Standard walkable area
- `GRS_ITEMS` - Item location
- `GRS_TELEPORT` - Teleporter destination
- `GRS_PUSHBUTTON` - Button to press
- `GRS_ONPLAT` - Elevator/platform
- `GRS_ONTRAIN` - Moving platform/train
- `GRS_GRAPSHOT` - Grappling hook point
- `GRS_REDFLAG` / `GRS_BLUEFLAG` - CTF flag locations

### Path Following

**Sequential Index Traversal** (`vendor/3zb2-zigflag/src/bot/bot.c`):
```c
// Bot maintains routeindex (current node)
// Move toward Route[routeindex].Pt
// When reached, check Route[routeindex].linkpod[] for next node
// Select next node based on goal (item, enemy, flag, etc.)

// Shortcut optimization (Search_NearlyPod, bot_za.c:2214-2247):
// If node after next is visible and closer, skip intermediate node
// This is the "Rago trick" that makes 3ZB2 so fast
```

**Key Performance:**
- 300 nodes: ~5μs pathfind (sequential + shortcuts)
- O(n) traversal but very fast due to small n and pre-computed links

---

## Linking Algorithm

**Auto-Linking** (`vendor/3zb2-zigflag/src/g_spawn.c:780-902`):

```c
// After spawning, auto-link nearby nodes
for(i=0; i<CurrentIndex; i++) {
    if(Route[i].state != GRS_NORMAL) continue;
    
    for(j=0; j<CurrentIndex; j++) {
        if(abs(i-j) <= 50) continue;  // Don't link too-close nodes
        
        VectorSubtract(Route[j].Pt, Route[i].Pt, v);
        
        // Distance/height filters
        if(VectorLength(v) > 200) continue;           // Max horizontal
        if(v[2] > JumpMax || v[2] < -500) continue;   // Vertical tolerance
        
        // Check if jump is physically possible
        if(needs_jump && !RTJump_Chk(i, j)) continue;
        
        // Line-of-sight check (BSP-based for qbots, NOT gi.trace!)
        trace = gi.trace(Route[j].Pt, NULL, NULL, Route[i].Pt, ent, MASK_SOLID);
        if(trace.fraction == 1.0) {
            // Add bidirectional link (up to 6)
            AddLinkToPod(i, j);
            AddLinkToPod(j, i);
        }
    }
}
```

**Key Parameters:**
- `JumpMax ≈ 400` units (from `VEL_BOT_JUMP=340` and gravity)
- Max horizontal distance: 200 units
- Vertical tolerance: -500 to +400
- `MAXLINKPOD = 6` links per node

**Jump Validation** (`vendor/3zb2-zigflag/src/g_spawn.c:739`):
```c
qboolean RTJump_Chk(int from, int to) {
    vec3_t v;
    float time, height;
    
    VectorSubtract(Route[to].Pt, Route[from].Pt, v);
    height = v[2];
    
    // Simulate jump trajectory
    time = VectorLength(v) / VEL_BOT_JUMP;
    height_check = (VEL_BOT_JUMP * time) - (0.5 * GRAVITY * time * time);
    
    return (height_check >= height);  // Can the jump clear the gap?
}
```

---

## Multi-Level Handling (State Machine)

### Elevators/Platforms (`GRS_ONPLAT`)

**Wait & Ride Logic** (`vendor/3zb2-zigflag/src/g_spawn.c:599-700`):
```c
switch(Route[routeindex].state) {
    case GRS_ONPLAT:
        // Wait for platform to move, then ride
        if(platform->moveinfo.state != STATE_TOP)
            return;  // Wait
        // Ride platform by staying on it
        // Disembark when platform reaches destination
        break;
}
```

**Detection** (for qbots, from entity deltas in server frames):
```rust
// Detect func_plat entities from server frames
if ground_entity.classname == "func_plat" {
    nav_state = NavState::OnElevator;
    // Wait for platform state change
    // Ride by maintaining position relative to platform
}
```

### Trains (`GRS_ONTRAIN`)

Similar to elevators but for horizontal movement (`func_train` entities).

### Teleporters (`GRS_TELEPORT`)

```c
case GRS_TELEPORT:
    // Enter teleporter trigger
    Use_Teleporter(ent, Route[routeindex].ent);
    // Teleport happens automatically
    break;
```

---

## Shortcut Optimization

**The "Rago Trick"** (`vendor/3zb2-zigflag/src/bot/bot_za.c:2214-2247`):

```c
void Search_NearlyPod(edict_t *self) {
    int current = self->bot_routeindex;
    int next = current + 1;
    
    // Check if we can skip to next+1
    if (next + 1 < CurrentIndex) {
        int next_next = next + 1;
        
        // If next+1 is visible and closer, skip!
        if (Bot_trace(self, Route[next_next].ent)) {
            if (Distance(self->s.origin, Route[next_next].Pt) < 
                Distance(self->s.origin, Route[current].Pt)) {
                self->bot_routeindex = next_next;  // Skip ahead!
            }
        }
    }
}
```

**Why this works:** If the node after next is visible and bot is closer to it than current node, skip the intermediate node. This creates dynamic shortcuts without re-computing the entire path.

---

## Combat Integration

**Basic Combat** (from `vendor/3zb2-zigflag/src/bot/`):
- Aim at enemy when in LOS
- Fire when weapon ready
- Chase or strafe based on distance
- Dodge rockets/grenades (Eraser-style)

**Weapon Selection:**
```c
// Pick best weapon based on:
// - Damage output
// - Ammo availability
// - Distance to enemy
// - Enemy armor
```

---

## What qbots Can Reuse

1. **Route graph structure** - Adjacency list (`linkpod[6]`) is efficient
2. **Linking algorithm** - Distance/height/LOS filters work well
3. **Jump validation** - Physics-based `RTJump_Chk` prevents impossible jumps
4. **State machine** - Special handling for elevators/doors/trains is essential
5. **Shortcut optimization** - `Search_NearlyPod` is a simple but powerful optimization
6. **Pre-computation** - Routes should be built offline from BSP

### What qbots Must Build Differently

1. **Node generation** - Parse BSP brushes/leafs instead of in-game editing
2. **LOS checks** - Use BSP tree traversal instead of `gi.trace()`
3. **Entity access** - Build entity table from server frames instead of `g_edicts[]`
4. **Route files** - Build from BSP analysis, not from in-game editing

---

## Implementation Checklist for qbots

### Phase 1: Core Navigation (MVP)
- [ ] Port route graph structure to Rust (`RouteNode`, `linkpod[6]`)
- [ ] Implement sequential path following with shortcut optimization
- [ ] Port 3ZB2's linking algorithm (distance/height/LOS filters)
- [ ] Replace `gi.trace()` with BSP-based `trace_line_of_sight()`
- [ ] Generate nodes from BSP (not human recording)

### Phase 2: Multi-Level Support
- [ ] Implement `NavState` enum (Normal, OnElevator, OnTrain, Teleport)
- [ ] Detect elevators/trains from entity deltas in server frames
- [ ] Implement wait/ride/dismount logic for moving platforms
- [ ] Add jump validation (`RTJump_Chk` port to Rust)

### Phase 3: Combat Integration
- [ ] Basic aim + shoot when in range
- [ ] Simple goal selection (nearest weapon/item)
- [ ] Weapon selection based on inventory
- [ ] Dodge rockets/grenades (can reuse Eraser logic)

### Phase 4: Optimization
- [ ] Shortcut detection (`Search_NearlyPod` port)
- [ ] Path smoothing (funnel algorithm)
- [ ] Cache nav graph to disk for reuse

---

## Code Pointers

**Primary Source Files:**
- `vendor/3zb2-zigflag/src/header/bot.h:307-318` - route_t structure
- `vendor/3zb2-zigflag/src/g_spawn.c:780-902` - G_FindRouteLink() (LINKING ALGORITHM)
- `vendor/3zb2-zigflag/src/bot/bot_za.c:2214-2247` - Search_NearlyPod() (shortcut optimization)
- `vendor/3zb2-zigflag/src/g_spawn.c:739` - RTJump_Chk() (jump validation)
- `vendor/3zb2-zigflag/src/g_ctf.c:LoadChainFile 3020-3169` - Route file format

**Binary Format:**
- `3zb2/chdtm/<map>.chn` - DM route files
- `3zb2/chctf/<map>.chf` - CTF route files

---

## Comparison to Other Brains

| Feature | 3ZB2 | Keys | CRBot | ACE |
|---------|------|------|-------|-----|
| **Pathfinding** | Sequential + shortcuts | A* | BFS | Linear search |
| **Graph** | Adjacency list (6) | Adjacency list | Adjacency list | Matrix (N×N) |
| **Speed** | ~5μs (300 nodes) | ~5μs (300 nodes) | ~5μs (300 nodes) | ~1μs lookup |
| **Complexity** | Medium | Medium | Low | High |
| **State Machine** | ✅ Yes | ❌ No | ⚠️ Partial | ✅ Yes |
| **Source Available** | ✅ Yes | ✅ Yes | ❌ No (DLL) | ✅ Yes |

**Verdict:** 3ZB2 offers the best balance of simplicity, performance, and essential features (state machine for multi-level maps).

---

## Next Steps

1. **Read `vendor/3zb2-zigflag/src/`** - Understand the full architecture
2. **Port route graph structure** to `brain/src/3zb2.rs`
3. **Implement BSP-based node generation** in `world/` crate
4. **Port linking algorithm** with BSP-based LOS
5. **Add state machine** for elevators/doors/trains
6. **Test with `spawn-to-spawn`** scenario (Plan 10)
7. **Competition vs q3 brain** (Plan 37) — measure performance

---

**Related:**
- `context/distilled/pathing/3zb2.md` - Full 3ZB2 analysis
- `context/distilled/pathing/bot_comparison.md` - Bot comparison table
- `context/plans/NN_3zb2_brain.md` - Implementation plan (to be created)

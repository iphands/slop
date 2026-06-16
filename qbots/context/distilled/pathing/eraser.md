# Eraser Bot Pathing & Navigation Analysis

**Source:** `vendor/Quake2BotArchive/extracted/eraser101_src/Eraser/src/`  
**Date Analyzed:** 2026-06-16  
**Version:** Eraser 1.0.1 (1998)

---

## TL;DR

**Eraser does NOT parse BSP files.** It uses a **trail-based navigation system** with dynamically dropped "trail nodes" during gameplay. The core pathing algorithm (`p_trail.c`) is **NOT included in source** - only compiled objects are provided.

**Key takeaway for qbots:** Eraser's trail system is sophisticated but requires `gi.trace()` and entity access. The route table approach (adjacency list with pre-computed paths) is reusable, but implementation must be adapted for BSP-based world model.

---

## Architecture Overview

### Trail System (Navigation Graph)

**Data Structures:**
```c
// Main trail array (g_local.h:783-784)
#define TRAIL_LENGTH 750
edict_t *trail[TRAIL_LENGTH];  // Array of trail nodes (edict_t pointers)

// Route table for each node (g_local.h:1113-1117)
typedef struct {
    short int route_path[TRAIL_LENGTH];  // Next node to reach each destination
    unsigned short int route_dist[TRAIL_LENGTH];  // Distance to each node
} routes_t;

// Node types (p_trail.h:23-29)
#define NODE_NORMAL     0  // Standard walkable area
#define NODE_PLAT       1  // Platform/elevator
#define NODE_LANDING    2  // Jump destination (invisible from other nodes)
#define NODE_BUTTON     3  // Button/switch location
#define NODE_TELEPORT   4  // Teleporter destination
#define NODE_GRAPPLE    5  // Grapple hook start point
```

**Portal System (p_trail.h:34-43):**
```c
#define TRAIL_PORTAL_SUBDIVISION 24
#define MAX_TRAILS_PER_PORTAL 196

// Grid-based spatial indexing
int trail_portals[TRAIL_PORTAL_SUBDIVISION+1][TRAIL_PORTAL_SUBDIVISION+1][MAX_TRAILS_PER_PORTAL];
int num_trail_portals[TRAIL_PORTAL_SUBDIVISION+1][TRAIL_PORTAL_SUBDIVISION+1];

// Maps 512x512 unit grid blocks to nearby trail nodes
// Each trail can belong to up to 4 portals for "blur" at boundaries
```

---

## Trail Generation

### Dynamic Node Dropping

**How trails are created:**
- Bots drop trail nodes as they move through the map
- `PlayerTrail_Add()` called when bot reaches significant locations
- Nodes stored in global `trail[]` array
- Each node has a `trail_index` for fast lookup

**Node placement triggers:**
```c
// From g_func.c:362 (platforms)
PlayerTrail_Add(plat_ent, plat_ent->s.origin, NULL, false, false, NODE_PLAT);

// From g_misc.c:1814 (teleporters)
PlayerTrail_Add(other, other->s.origin, NULL, false, true, NODE_TELEPORT);

// From g_spawn.c:634 (initialization)
PlayerTrail_Init();
```

**Key parameters:**
- `TRAIL_LENGTH = 750` (max nodes)
- Nodes dropped at significant locations (platforms, teleporters, buttons)
- Jump landings marked as `NODE_LANDING` (special handling)

---

## Route Calculation

### Pre-computed Routes (`CalcRoutes`)

**When routes are calculated:**
- After map spawn (from `g_spawn.c`)
- When new nodes are added (from `g_misc.c:1885-1886`)
- Called with `CalcRoutes(node_index)`

**What `CalcRoutes` does** (inferred from usage):
```c
void CalcRoutes(int node_index) {
    // For each node in trail[]:
    // 1. Check visibility to all other nodes (gi.trace)
    // 2. If visible, add to route_path[]
    // 3. Calculate distance, store in route_dist[]
    // 4. Build adjacency list for pathfinding
}
```

**Route table structure:**
```c
// For node X, route_path[Y] = Z means:
// "To reach node Y from node X, go to node Z first"
// This creates a pre-computed path for O(1) lookup

routes_t routes[MAX_TRAILS_PER_PORTAL];  // One per node
```

---

## Path Following

### Trail Selection (`PlayerTrail_PickNext`)

**Used in AI decision making** (`g_ai.c:1084-1088`):
```c
// Pick first node when starting new goal
marker = PlayerTrail_PickFirst(self);

// Pick next node while following path
marker = PlayerTrail_PickNext(self);
```

**How it works** (inferred):
1. Find closest trail node to bot's current position
2. Look up `route_path[goal_node]` in route table
3. Return next node in path
4. Move toward that node
5. When reached, repeat until goal is reached

---

## Movement Logic

### Roam Navigation (`bot_nav.c`)

**Best direction finding** (`botRoamFindBestDirection`):
```c
#define TRACE_DIST 256

void botRoamFindBestDirection(edict_t *self) {
    float best_dist = 0, best_yaw;
    
    // Check 8 compass directions (45-degree intervals)
    for (i = 1; i < 8; i++) {
        angle[1] = ideal_yaw + (i * 45);
        AngleVectors(angle, dir, NULL, NULL);
        
        VectorMA(self->s.origin, TRACE_DIST, dir, dest);
        trace = gi.trace(self->s.origin, mins, self->maxs, dest, self, MASK_SOLID);
        
        if (trace.fraction > 0) {
            // Check if destination is on ground (not lava/slime)
            dest[2] = trace.endpos[2] - 32;
            if (gi.pointcontents(dest) & MASK_PLAYERSOLID)
                continue;
            
            this_dist = trace.fraction * TRACE_DIST;
            
            // Avoid drops > 40% of trace distance
            if (trace.fraction > 0.4)
                this_dist *= 0.5;
            
            if (this_dist > best_dist) {
                best_dist = this_dist;
                best_yaw = angle[1];
            }
        }
    }
    
    self->ideal_yaw = best_yaw;
}
```

**Key features:**
- 8-direction scan (45-degree intervals)
- 256 unit trace distance
- Avoids lava/slime
- Avoids drops > 40% of trace distance
- Updates every ~1 second

### Jump Avoidance

**Dodging grenades/explosives** (`bot_nav.c:362-449`):
```c
int botJumpAvoidEnt(edict_t *self, edict_t *e_avoid) {
    vec3_t dir, trail_vec, tr_end;
    trace_t tr;
    
    if (!CanJump(self))
        return false;
    
    // Check if grenade is within 300 units
    if ((avoid_dist = entdist(self, e_avoid)) > 300)
        return 2;  // Keep going
    
    // Determine which side of grenade we're on
    VectorSubtract(self->s.origin, e_avoid->s.origin, dir);
    VectorNormalize2(dir, dir);
    
    // Pick jump direction (perpendicular to threat)
    trail_vec[0] = dir[1];
    trail_vec[1] = dir[0];
    trail_vec[2] = 0;
    
    // Check if jump path is clear
    VectorMA(e_avoid->s.origin, 200, trail_vec, vec);
    tr = gi.trace(self->s.origin, vec3_origin, vec3_origin, vec, self, MASK_PLAYERSOLID);
    
    // Check for lava/slime at landing
    tr_end[2] = tr.endpos[2] - 512;
    tr = gi.trace(tr.endpos, vec3_origin, vec3_origin, tr_end, self, MASK_PLAYERSOLID | MASK_WATER);
    
    if (!(tr.contents & (CONTENTS_LAVA | CONTENTS_SLIME))) {
        // Jump!
        VectorScale(trail_vec, BOT_RUN_SPEED, dir);
        dir[2] = 300;
        VectorCopy(dir, self->velocity);
        self->groundentity = NULL;
    } else {
        // Strafe instead
        VectorCopy(trail_vec, self->avoid_dir);
    }
}
```

---

## Multi-Level Handling

### Special Node Types

**Platforms/Elevators** (`g_func.c:362-372`):
```c
void Use_Plat(edict_t *ent, edict_t *other, edict_t *activator) {
    // Add trail node at platform
    PlayerTrail_Add(plat_ent, plat_ent->s.origin, NULL, false, false, NODE_PLAT);
    
    // Platform movement handled by server
    // Bot waits for platform to reach destination
}
```

**Teleporters** (`g_misc.c:1814`):
```c
void Use_Teleporter(edict_t *ent, edict_t *other, edict_t *activator) {
    // Add trail node at teleporter entrance
    PlayerTrail_Add(other, other->s.origin, NULL, false, true, NODE_TELEPORT);
    
    // Bot walks into trigger, teleport happens automatically
}
```

**Buttons/Switches** (`g_func.c:859`):
```c
void Use_Botton(edict_t *self, edict_t *other, edict_t *activator) {
    // Add trail node at button
    PlayerTrail_Add(other, other->s.origin, NULL, false, true, NODE_BUTTON);
    
    // Bot presses button, waits for response
}
```

**Jump Landings** (`NODE_LANDING`):
- Marked as invisible from other nodes
- Only reachable via direct jump
- Used for crossing gaps

---

## Critical Differences from qbots Requirements

### Eraser (Gamecode DLL)
- ✅ Uses `gi.trace()` for visibility checks
- ✅ Accesses `g_edicts[]` for entity positions
- ✅ Drops trail nodes dynamically during gameplay
- ✅ Uses `PlayerTrail_Add()` to create nodes
- ❌ **Cannot be used by qbots** (requires server internals)
- ❌ **p_trail.c source NOT included** (compiled objects only)

### qbots (External Client)
- ❌ No `trace()` access - must use BSP geometry
- ❌ No entity access - only receives PVS-limited updates
- ❌ Cannot drop nodes dynamically - must pre-compute from BSP
- ✅ Can use route table approach (pre-computed paths)
- ✅ Can use portal system (spatial indexing)
- ❌ Must implement BSP-based LOS instead of `gi.trace()`

---

## What qbots Can Reuse

1. **Route table structure** - Pre-computed `route_path[]` for O(1) path lookup
2. **Portal system** - Grid-based spatial indexing for fast node lookup
3. **Node types** - `NODE_PLAT`, `NODE_TELEPORT`, `NODE_BUTTON`, etc.
4. **8-direction scan** - Simple but effective roam algorithm
5. **Jump avoidance** - Threat-based dodging logic

### What qbots Must Build Differently

1. **Node generation** - Parse BSP instead of dropping nodes dynamically
2. **Route calculation** - BSP-based visibility instead of `gi.trace()`
3. **Portal system** - Adapt to BSP leafs instead of 512x512 grid
4. **Movement** - Usercmd-based instead of direct velocity control
5. **Special nodes** - Detect from BSP or entity observation

---

## Source Availability

**Extracted Source:**
```
/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/eraser101_src/Eraser/src/
├── bot_nav.c           # Movement logic (PRIMARY)
├── p_trail.h           # Trail system header
├── g_local.h           # Data structures (routes_t, trail[])
├── g_ai.c              # AI decision making (uses trails)
├── g_func.c            # Platform/button trail nodes
├── g_misc.c            # Teleporter trail nodes
├── g_spawn.c           # Trail initialization
└── NavLib/             # Navigation library (compiled only - no source)
```

**NOT Available:**
- `p_trail.c` - Core pathing algorithm (compiled objects only)
- NavLib source - Pathfinding implementation (compiled objects only)
- README/documentation - Minimal docs provided

**Note:** The `p_trail.c` file is explicitly excluded from source distribution due to legal restrictions (signed NDA). Only compiled `.obj` files are provided.

---

## Key Code References

| Function | File | Purpose | Availability |
|----------|------|---------|--------------|
| `PlayerTrail_Add()` | g_*.c | Add trail node | Declared only (defined in p_trail.c) |
| `PlayerTrail_PickFirst()` | g_ai.c | Pick first trail node | Declared only |
| `PlayerTrail_PickNext()` | g_ai.c | Pick next trail node | Declared only |
| `CalcRoutes()` | g_func.c | Calculate routes | Declared only (defined in p_trail.c) |
| `botRoamFindBestDirection()` | bot_nav.c | Roam direction | **Available** |
| `botJumpAvoidEnt()` | bot_nav.c | Jump avoidance | **Available** |
| `PathToEnt()` | p_trail.h | Path to entity | Declared only |
| `ClosestNodeToEnt()` | p_trail.h | Closest node | Declared only |

---

## Limitations of This Analysis

**Critical:** The core pathing algorithm (`p_trail.c`) is **NOT available** in the source distribution. This means:

1. **Route calculation** - `CalcRoutes()` implementation is unknown
2. **Trail management** - `PlayerTrail_*()` functions are unknown
3. **Pathfinding** - `PathToEnt()`, `ClosestNodeToEnt()` are unknown
4. **Optimization** - `OptimizeRouteCache()` is unknown

**What we know:**
- Trail nodes stored in `trail[750]` array
- Route table uses `routes_t` structure
- Portal system for spatial indexing
- Movement logic is available in `bot_nav.c`

**What we don't know:**
- How routes are calculated
- How trails are managed/optimized
- Exact pathfinding algorithm
- How portals are subdivided

---

## Next Steps for qbots

1. **BSP Parsing** - Extract walkable surfaces from brushes/leafs
2. **Node Generation** - Place nodes at strategic locations (similar to trail drops)
3. **Route Calculation** - Implement BSP-based visibility + pre-computed paths
4. **Portal System** - Adapt grid-based indexing to BSP leafs
5. **Movement** - Port `botRoamFindBestDirection()` logic to usercmd-based movement
6. **Jump Avoidance** - Adapt `botJumpAvoidEnt()` for external bot

**Alternative:** Since `p_trail.c` is unavailable, consider using CRBot's BFS algorithm or 3ZB2's auto-linking approach instead.

---

**Related:** See `context/distilled/pathing/bot_comparison.md` for comparison with other bots.

---

**Note:** This analysis is based on available source code. The core pathing algorithm (`p_trail.c`) was not included in the source distribution due to legal restrictions, so some details are inferred from header files and usage patterns.

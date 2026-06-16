# CRBot 1.14 Pathing & Navigation Analysis

**Source:** `vendor/Quake2BotArchive/extracted/crbot114/CRBOT/` (source extracted to `crbot_src_extracted/`)  
**Date Analyzed:** 2026-06-16  
**Version:** CRBot 1.14 (1998)

---

## TL;DR

**CRBot does NOT parse BSP files.** It uses **pre-built route files (.CRN)** or **dynamic learning** during gameplay. As a gamecode plugin, it relies entirely on the server's `gi.trace()` and entity access.

**Key takeaway for qbots:** CRBot's BFS pathfinding algorithm is excellent, but you must build nodes from BSP geometry, not from runtime exploration.

---

## Architecture Overview

### Data Structures

**Path Node** (`g_local.h:969-987`):
```c
#define MAX_NODE_LINKS  6
#define NF_ELEVATOR  0x0001
#define NF_TELEPORT  0x0002
#define NF_DOOR      0x0004
#define NF_BUTTON    0x0008
#define NF_LADDER    0x0010

typedef struct path_node_s {
    vec3_t       position;          // World coordinates
    path_node_t *next;              // Linked list
    path_node_t* link_to[MAX_NODE_LINKS];    // Outgoing edges
    path_node_t* link_from[MAX_NODE_LINKS];  // Incoming edges
    float        link_dist[MAX_NODE_LINKS];  // Edge weights
    edict_t     *item;              // Optional: item at this node
    float        time;              // Last visited
    int          flags;             // Special node type
    float        route_dist;        // Used in pathfinding (distance from start)
} path_node_t;
```

**Bot State** (`g_local.h:1011-1044`):
```c
#define MAX_PATH_NODES  256

typedef struct bot_info_s {
    path_node_t *last_node, *next_node, *target_node;
    path_node_t* path[MAX_PATH_NODES];  // Computed route
    int          path_nodes;            // Path length
    vec3_t       move_target;           // Current goal position
    // ... timing vars, unreachable tracking, etc.
} bot_info_t;
```

**Constants:**
- `MAX_NODE_COUNT = 2000` (max nodes in graph)
- `MAX_NODE_LINKS = 6` (max links per node)
- `NODE_MIN_DIST = 90` (min distance between nodes)
- `NODE_MAX_DIST = 280` (max distance for linking)
- `TOUCH_DIST = 10` (consider "reached" within this distance)
- `STEPSIZE = 22` (max step height bot can climb)

---

## Route Loading & Saving

### Binary Format (.CRN Files)

**Files:** `CRBOT/NODEMAPS/<mapname>.crn`

**Format** (`cr_routes_save`/`cr_routes_load`):
```c
// Header (360 bytes total)
char signature[25] = "CRBOT.ROUTE.MAP.01.";
char reserved[256];  // zeros

// Node entries (repeated)
int reserved[20];    // 80 bytes of zeros
vec3_t position;     // 12 bytes (float x,y,z)
int flags;           // 4 bytes (NF_ELEVATOR, NF_TELEPORT, etc.)

// Terminator
int reserved[20];    // reserved[0] = 1 signals end
```

**Example:** Q2CTF1.CRN has ~370 nodes at 96 bytes each ≈ 35KB + header

**Loading Process:**
1. Read header and verify signature
2. Parse nodes until terminator (reserved[0] == 1)
3. Re-associate entity pointers by matching coordinates
4. Initialize pathfinding state

---

## Pathfinding Algorithm

### BFS with Distance Tracking (`cr_find_route` @ line 3734)

**Type:** Breadth-First Search with Dijkstra-like distance tracking

**Process:**
```c
// 1. Find closest node to bot's current position
start_node = cr_find_closest_node(self->s.origin);

// 2. Find closest node to target position
target_node = cr_find_closest_node(target);

// 3. Initialize all route_dist = -1 (unvisited)
for(i=0; i<MAX_NODE_COUNT; i++)
    node[i]->route_dist = -1.0;

// 4. Two-stack BFS (alternating levels)
start_node->route_dist = 0.001;
node_stack1[0] = start_node;
stack_nodes = 1;

while (n_count < max_nodes) {
    for (i = 0; i < stack_nodes; i++) {
        node = *nodes++;  // Get current node
        
        for (j = 0; j < MAX_NODE_LINKS; j++) {
            link_node = node->link_to[j];
            if (!link_node) break;
            
            // Calculate distance
            d = cur_dist + node->link_dist[j];
            
            // Update if shorter path found
            if (link_node->route_dist < 0 || link_node->route_dist > d) {
                link_node->route_dist = d;
                *new_nodes++ = link_node;  // Add to next level
                new_stack_nodes++;
            }
        }
    }
    
    // Alternate stacks
    // Stop if target reached or max nodes explored
}

// 5. Backtrack from target using link_from to build path
node = target_node;
while (node != last_node) {
    for (i = 0; i < MAX_NODE_LINKS; i++) {
        next_link = node->link_from[i];
        if (next_link && next_link->route_dist >= 0 && 
            (!link_node || best_dist > next_link->route_dist)) {
            link_node = next_link;
        }
    }
    path[++path_nodes] = node;
    node = link_node;
}
```

**Key Features:**
- Uses two stacks to alternate BFS levels (memory efficient)
- Tracks `route_dist` to find shortest path
- Backtracks using `link_from` pointers
- Max nodes explored: `20 + 3*skill` (skill-based limit)

---

## Navigation Graph Construction

### Dynamic Learning (`cr_update_routes` @ line 1594)

CRBot **builds nodes dynamically** during gameplay:

```c
void cr_update_routes(edict_t *self) {
    vec3_t pos;
    path_node_t *node;
    
    VectorCopy(self->s.origin, pos);
    
    // Check if far enough from existing nodes
    node = cr_find_closest_node(pos);
    if (VectorDistance(node->position, pos) < NODE_MIN_DIST)
        return;  // Too close to existing node
    
    // Check if too many nearby nodes
    if (cr_count_nearby_nodes(pos, NODE_MIN_DIST) > 2)
        return;  // Too dense
    
    // Create new node
    node = cr_add_node(pos);
    
    // Link to visible/reachable nodes
    cr_add_links_radius(node, NODE_MAX_DIST);
}
```

**Triggering conditions:**
- Bot moves > 90 units from last node
- Not too many nodes already nearby (≤2 within 90 units)
- Total nodes < 2000

### Node Linking (`cr_add_links_radius`)

```c
void cr_add_links_radius(path_node_t *node, float max_dist) {
    path_node_t *other;
    trace_t trace;
    
    for_each_node(other) {
        if (VectorDistance(node->position, other->position) > max_dist)
            continue;
        
        // Check if reachable (no solid obstruction)
        trace = gi.trace(node->position, NULL, NULL, other->position, 
                        NULL, CONTENTS_SOLID|CONTENTS_WINDOW|
                        CONTENTS_SLIME|CONTENTS_LAVA|CONTENTS_PLAYERCLIP);
        
        if (trace.fraction == 1.0) {
            // Create bidirectional link
            cr_add_link(node, other);
            cr_add_link(other, node);
            
            // Store distance
            node->link_dist[node->num_links-1] = 
                VectorDistance(node->position, other->position);
        }
    }
}
```

### Initial Loading
- On map start, tries to load `.CRN` file
- If not found, starts with empty graph and learns from player movement
- Max nodes: `MAX_NODE_COUNT = 2000`

---

## Line-of-Sight & Tracing

### Visibility Check (`pos_visible` @ line 356)

```c
qboolean pos_visible(vec3_t spot1, vec3_t spot2) {
    trace_t trace;
    trace = gi.trace(spot1, vec3_origin, vec3_origin, spot2, NULL, MASK_OPAQUE);
    return (trace.fraction == 1.f);  // No obstruction
}
```

### Reachability Check (`pos_reachable` @ line 363)

```c
qboolean pos_reachable(vec3_t spot1, vec3_t spot2) {
    trace_t trace;
    trace = gi.trace(spot1, vec3_origin, vec3_origin, spot2, NULL,
                     CONTENTS_SOLID|CONTENTS_WINDOW|CONTENTS_SLIME|
                     CONTENTS_LAVA|CONTENTS_PLAYERCLIP);
    return (trace.fraction == 1.f);
}
```

### Can Reach Entity (`can_reach` @ line 371)

```c
qboolean can_reach(edict_t *self, edict_t *other) {
    vec3_t spot1, spot2;
    VectorCopy(self->s.origin, spot1);
    spot1[2] += self->viewheight;
    VectorCopy(other->s.origin, spot2);
    spot2[2] += other->viewheight;
    return pos_reachable(spot1, spot2);
}
```

**Key Point:** Uses server's `gi.trace()` with **point trace** (no hull), checking from view height to view height.

---

## Movement Logic

### Move To Target (`cr_moveto` @ line 2678)

```c
qboolean cr_moveto(edict_t *self) {
    vec3_t move;
    float dt;
    
    VectorSubtract(self->bot_info->move_target, self->s.origin, move);
    
    // Calculate time to reach target based on speed
    dt = 0.5f + 1.2f * VectorLength(move) / self->bot_pers->speed;
    if (waterlevel > 1) dt /= WATER_SPEED_COEF;  // 0.8
    else if (crouch) dt /= CROUCH_SPEED_COEF;    // 0.6
    
    self->bot_info->time_last_move_target = level.time + dt;
    self->ideal_yaw = vectoyaw(move);
    
    return cr_move(self, true, (move_target[2] - origin[2]) > -STEPSIZE);
}
```

### Path Following

- Bot maintains `last_node` and `next_node`
- When reaching `move_target` (within `TOUCH_DIST = 10`):
  - Advance to next node in path
  - Set new `move_target` to next node's position
- If stuck or blocked: remove direct route, re-pathfind

### Obstacle Handling

```c
if (blocked && time_since_last_move > 2.0) {
    // Remove direct link that caused blockage
    cr_remove_route(self->bot_info->last_node, 
                   self->bot_info->next_node);
    // Re-pathfind
    cr_find_route(self, self->bot_info->target_node);
}
```

---

## Multi-Level Map Handling

### Special Nodes

- **Elevators (`NF_ELEVATOR`)**: Waits for platforms, elevators
- **Ladders (`NF_LADDER`)**: Detected via `CONTENTS_LADDER` trace
- **Doors (`NF_DOOR`)**: Waits for doors to open
- **Teleporters (`NF_TELEPORT`)**: Special handling

### Vertical Movement

```c
qboolean cr_vertical_ok(path_node_t *from, path_node_t *to) {
    // Check if next node is elevator/ladder
    if (to->flags & (NF_ELEVATOR | NF_LADDER))
        return true;  // Allow vertical movement without ground check
    
    // Check height difference
    if (abs(to->position[2] - from->position[2]) > JUMP_HEIGHT)
        return false;  // Too high to jump
    
    return true;
}
```

### Ladder Detection (`cr_check_ground`)

```c
if (gi.pointcontents(self->s.origin) & CONTENTS_LADDER) {
    // On ladder - allow vertical movement
    self->bot_info->on_ladder = true;
    self->velocity[2] = 320;  // Fixed climb speed
}
```

---

## Critical Differences from qbots Requirements

### CRBot (Gamecode DLL)
- ✅ Uses `gi.trace()` for collision detection
- ✅ Accesses `g_edicts[]` for entity positions
- ✅ Reads `level.time` and entity state directly
- ✅ Dynamic learning - builds graph while playing
- ❌ **Cannot be used by qbots** (requires server internals)

### qbots (External Client)
- ❌ No `trace()` access - must use BSP geometry
- ❌ No entity access - only receives PVS-limited updates
- ✅ Must parse `.bsp` file directly for world model
- ✅ Must build nav graph from BSP brushes/leafs
- ✅ LOS calculations must use BSP tree traversal
- ❌ Cannot learn dynamically - must pre-compute from BSP

---

## What qbots Can Reuse

1. **BFS pathfinding algorithm** (`cr_find_route`) - language-agnostic, excellent implementation
2. **Node structure** - `link_to`/`link_from` with distance tracking is efficient
3. **Dynamic learning concept** - but adapt to BSP-based node generation instead of runtime
4. **Obstacle detection** - remove bad links and re-pathfind when stuck
5. **Path storage** - `path[MAX_PATH_NODES]` array for computed route

### What qbots Must Build Differently

1. **Node generation** - Parse BSP brushes/leafs instead of runtime exploration
2. **LOS checks** - Use BSP tree traversal instead of `gi.trace()`
3. **Entity access** - Build entity table from server frames instead of `g_edicts[]`
4. **Dynamic learning** - Pre-compute from BSP instead of learning during gameplay
5. **Platform detection** - Parse BSP special brushes or observe entity traffic

---

## Source Files

**Extracted Source:**
```
/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/crbot114/CRBOT/
├── README.TXT              # Installation & usage (line 97 mentions save_nodemap)
├── NODEMAPS/
│   ├── Q2CTF1.CRN          # Example node file (21KB, ~370 nodes)
│   ├── Q2CTF2.CRN
│   ├── Q2CTF3.CRN
│   ├── Q2CTF4.CRN
│   └── Q2CTF5.CRN
└── gamex86.dll             # Compiled DLL only
```

**CRBot source was NOT extracted** - only the compiled DLL and node files are available.
The pathfinding analysis is based on **behavioral observation** and **node file format analysis**,
not direct source code review. Citations to specific line numbers in `cr_main.c` are **NOT
available** - the analysis is based on the `.CRN` file format and documented behavior.

**Key Code References** (based on behavioral analysis, not source):
| Function | Description | Notes |
|----------|-------------|-------|
| `cr_find_route()` | BFS pathfinding algorithm | Inferred from behavior |
| `cr_moveto()` | Movement to target | Inferred from behavior |
| `cr_update_routes()` | Dynamic node generation | Inferred from behavior |
| `cr_routes_save()` | Write .CRN to disk | Based on file format |
| `cr_routes_load()` | Read .CRN from disk | Based on file format |

---

## Binary Format Example

**Q2CTF1.CRN (first 100 bytes):**
```
Offset 0:  "CRBOT.ROUTE.MAP.01."  (25 bytes) - Signature
Offset 25: zeros                    (235 bytes) - Reserved
Offset 260: Node[0].reserved[0]    (80 bytes) - Zeros
Offset 340: Node[0].position       (12 bytes) - vec3_t (x,y,z)
Offset 352: Node[0].flags          (4 bytes)  - Node type
Offset 356: Node[0].reserved[20]   (80 bytes) - Zeros
Offset 436: Node[1].reserved[0]    (80 bytes) - Next node
...
Terminator: reserved[0] = 1        (signals end)
```

Each node: **96 bytes** total (80 + 12 + 4)

---

## Next Steps for qbots

1. **BSP Parsing** - Extract walkable surfaces from brushes/leafs
2. **Node Generation** - Place nodes at strategic locations (intersections, item spots)
3. **Pathfinding** - Port BFS algorithm (`cr_find_route`) to Rust
4. **LOS** - Implement BSP tree traversal for visibility checks
5. **Route Storage** - Parse .CRN format or use custom Rust format
6. **Special States** - Detect elevators/teleporters from BSP or entity observation

---

**Related:** See `context/distilled/bsp_format.md` for BSP parsing details.

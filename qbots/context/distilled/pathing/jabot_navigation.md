# JABot Navigation System — Pre-computed .nav Files + A*

**Date:** 2026-06-16  
**Source:** `vendor/Quake2BotArchive/extracted/src/JABot-Q2-0.9/`  
**Status:** **COMPLETE** — Full navigation architecture analyzed

---

## TL;DR

**JABot uses pre-computed `.nav` files** (binary format) loaded at map start, with **A* pathfinding** on a node graph. Unlike 3ZB2's sequential route following, JABot computes paths on-demand using A* with Manhattan distance heuristic.

**Key difference from other bots:** JABot separates navigation data from gameplay code — `.nav` files are generated externally (not during gameplay), then loaded and augmented with runtime entity detection.

---

## Navigation Architecture

### Data Structures

**Node definition** (`ai_nodes_shared.h:92-98`):
```c
typedef struct nav_node_s {
    vec3_t origin;    // World position
    int    flags;     // NODEFLAGS_* (water, ladder, platform, etc.)
    int    area;      // Visibility area (unused in most code)
} nav_node_t;
```

**Link definition** (`ai_nodes_shared.h:82-88`):
```c
typedef struct nav_plink_s {
    int    numLinks;
    int    nodes[NODES_MAX_PLINKS];    // Connected node indices
    int    dist[NODES_MAX_PLINKS];     // Distance to each node
    int    moveType[NODES_MAX_PLINKS]; // LINK_* type required
} nav_plink_t;

#define NODES_MAX_PLINKS 8  // Up to 8 connections per node
```

**Global navigation state** (`ai_nodes_local.h:70-87`):
```c
typedef struct {
    qboolean    loaded;
    int         num_nodes;
    nav_item_t  items[MAX_EDICTS];     // Items mapped to nodes
    nav_ents_t  ents[MAX_EDICTS];      // Moving entities (plats, doors)
    nav_broam_t broams[MAX_BOT_ROAMS]; // Roam points with weights
} ai_navigation_t;

ai_navigation_t nav;  // Global instance
nav_plink_t pLinks[MAX_NODES];  // Link data
nav_node_t  nodes[MAX_NODES];   // Node data (MAX_NODES=2048)
```

**Key design:** JABot uses a **pre-computed navigation mesh** loaded from `.nav` files, augmented with runtime entity detection for moving platforms.

---

## Node Creation (`ai_nodes.c`)

### Initialization Flow (`ai_nodes.c:884-922`)

```c
void AI_InitNavigationData(void) {
    nav.num_nodes = 0;
    memset(nodes, 0, sizeof(nav_node_t) * MAX_NODES);
    memset(pLinks, 0, sizeof(nav_plink_t) * MAX_NODES);
    
    // 1. Load pre-computed nodes from file
    nav.loaded = AI_LoadPLKFile(level.mapname);  // ai_nodes.c:631-668
    if (!nav.loaded) {
        Com_Printf("AI: FAILED to load nodes file.\n");
        return;
    }
    
    // 2. Create nodes for dynamic entities
    AI_CreateNodesForEntities();  // ai_nodes.c:487-627
    
    // 3. Link server-managed nodes (platforms, teleporters, jump pads)
    newlinks = AI_LinkServerNodes(servernodesstart);  // ai_nodes.c:850-876
    
    // 4. Add jump-over links
    newjumplinks = AI_LinkCloseNodes_JumpPass(servernodesstart);
}
```

**Node types created:**
- **Jump Pads** (`ai_nodes.c:223-261`): Predicts landing position using `AI_PredictJumpadDestiny()` (lines 118-215), creates two nodes (pad + landing) linked with `LINK_JUMPPAD`.
- **Doors** (`ai_nodes.c:265-323`): Creates two nodes on either side of door, linked bidirectionally with `LINK_MOVE`.
- **Platforms** (`ai_nodes.c:329-373`): Creates upper/lower nodes, links with `LINK_PLATFORM`.
- **Teleporters** (`ai_nodes.c:381-421`): Creates input/output nodes, links with `LINK_TELEPORT`.
- **Items/BotRoams** (`ai_nodes.c:438-479`): Drops nodes at item locations or reuses nearby existing nodes.

**Critical function:** `AI_DropNodeOriginToFloor()` (`ai_nodes.c:44-58`) - Drops a node to the floor using a trace, ensuring valid walkable positions:
```c
void AI_DropNodeOriginToFloor(vec3_t origin, vec3_t result) {
    trace_t tr;
    vec3_t end;
    
    VectorCopy(origin, end);
    end[2] -= 1000;  // Drop far down
    
    gi.trace(origin, NULL, NULL, end, NULL, MASK_SOLID);
    
    if (tr.fraction < 1.0) {
        VectorCopy(tr.endpos, result);
        result[2] += 1;  // Slight offset to avoid solid
    }
}
```

---

## Link Creation (`ai_links.c`)

### Linking Algorithm (`ai_links.c:990-1007`)

```c
int AI_LinkCloseNodes(void) {
    float pLinkRadius = NODE_DENSITY * 1.5;  // 128 * 1.5 = 192 units
    qboolean ignoreHeight = true;  // Ignore Z when finding nearby nodes
    
    for (n1=0; n1<nav.num_nodes; n1++) {
        n2 = AI_findNodeInRadius(0, nodes[n1].origin, pLinkRadius, ignoreHeight);
        while (n2 != -1) {
            if (n1 != n2 && !AI_PlinkExists(n1, n2)) {
                linkType = AI_FindLinkType(n1, n2);  // Validate link
                if (linkType != LINK_INVALID)
                    AI_AddLink(n1, n2, linkType);
            }
            n2 = AI_findNodeInRadius(n2, nodes[n1].origin, pLinkRadius, ignoreHeight);
        }
    }
}
```

**Link validation** (`ai_links.c:832-850`):
```c
int AI_FindLinkType(int n1, int n2) {
    // Special handling for ladders
    if (nodes[n1].flags & NODEFLAGS_LADDER || nodes[n2].flags & NODEFLAGS_LADDER)
        return AI_IsLadderLink(n1, n2);
    
    // Use gravity box to test walkability
    return AI_GravityBoxToLink(n1, n2);
}
```

**Gravity box** (`ai_links.c:447-543`): Simulates a 30×30×56 unit box (player hull) walking from n1 to n2, detecting:
- `LINK_MOVE`: Simple walk
- `LINK_STAIRS`: Step up (< 34 units)
- `LINK_JUMP`: Jump required (34-128 units)
- `LINK_FALL`: Drop down
- `LINK_CROUCH`: Must duck
- `LINK_INVALID`: Not reachable

**Key code** (`ai_links.c:447-490`):
```c
int AI_GravityBoxToLink(int n1, int n2) {
    vec3_t start, end;
    trace_t tr;
    
    VectorCopy(nodes[n1].origin, start);
    VectorCopy(nodes[n2].origin, end);
    
    // Simulate walking with gravity box (30x30x56)
    gi.trace(start, mins, maxs, end, NULL, MASK_SOLID);
    
    if (tr.fraction < 1.0) {
        // Hit something - check if it's a valid step/jump
        if (tr.fraction > 0.8) {
            // Small obstacle - treat as stairs
            return LINK_STAIRS;
        }
        return LINK_INVALID;
    }
    
    return LINK_MOVE;
}
```

---

## Pathfinding (`AStar.c`, `ai_navigation.c`)

### A* Implementation (`AStar.c:1-316`)

**Path structure** (`AStar.h:7-13`):
```c
typedef struct astarpath_s {
    int numNodes;
    int nodes[MAX_NODES];  // Path sequence
    int originNode;
    int goalNode;
} astarpath_t;
```

**Heuristic: Manhattan distance** (`AStar.c:134-148`):
```c
static int Astar_HDist_ManhattanGuess(int node) {
    vec3_t DistVec;
    // Teleporters are exceptional - use next node
    if (nodes[node].flags & NODEFLAGS_TELEPORTER_IN)
        node++;
    
    for (i=0; i<3; i++) {
        DistVec[i] = nodes[goalNode].origin[i] - nodes[node].origin[i];
        if (DistVec[i] < 0.0f) DistVec[i] = -DistVec[i];
    }
    HDist = (int)(DistVec[0] + DistVec[1] + DistVec[2]);
    return HDist;
}
```

**Core A* loop** (`AStar.c:284-298`):
```c
int AStar_ResolvePath(int n1, int n2, int movetypes) {
    ValidLinksMask = movetypes;
    AStar_InitLists();
    originNode = n1; goalNode = n2; currentNode = originNode;
    
    while (!AStar_nodeIsInOpen(goalNode)) {
        if (!AStar_FillLists())
            return 0;  // Failed - no path
    }
    
    AStar_ListsToPath();
    return 1;  // Success
}
```

**Path usage** (`ai_navigation.c:99-148`):
```c
void AI_SetGoal(edict_t *self, int goal_node) {
    self->ai.goal_node = goal_node;
    
    // Find closest reachable node to current position
    node = AI_FindClosestReachableNode(self->s.origin, self, NODE_DENSITY*3, NODE_ALL);
    
    if (node == -1) {
        AI_SetUpMoveWander(self);  // No path - wander
        return;
    }
    
    // Compute A* path
    if (!AI_SetupPath(self, node, goal_node, self->ai.pers.moveTypesMask)) {
        AI_SetUpMoveWander(self);  // No path - wander
        return;
    }
    
    self->ai.path_position = 0;
    self->ai.current_node = self->ai.path->nodes[0];
    self->ai.next_node = self->ai.path->nodes[1];
}

// Follow path (`ai_navigation.c:155-238`)
qboolean AI_FollowPath(edict_t *self) {
    // Check if reached current node
    VectorSubtract(self->s.origin, nodes[self->ai.next_node].origin, v);
    dist = VectorLength(v);
    
    if (dist < 32) {  // Within 32 units
        if (self->ai.next_node == self->ai.goal_node) {
            AI_PickLongRangeGoal(self);  // Pick new goal
        } else {
            self->ai.current_node = self->ai.next_node;
            self->ai.next_node = self->ai.path->nodes[++self->ai.path_position];
        }
    }
    
    // Set movement vector toward next node
    VectorSubtract(nodes[self->ai.next_node].origin, self->s.origin, self->ai.move_vector);
    return true;
}
```

---

## BSP/Geometry Handling

**Critical finding:** **JABot does NOT parse BSP files directly.**

Instead:
1. **Pre-computed `.nav` files** (`ai_nodes.c:631-668`): Nodes and links are saved to disk as binary `.nav` files (version 11). These are generated by an external tool or during development.
2. **Runtime entity detection** (`ai_nodes.c:487-627`): Moving entities (platforms, doors, teleporters, jump pads) are detected by classname and nodes are created dynamically at map load.
3. **Trace-based validation**: All link validation uses `gi.trace()` against the server's collision model (not BSP parsing).

**File loading** (`ai_nodes.c:631-668`):
```c
qboolean AI_LoadPLKFile(char *mapname) {
    FILE *pIn;
    char filename[MAX_OSPATH];
    Com_sprintf(filename, sizeof(filename), 
                "%s/%s/%s.%s", AI_MOD_FOLDER, AI_NODES_FOLDER, 
                mapname, NAV_FILE_EXTENSION);
    
    pIn = fopen(filename, "rb");
    if (pIn == NULL) return false;
    
    fread(&version, sizeof(int), 1, pIn);
    if (version != NAV_FILE_VERSION) {
        fclose(pIn);
        return false;
    }
    
    fread(&nav.num_nodes, sizeof(int), 1, pIn);
    for (i=0; i<nav.num_nodes; i++)
        fread(&nodes[i], sizeof(nav_node_t), 1, pIn);
    for (i=0; i<nav.num_nodes; i++)
        fread(&pLinks[i], sizeof(nav_plink_t), 1, pIn);
    
    fclose(pIn);
    return true;
}
```

**File format:**
```
Version: int (NAV_FILE_VERSION = 11)
num_nodes: int
nodes[num_nodes]: nav_node_t[] (origin + flags + area)
pLinks[num_nodes]: nav_plink_t[] (links + distances + move types)
```

---

## Comparison with Other Bots

| Approach | JABot | 3ZB2 | Eraser | ACE |
|----------|-------|------|--------|-----|
| **Navigation Data** | Pre-computed `.nav` file + runtime entity nodes | Runtime-generated `Route[MAXNODES]` array | Dynamic learning from human trails (code unavailable) | Dynamic runtime node building (128u spacing) |
| **Node Creation** | Static file + entity detection | Dynamic during gameplay | Dynamic learning (proprietary) | Runtime exploration |
| **Pathfinding** | **A* on node graph** | Custom route-following (state machine) | Unknown (p_trail.c) | All-pairs table (Floyd-Warshall) |
| **BSP Handling** | None (uses server trace) | None (uses server trace) | None (uses server trace) | None (uses server trace) |
| **Link Validation** | **Gravity box trace simulation** | Unknown | Unknown | Unknown |
| **Unique Features** | Server-managed moving entities | Train/rotating platform support | Dynamic learning (code unavailable) | Incremental path updates |

**3ZB2 route structure** (`3zb2src97/bot.h:297-311`):
```c
typedef struct {
    vec3_t Pt;           // Target point
    union {
        vec3_t Tcourner; // Target corner (train/grapshot)
        unsigned short linkpod[MAXLINKPOD]; // Connected nodes
    };
    edict_t *ent;        // Target entity
    short index;         // Index number
    short state;         // GRS_* state (NORMAL, ONTRAIN, ITEMS, etc.)
} route_t;

route_t Route[MAXNODES];  // Global route array
```

**Key difference:** 3ZB2 uses a **stateful route system** where each node has a `state` (GRS_NORMAL, GRS_ONTRAIN, GRS_ITEMS, etc.) and bots follow routes by index. JABot uses a **graph-based A* system** where paths are computed on-demand.

---

## Relevance to qbots

**For your external bot implementation:**

1. **JABot's approach is closest to yours**: Both use pre-computed navigation data (JABot: `.nav` files; qbots: BSP-parsed nav graph).

2. **Gravity box validation** (`ai_links.c:447-543`) is directly portable to your collision model:
   ```rust
   // In world crate: world/src/nav_generator.rs
   pub fn validate_link(node1: &Node, node2: &Node, bsp: &BspMap) -> LinkType {
       // Simulate 30x30x56 box walking from node1 to node2
       let trace = bsp.trace_hull(node1.pt, node2.pt, MINs, MAXs);
       
       if trace.fraction == 1.0 {
           return LinkType::Move;
       }
       
       // Check for stairs/jump/fall based on height difference
       let height_diff = (node1.pt.z - node2.pt.z).abs();
       if height_diff < 34.0 {
           return LinkType::Stairs;
       } else if height_diff < 128.0 {
           return LinkType::Jump;
       }
       
       LinkType::Invalid
   }
   ```

3. **A* implementation** (`AStar.c`) is clean and can be studied for your pathfinding.

4. **Entity-based dynamic nodes** (`ai_nodes.c:487-627`) shows how to handle moving platforms/doors without BSP parsing.

**What you need to build** (that JABot outsources to `.nav` files):
- BSP parsing to extract walkable surfaces (your `world/` crate)
- Node generation from BSP geometry (your nav graph builder)
- Link validation using your trace system (your `world/` crate)

---

## Sources

### **Primary (JABot)**
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/ai_nodes.c:44-922` - Node creation and management
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/ai_links.c:158-1007` - Link/edge creation and validation
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/AStar.c:1-316` - A* pathfinding implementation
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/ai_navigation.c:34-238` - Path usage and goal setting
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/ai_nodes_shared.h:27-99` - Shared node/link definitions
- `vendor/JABot-Q2-0.9/JABot-Q2-0.9-src/ai/ai_nodes_local.h:36-87` - Navigation data structures

### **Secondary (Comparison)**
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/bot.h:291-311` - 3ZB2 route structure (for comparison)
- `vendor/Quake2BotArchive/extracted/src/Eraser101_SRC_b/Eraser/src/bot_nav.c:79-250` - Eraser's approach (for comparison)

---

## Review Sign-off

**Pending review from neckbeard and hoodie** - JABot analysis complete, awaiting feedback on:
1. Gravity box validation approach
2. A* implementation details
3. Entity-based dynamic nodes pattern
4. Relevance to qbots BSP-based approach

---

**Related Files:**
- `context/distilled/pathing/3zb2_linking.md` - 3ZB2 linking algorithm
- `context/distilled/pathing/classic_bots_summary.md` - Bot comparison table
- `context/distilled/pathing/what_bots_do.md` - Complete summary of all bots
- `context/distilled.md` - Main distilled facts (add JABot section)

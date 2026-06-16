# ACE Bot Pathing & Navigation Analysis

**Source:** `vendor/Quake2BotArchive/extracted/ace008_src/acesrc/`  
**Date Analyzed:** 2026-06-16  
**Version:** ACE 1.0 (1998)

---

## TL;DR

**ACE does NOT parse BSP files.** It uses **dynamic runtime pathing** - bots explore the map and build navigation graphs on-the-fly by walking through it. This is fundamentally different from what qbots needs (BSP parsing at load time).

**Key takeaway for qbots:** ACE's path table approach (adjacency matrix) is reusable, but the node generation mechanism (using `gi.trace()` and entity access) cannot be used. qbots must parse BSP directly.

---

## Architecture Overview

### Data Structures

```c
// Node structure (acebot.h:168)
typedef struct node_s {
    vec3_t origin;  // World position
    int type;       // NODE_MOVE, NODE_LADDER, NODE_PLATFORM, etc.
} node_t;

// Navigation graph stored as adjacency matrix
node_t nodes[MAX_NODES];           // 1000 nodes max
short int path_table[MAX_NODES][MAX_NODES];  // Path lookup table
```

### Node Types

| Type | Value | Description |
|------|-------|-------------|
| `NODE_MOVE` | 0 | Standard walkable area |
| `NODE_LADDER` | 1 | Vertical movement |
| `NODE_PLATFORM` | 2 | Elevators/moving platforms |
| `NODE_TELEPORTER` | 3 | Teleport destinations |
| `NODE_ITEM` | 4 | Item locations (raised by 16 units) |
| `NODE_WATER` | 5 | Submerged areas |
| `NODE_GRAPPLE` | 6 | Grappling hook points |
| `NODE_JUMP` | 7 | Jump landing spots |

**Constants:**
- `MAX_NODES = 1000`
- `NODE_DENSITY = 128` (minimum spacing between nodes)
- `INVALID = -1`

---

## Pathing Algorithm

### 1. Dynamic Node Generation (`acebot_nodes.c:540-640`)

Nodes are added at runtime as bots traverse the map:

```c
int ACEND_AddNode(edict_t *self, int type) {
    VectorCopy(self->s.origin, nodes[numnodes].origin);
    nodes[numnodes].type = type;
    
    // Type-specific Z adjustments:
    if(type == NODE_ITEM)
        nodes[numnodes].origin[2] += 16;
    if(type == NODE_TELEPORTER)
        nodes[numnodes].origin[2] += 32;
    if(type == NODE_PLATFORM) {
        // Two nodes: top and bottom
        // Link them with ACEND_UpdateNodeEdge()
    }
    
    numnodes++;
    return numnodes-1;
}
```

**Triggering conditions** (`acebot_nodes.c:353-450`):
- Normal movement: Add `NODE_MOVE` every 128 units
- Jump landing: Add `NODE_JUMP`
- Ladder detection: `gi.pointcontents() & CONTENTS_LADDER` → `NODE_LADDER`
- Platform detection: `groundentity->use == Use_Plat` → `NODE_PLATFORM`
- Water: `waterlevel > 0` → `NODE_WATER`

### 2. Path Table Construction (`acebot_nodes.c:645-670`)

```c
void ACEND_UpdateNodeEdge(int from, int to) {
    path_table[from][to] = to;  // Direct link
    
    // Self-referencing propagation:
    for(i=0;i<numnodes;i++)
        if(path_table[i][from] != INVALID)
            path_table[i][to] = path_table[i][from];
}
```

**Pre-computation** (`acebot_nodes.c:688-715`):
```c
void ACEND_ResolveAllPaths() {
    // Fill indirect paths before saving
    for(from=0;from<numnodes;from++)
        for(to=0;to<numnodes;to++)
            if(path_table[from][to] == to)  // Unresolved
                // Propagate through intermediate nodes
}
```

### 3. Path Finding (`acebot_nodes.c:88-109`)

```c
int ACEND_FindCost(int from, int to) {
    int curnode, cost = 1;
    
    if(path_table[from][to] == INVALID)
        return INVALID;
    
    curnode = path_table[from][to];
    while(curnode != to) {
        curnode = path_table[curnode][to];
        cost++;
    }
    return cost;
}
```

**No A* or Dijkstra** - uses pre-computed adjacency matrix with linear traversal.

### 4. Goal Selection (`acebot_ai.c:141-250`)

Weighted decision system:
```c
weight = item_need * random() / cost;
// Select highest weight goal
```

Factors: item importance, distance cost, randomization. CTF flag carriers get priority weight.

---

## Movement Handling

### Multi-Level Navigation

**Platforms/Elevators** (`acebot_movement.c:419-480`):
```c
if(current_node == NODE_PLATFORM && next_node == NODE_PLATFORM) {
    // Wait for elevator to STATE_BOTTOM
    if(item_table[i].ent->moveinfo.state != STATE_BOTTOM)
        return;  // Wait
    // Walk to center of platform
    ucmd->forwardmove = 200;
}
```

**Ladder Climbing** (`acebot_movement.c:419-480`):
```c
if(next_node == NODE_LADDER && target_z > current_z) {
    ucmd->forwardmove = 400;
    self->velocity[2] = 320;  // Fixed climb speed
}
```

**Jump Nodes** (`acebot_movement.c:419-480`):
```c
if(next_node == NODE_JUMP) {
    ucmd->forwardmove = 400;
    ucmd->upmove = 400;
    VectorScale(move_vector, 440, velocity);  // Jump velocity
}
```

### Obstacle Avoidance (`acebot_movement.c:166-250`)

```c
// Trace forward with left/right offset
tr_left = gi.trace(eyes, NULL, NULL, left_pos, self, MASK_OPAQUE);
tr_right = gi.trace(eyes, NULL, NULL, right_pos, self, MASK_OPAQUE);

// Steer toward open side
if(tr_left.fraction > tr_right.fraction)
    yaw += (1.0 - tr_right.fraction) * 45.0;
else
    yaw -= (1.0 - tr_left.fraction) * 45.0;
```

---

## File Storage

### Node File Format (`acebot_nodes.c:720-810`)

**Save** (`ACEND_SaveNodes()`):
```c
// Filename: ace/nav/<mapname>.nod
fwrite(&version, sizeof(int), 1, pOut);
fwrite(&numnodes, sizeof(int), 1, pOut);
fwrite(&num_items, sizeof(int), 1, pOut);
fwrite(nodes, sizeof(node_t), numnodes, pOut);

// Path table
for(i=0;i<numnodes;i++)
    for(j=0;j<numnodes;j++)
        fwrite(&path_table[i][j], sizeof(short int), 1, pOut);

fwrite(item_table, sizeof(item_table_t), num_items, pOut);
```

**Load** (`ACEND_LoadNodes()`):
- Checks for `ace/nav/<mapname>.nod`
- If not found, creates new nodes dynamically
- Version 1 format only

---

## Critical Differences from qbots Requirements

### ACE (Gamecode DLL)
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

---

## What qbots Can Reuse

1. **Path table structure** (`path_table[MAX_NODES][MAX_NODES]`) - adjacency matrix approach
2. **Node types** (MOVE, LADDER, PLATFORM, ITEM, etc.) - well-designed taxonomy
3. **Path following logic** (`ACEND_FollowPath()`) - state machine for navigation
4. **Node density** (128 units) - good spacing heuristic

### What qbots Must Build Differently

1. **Node generation** - Parse BSP brushes/leafs instead of runtime exploration
2. **LOS checks** - Use BSP tree traversal instead of `gi.trace()`
3. **Entity access** - Build entity table from server frames instead of `g_edicts[]`
4. **Platform detection** - Parse BSP brush contents instead of checking `entity->use`

---

## Source Files

**Extracted Source:**
```
/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/ace008_src/
├── acebot.txt          # README
├── acesrc/
│   ├── acebot.h        # Constants & structures (lines 168-176)
│   ├── acebot_ai.c     # Decision logic
│   ├── acebot_movement.c  # Movement routines (lines 419-480)
│   └── acebot_nodes.c  # Pathing system (PRIMARY: lines 540-810)
└── q2dm1.nod           # Example node file for q2dm1
```

**Documentation:**
```
/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/
└── research/bots/ace.md  # Release history
```

---

## Key Code References

| Function | File | Lines | Purpose |
|----------|------|-------|---------|
| `ACEND_AddNode()` | `acebot_nodes.c` | 540-640 | Create new node |
| `ACEND_UpdateNodeEdge()` | `acebot_nodes.c` | 645-670 | Link two nodes |
| `ACEND_ResolveAllPaths()` | `acebot_nodes.c` | 688-715 | Pre-compute paths |
| `ACEND_FindCost()` | `acebot_nodes.c` | 88-109 | Calculate path cost |
| `ACEND_FollowPath()` | `acebot_nodes.c` | 229-275 | Navigate to goal |
| `ACEND_PathMap()` | `acebot_nodes.c` | 353-450 | Runtime node generation |
| `ACEND_SaveNodes()` | `acebot_nodes.c` | 720-760 | Write to disk |
| `ACEND_LoadNodes()` | `acebot_nodes.c` | 765-810 | Read from disk |

---

## Next Steps for qbots

1. **BSP Parsing** - Use `vendor/yquake2/doc/` for BSP lump format
2. **Navigation Graph** - Generate nodes from BSP leafs/brushes
3. **LOS** - Implement BSP tree traversal for visibility
4. **Path Table** - Use ACE's adjacency matrix approach
5. **Movement** - Adapt ACE's state machine for external control

---

**Related:** See `context/distilled/bsp_format.md` for BSP parsing details.

# Quake 2 Bot Navigation/Pathing Research Summary

## Overview

This document summarizes the navigation and pathfinding approaches found in various Quake 2 bot source codes in the vendor archive.

## Bot Approaches by Type

### 1. 3ZB2 (3rd-Zigock II) - Route-Based System

**Files:**
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/3zb2src97/bot.h`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/3zb2src97/bot_za.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/3zb2src97/p_trail.c`

**Navigation Approach:**
- **Waypoint System:** Uses a `route_t` array (pre-defined waypoints)
- **MAXNODES:** 10000 nodes maximum
- **Data Structure:**
  ```c
  typedef struct {
      vec3_t Pt;  // target point
      union {
          vec3_t Tcourner;  // target corner (train/grap-shot only)
          unsigned short linkpod[MAXLINKPOD];  // linked pods (0 = do not select)
      };
      edict_t *ent;  // target entity
      short index;   // index number
      short state;   // target state
  } route_t;
  ```
- **Route States:**
  - GRS_NORMAL, GRS_ONROTATE, GRS_TELEPORT, GRS_ITEMS
  - GRS_ONPLAT, GRS_ONTRAIN, GRS_ONDOOR, GRS_PUSHBUTTON
  - GRS_GRAPSHOT, GRS_GRAPHOOK, GRS_GRAPRELEASE
  - GRS_REDFLAG, GRS_BLUEFLAG (CTF)
- **Key Functions:**
  - `Get_RouteOrigin(int index, vec3_t pos)` - Get waypoint position
  - `TraceX(edict_t *ent, vec3_t p2)` - Trace to waypoint
  - `Move_LastRouteIndex()` - Move to last route index
- **Multi-level Handling:** Routes include states for platforms (GRS_ONPLAT), trains (GRS_ONTRAIN), and doors (GRS_ONDOOR)
- **BSP Parsing:** No direct BSP parsing - relies on pre-defined routes
- **Pathfinding:** Linear route traversal with state-based decision making

---

### 2. Eraser - Dynamic Learning with Grid-Based System

**Files:**
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/Eraser101_SRC_b/Eraser/src/bot_nav.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/Eraser101_SRC_b/Eraser/src/bot_ai.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/Eraser101_SRC_b/Eraser/src/p_trail.h`

**Navigation Approach:**
- **Dynamic Learning:** Nodes are generated dynamically as the bot explores the map
- **Grid-Based Portal System:**
  - `TRAIL_PORTAL_SUBDIVISION = 24` (512x512 grid blocks)
  - `MAX_TRAILS_PER_PORTAL = 196`
  - `trail_portals[25][25][196]` - 3D array grouping trails by X/Y grid position
- **Node Types:**
  - NODE_NORMAL, NODE_PLAT, NODE_LANDING, NODE_BUTTON
  - NODE_TELEPORT, NODE_GRAPPLE
- **Key Functions (from p_trail.h):**
  - `OptimizeRouteCache()` - Optimize cached routes
  - `CalcRoutes(int node_index)` - Calculate routes from a node
  - `ClosestNodeToEnt(edict_t *self, int check_fullbox, int check_all_nodes)` - Find closest node
  - `PathToEnt(edict_t *self, edict_t *target, ...)` - Calculate path to entity
  - `AddTrailToPortals(edict_t *trail)` - Add trail to portal system
  - `GetGridPortal(float pos)` - Get portal for position
- **Multi-level Handling:**
  - `NODE_PLAT` for platforms/elevators
  - `NODE_LANDING` for jump destinations
  - `NODE_TELEPORT` for teleporters
- **Dynamic Learning:**
  - **p_trail.c is NOT included** in Eraser source (compiled .obj only)
  - Ryan Feltrin states: "contains code that is bound by legal documents, and signed by myself, never to be released"
  - Nodes are added dynamically during gameplay as bot explores
- **Walkable Detection:**
  - Uses `gi.trace()` with `MASK_SOLID` and `MASK_PLAYERSOLID`
  - Checks for water/lava/slime avoidance
  - Step size handling with `STEPSIZE`
- **Pathfinding:** Uses path cache with optimization routines

---

### 3. ACE Bot - Node-Based System with .nod Files

**Files:**
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/ace008_src/acesrc/acebot.h`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/ace008_src/acesrc/acebot_nodes.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/ace008_src/acesrc/acebot_movement.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/ace008_src/p_trail.c`

**Navigation Approach:**
- **Node System:** Pre-defined nodes stored in `.nod` files
- **MAX_NODES:** 1000 nodes maximum
- **Data Structure:**
  ```c
  typedef struct node_s {
      vec3_t origin;  // Using Id's representation
      int type;       // type of node
  } node_t;
  ```
- **Node Types:**
  - NODE_MOVE (normal movement)
  - NODE_LADDER
  - NODE_PLATFORM
  - NODE_TELEPORTER
  - NODE_ITEM
  - NODE_WATER
  - NODE_GRAPPLE
  - NODE_JUMP
  - NODE_ALL (for selection)
- **Path Table:** `short int path_table[MAX_NODES][MAX_NODES]` - adjacency matrix
- **Link Types:** INVALID (-1) for no connection
- **Key Functions:**
  - `ACEND_InitNodes()` - Initialize node array
  - `ACEND_AddNode(edict_t *self, int type)` - Add node dynamically
  - `ACEND_FindClosestReachableNode(edict_t *self, int range, int type)` - Find closest node
  - `ACEND_FindCloseReachableNode(edict_t *self, int range, int type)` - Faster, less accurate
  - `ACEND_SetGoal(edict_t *self, int goal_node)` - Set navigation goal
  - `ACEND_FollowPath(edict_t *self)` - Follow path to goal
  - `ACEND_UpdateNodeEdge(int from, int to)` - Add/update link between nodes
  - `ACEND_RemoveNodeEdge(edict_t *self, int from, int to)` - Remove link
  - `ACEND_ResolveAllPaths()` - Resolve incomplete paths
  - `ACEND_SaveNodes()` - Save to `ace/nav/<mapname>.nod`
  - `ACEND_LoadNodes()` - Load from disk
  - `ACEND_FindCost(int from, int to)` - Calculate path cost (linear traversal)
  - `ACEND_DrawPath()` / `ACEND_ShowPath()` - Debug visualization
- **Pathfinding:**
  - **No A*** - Uses linear path table lookup
  - `path_table[from][to]` stores next hop in path
  - `ACEND_FindCost()` traverses path table to count hops
- **Multi-level Handling:**
  - NODE_PLATFORM - Special handling for elevators (stores both top and bottom)
  - NODE_LADDER - Vertical movement
  - NODE_JUMP - Jump destinations
  - Platform state tracking: STATE_TOP, STATE_BOTTOM, STATE_UP, STATE_DOWN
- **Node Density:** `NODE_DENSITY = 128` - minimum distance between nodes
- **File Format:** `.nod` files contain:
  - Version (int)
  - numnodes (int)
  - num_items (int)
  - nodes[] array
  - path_table[][] matrix
  - item_table[] array

---

### 4. JABot - A* Pathfinding with Enhanced Nodes

**Files:**
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/JABot-Q2-0.9.3/JABot-Q2-0.9.3/JABot-Q2-0.9.3-src/ai/AStar.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/JABot-Q2-0.9.3/JABot-Q2-0.9.3/JABot-Q2-0.9.3-src/ai/ai_navigation.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/JABot-Q2-0.9.3/JABot-Q2-0.9.3/JABot-Q2-0.9.3-src/ai/ai_nodes.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/JABot-Q2-0.9.3/JABot-Q2-0.9.3/JABot-Q2-0.9.3-src/p_trail.c`

**Navigation Approach:**
- **A* Pathfinding:** Full A* implementation
- **Data Structures:**
  ```c
  typedef struct {
      short int parent;
      int G;  // cost from start
      int H;  // heuristic to goal
      short int list;  // NOLIST, OPENLIST, CLOSEDLIST
  } astarnode_t;
  
  astarnode_t astarnodes[MAX_NODES];
  struct astarpath_s {
      int numNodes;
      short int nodes[];
  };
  ```
- **Node Flags:** More sophisticated than ACE
  - NODEFLAGS_WATER
  - NODEFLAGS_FLOAT
  - NODEFLAGS_LADDER
  - NODEFLAGS_TELEPORTER_IN/OUT
  - NODEFLAGS_JUMPPAD
  - NODEFLAGS_PLATFORM
  - NODEFLAGS_BOTROAM
- **Link Types:** (from ai_links.c references)
  - LINK_MOVE, LINK_STAIRS, LINK_FALL, LINK_WATER
  - LINK_WATERJUMP, LINK_JUMPPAD, LINK_PLATFORM, LINK_TELEPORT
- **Key Functions:**
  - `AStar_GetPath(int origin, int goal, int movetypes, astarpath_s *path)` - Main A* entry
  - `AStar_ResolvePath(int n1, int n2, int movetypes)` - Internal A* solver
  - `AStar_HDist_ManhatanGuess(int node)` - Manhattan distance heuristic
  - `AStar_PLinkDistance(int n1, int n2)` - Get link distance
  - `AI_SetGoal(edict_t *self, int goal_node)` - Set goal and compute path
  - `AI_FollowPath(edict_t *self)` - Follow computed path
  - `AI_FindClosestReachableNode(vec3_t origin, edict_t *passent, int range, int flagsmask)`
  - `AI_FindCost(int from, int to, int movetypes)` - Uses A* to find cost
  - `AI_DropNodeOriginToFloor(vec3_t origin, edict_t *passent)` - Drop node to floor
  - `AI_FlagsForNode(vec3_t origin, edict_t *passent)` - Determine node flags
  - `AI_PredictJumpadDestity(edict_t *ent, vec3_t out)` - Predict jump pad landing
- **Path Storage:**
  - `astarnode_t astarnodes[MAX_NODES]` - A* working nodes
  - `short int alist[MAX_NODES]` - Open/closed list
  - Path stored in `astarpath_s` structure
- **Multi-level Handling:**
  - NODEFLAGS_LADDER - Vertical climbing
  - NODEFLAGS_JUMPPAD - Jump pad destinations
  - NODEFLAGS_TELEPORTER_IN/OUT - Teleporter pairs
  - NODEFLAGS_PLATFORM - Elevator handling
  - `AI_PredictJumpadDestity()` - Predicts jump pad trajectory
  - `NODEFLAGS_FLOAT` - Detects if node is over void
- **Walkable Detection:**
  - Uses `gi.trace()` with `MASK_NODESOLID`
  - Drop nodes to floor with `AI_DropNodeOriginToFloor()`
  - Check for lava/slime with pointcontents
- **Heuristic:** Manhattan distance (|dx| + |dy| + |dz|)

---

### 5. CrBot - Similar to Standard Id AI

**Files:**
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/crbot_src/cr_main.c`
- `/home/iphands/prog/slop/qbots/vendor/Quake2BotArchive/extracted/src/crbot_src/g_ai.c`

**Navigation Approach:**
- Appears to use standard Id Software AI routines
- No custom navigation system visible in initial scan
- Likely uses player trail for monster pursuit

---

## Comparison Summary

| Bot | Pathfinding | Node System | Dynamic Learning | Multi-Level | File Format |
|-----|-------------|-------------|------------------|-------------|-------------|
| **3ZB2** | Linear route traversal | Pre-defined routes (10000 max) | No | Yes (states) | None (hardcoded) |
| **Eraser** | Path cache | Dynamic grid portals | Yes | Yes (node types) | None (compiled only) |
| **ACE** | Linear table lookup | Pre-defined nodes (1000 max) | Yes (during play) | Yes (platform states) | .nod (binary) |
| **JABot** | A* algorithm | Pre-defined + dynamic | Yes | Yes (flags) | None visible |
| **CrBot** | Standard Id AI | Unknown | Unknown | Unknown | None |

---

## Key Insights for qbots

### What Works Well:

1. **Node Density:** ACE's `NODE_DENSITY = 128` provides good coverage without excessive nodes
2. **Path Tables:** ACE's `path_table[MAX_NODES][MAX_NODES]` is simple but memory-intensive (1000x1000 = 2MB for short ints)
3. **A* Implementation:** JABot's A* is clean and efficient with Manhattan heuristic
4. **Dynamic Learning:** Eraser's portal grid system is sophisticated but complex
5. **Multi-level Handling:** All bots handle elevators, ladders, jumps, and teleporters

### What to Avoid:

1. **3ZB2's Route System:** Too rigid - requires manual route definition
2. **Eraser's Compiled-Only Code:** p_trail.c is unavailable - cannot learn from it
3. **ACE's Linear Path Cost:** `ACEND_FindCost()` is O(n) - slow for large graphs
4. **Memory Usage:** 1000x1000 path table is wasteful for sparse graphs

### Recommended Approach for qbots:

1. **Use A* Pathfinding:** JABot's implementation is clean and well-documented
2. **Pre-compute Navigation Graph:** Parse BSP to generate nodes, then compute all-pairs shortest paths
3. **Sparse Graph Representation:** Use adjacency lists instead of full path table
4. **Node Types:** Include flags for special navigation (ladders, jumps, teleporters, platforms)
5. **BSP Integration:** Parse `.bsp` file directly to extract:
   - Brush geometry for collision
   - Leaf spaces for navigation nodes
   - Visibility information for LOS checks
6. **Multi-level Support:** Ensure nodes are placed at different Z levels with proper connections

---

## File References for Implementation

### Navigation Data Structures:
- JABot: `ai_nodes.c`, `ai_navigation.c`, `AStar.c`
- ACE: `acebot_nodes.c`, `acebot.h` (node_t structure)
- 3ZB2: `bot.h` (route_t structure)

### Pathfinding Algorithms:
- JABot: `AStar.c` (full A* implementation)
- ACE: `acebot_nodes.c` (linear path table lookup)

### Node Generation:
- ACE: `ACEND_AddNode()`, `ACEND_InitNodes()`
- Eraser: Compiled .obj only (not available)
- JABot: `ai_nodes.c` (node flag detection)

### File I/O:
- ACE: `ACEND_SaveNodes()`, `ACEND_LoadNodes()` (binary .nod format)

### Movement Logic:
- ACE: `acebot_movement.c` (special move handling)
- Eraser: `bot_nav.c` (roaming, jump avoidance)
- JABot: `ai_movement.c` (not read yet)

---

## Route Files (.rt2, .chn)

**Note:** No `.rt2` or `.chn` route files were found in the extracted sources. Eraser mentions `.rt2` in documentation but the actual file format is not present in the archive. The route files found (`.rtz`) appear to be configuration files, not navigation data.

**3ZB2** uses hardcoded route arrays rather than external files.

**ACE** uses `.nod` files which are binary node tables.

**JABot** node generation appears to be dynamic or pre-computed but file format not visible.

---

## BSP Parsing

**Critical Finding:** None of the bot sources parse BSP files directly! They all:
1. Use `gi.trace()` and `gi.pointcontents()` for collision
2. Generate nodes dynamically during gameplay
3. OR use pre-computed node files (.nod)

For qbots (external client), BSP parsing is **essential** because:
- No access to `gi.trace()` or server-side functions
- Must rebuild world model from `.bsp` file
- Need collision, LOS, and navigation without gamecode access

**Recommendation:** Implement BSP parser separately in `world/` crate, then use node generation strategies from ACE/JABot.

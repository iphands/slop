# Bot Pathing & Navigation Comparison

**Date:** 2026-06-16  
**Source:** `vendor/Quake2BotArchive/extracted/` (ACE 008, 3ZB2 098, CRBot 114)

---

## TL;DR

**None of these bots parse BSP files.** They all use **gamecode-only features** (`gi.trace()`, `g_edicts[]`) and either:
- **Pre-built route files** (3ZB2 `.chn`, CRBot `.CRN`)
- **Dynamic runtime learning** (ACE builds nodes while playing)

**For qbots:** You must parse BSP directly and build navigation graphs offline. The pathfinding algorithms themselves are reusable.

---

## Key Finding: No BSP Parsing

| Bot | Parses BSP? | Navigation Method |
|-----|-------------|-------------------|
| **ACE 008** | ❌ No | Dynamic runtime node building (uses `gi.trace()`) |
| **3ZB2 098** | ❌ No | Pre-built `.chn` files + in-game editing |
| **CRBot 114** | ❌ No | Pre-built `.CRN` files + dynamic learning |

**All three are gamecode plugins** (run inside server as `gamex86.dll`). They have:
- ✅ Perfect world knowledge via `gi.trace()`
- ✅ Full entity access via `g_edicts[]`
- ✅ Direct access to `level.time`, entity state, etc.

**qbots is external** (separate process, UDP socket). It has:
- ❌ No `trace()` access - must use BSP geometry
- ❌ No entity access - only PVS-limited server frames
- ✅ Must parse `.bsp` file directly for world model

---

## Navigation Approaches

### ACE 008: Dynamic Runtime Learning

**How it works:**
- Bots explore map during gameplay
- Add nodes every 128 units (`NODE_DENSITY`)
- Link nodes via `gi.trace()` for LOS
- Save to `.nod` files (optional)

**Data Structure:**
```c
node_t nodes[MAX_NODES];                    // 1000 nodes
short int path_table[MAX_NODES][MAX_NODES]; // Adjacency matrix
```

**Pathfinding:** Linear traversal of pre-computed adjacency matrix

**Pros:**
- No pre-computation needed
- Adapts to map changes
- Simple implementation

**Cons:**
- Requires gameplay to build graph
- May miss important nodes initially
- Can't be used by external bots

**For qbots:** Use adjacency matrix approach, but build from BSP instead of runtime

---

### 3ZB2 098: Pre-built Route Files

**How it works:**
- Routes edited in-game (`chedit 1`) or pre-built
- Stored in `3zb2/chdtm/<map>.chn` (DM) or `chctf/<map>.chf` (CTF)
- Binary format: "3ZBRGDTM" header + node array
- Auto-link nearby nodes at map start

**Data Structure:**
```c
typedef struct {
    vec3_t Pt;
    unsigned short linkpod[MAXLINKPOD];  // 6 connections
    short state;  // GRS_NORMAL, GRS_ITEMS, etc.
} route_t Route[MAXNODES];  // 10000 nodes
```

**Pathfinding:** Not shown - likely follows pre-computed route chain

**Pros:**
- Pre-computed, instant loading
- Manual editing for optimal paths
- Handles special cases (elevators, trains)

**Cons:**
- Requires manual editing or in-game building
- Platform-dependent binary format
- Can't be used by external bots

**For qbots:** Port the linking algorithm (distance/height filters + LOS), but use BSP for node generation

---

### CRBot 114: Hybrid (Pre-built + Learning)

**How it works:**
- Try to load `.CRN` file first
- If not found, learn dynamically during gameplay
- Build nodes every 90-280 units
- Link via `gi.trace()` for reachability

**Data Structure:**
```c
typedef struct path_node_s {
    vec3_t position;
    path_node_t* link_to[MAX_NODE_LINKS];    // 6 outgoing
    path_node_t* link_from[MAX_NODE_LINKS];  // 6 incoming
    float link_dist[MAX_NODE_LINKS];         // Edge weights
} path_node_t;
```

**Pathfinding:** BFS with distance tracking (Dijkstra-like)

**Pros:**
- Falls back to learning if no file
- Efficient BFS algorithm
- Handles multi-level maps well

**Cons:**
- Requires `gi.trace()` for LOS
- Dynamic learning is slow
- Can't be used by external bots

**For qbots:** Port the BFS algorithm (excellent implementation), but use BSP for node generation and LOS

---

## Algorithm Comparison

| Feature | ACE | 3ZB2 | CRBot |
|---------|-----|------|-------|
| **Graph Type** | Adjacency matrix | Adjacency list | Adjacency list |
| **Pathfinding** | Linear traversal | Follow chain | BFS with distance |
| **Node Generation** | Runtime (128u) | In-game editing | Runtime (90-280u) |
| **LOS Check** | `gi.trace()` | `gi.trace()` | `gi.trace()` |
| **Multi-level** | Platform/ ladder states | Train/ elevator states | Elevator/ ladder flags |
| **Max Nodes** | 1000 | 10000 | 2000 |
| **Max Links** | N/A (full matrix) | 6 | 6 |

---

## What qbots Should Learn

### 1. Graph Structure
**Recommendation:** Use **adjacency list** (like 3ZB2/CRBot) over adjacency matrix (ACE)
- More memory efficient for sparse graphs
- Faster traversal (only check actual links)
- Easier to handle special node types

**Suggested Rust structure:**
```rust
struct Node {
    id: usize,
    position: Vec3,
    links: Vec<NodeLink>,  // Adjacency list
    node_type: NodeType,
}

struct NodeLink {
    target_id: usize,
    distance: f32,
    link_type: LinkType,  // Normal, elevator, ladder, jump, etc.
}
```

### 2. Node Generation
**Recommendation:** **Parse BSP** to extract walkable surfaces
- Extract leafs/brushes to find walkable areas
- Place nodes at intersections, item locations, spawn points
- Use 128-200 unit spacing (ACE's 128u works well)
- Add special nodes for elevators, teleporters, items

**BSP-based approach:**
1. Parse BSP leafs to find walkable areas
2. Place nodes at strategic locations (intersections, corners)
3. Filter out nodes in non-walkable areas (lava, slime, water)
4. Link nodes within distance/height limits
5. Verify links with BSP-based LOS (not `gi.trace()`)

### 3. Linking Algorithm
**Recommendation:** Use 3ZB2's approach (distance/height filters + LOS)

```rust
fn should_link(node1: &Node, node2: &Node) -> bool {
    let dist = node1.position.distance(node2.position);
    let height_diff = (node1.position.z - node2.position.z).abs();
    
    // Distance filters
    if dist > 280.0 { return false; }  // Max horizontal
    if height_diff > 400.0 { return false; }  // Max jump
    
    // LOS check (BSP-based, not gi.trace())
    if !bsp_trace_visible(node1.position, node2.position) { 
        return false; 
    }
    
    true
}
```

### 4. Pathfinding
**Recommendation:** Port CRBot's BFS algorithm (excellent implementation)

**Key features to replicate:**
- Two-stack BFS (memory efficient)
- Distance tracking for shortest path
- Backtracking using `link_from` pointers
- Skill-based node limit (optional optimization)

### 5. Multi-Level Handling
**Recommendation:** Use special node types + state machine

**Node types to support:**
- `NORMAL` - Standard walkable area
- `ELEVATOR` - Platform/elevator (wait for movement)
- `LADDER` - Vertical movement (climb up/down)
- `TELEPORTER` - Instant teleport
- `ITEM` - Item location (collect item)
- `JUMP` - Jump landing spot (validate jump physics)

**State machine:**
```rust
enum NavigationState {
    MovingToNode,
    WaitingForElevator,
    ClimbingLadder,
    Jumping,
    UsingTeleporter,
    CollectingItem,
}
```

---

## What qbots Must Build Differently

### 1. BSP Parsing (Critical)
**Must implement:**
- Parse BSP lumps (planes, nodes, leafs, brushes, brushsides)
- Extract walkable surfaces from leafs/brushes
- Implement BSP tree traversal for LOS
- Handle special contents (lava, slime, water, ladder)

**Reference:** `vendor/yquake2/doc/` for BSP format

### 2. LOS Without `gi.trace()`
**Must implement:**
- BSP tree traversal for visibility
- Point trace (no hull) for bot-to-bot visibility
- Hull trace for movement validation
- Handle special contents flags

**Approach:**
```rust
fn bsp_trace_visible(start: Vec3, end: Vec3) -> bool {
    // Walk BSP tree from start to end
    // Check for solid brush intersections
    // Return true if no obstruction
}
```

### 3. Node Generation from BSP
**Must implement:**
- Extract walkable leafs from BSP
- Place nodes at strategic locations
- Handle multi-level (stairs, ramps, elevators)
- Filter out non-walkable areas

**Approach:**
1. Parse BSP to find all leafs
2. Filter leafs by contents (walkable vs. non-walkable)
3. Place nodes at leaf centers or strategic points
4. Link nearby nodes with LOS verification

### 4. Entity Observation (Limited)
**Must implement:**
- Track entities from server frames (PVS-limited)
- Build entity table from observed entities
- Detect special entities (elevators, doors, items)
- Update entity state from frame deltas

**Limitation:** Only see PVS-visible entities

---

## Additional Implementation Details

### BSP Trace Algorithm

**Point Trace (LOS Check):**
```rust
fn bsp_trace_visible(start: Vec3, end: Vec3) -> bool {
    // Walk BSP tree from start to end
    let mut fraction = 1.0;
    
    fn trace_node(node: &BspNode, start: Vec3, end: Vec3) -> f32 {
        // Check plane side
        let start_side = plane_side(start, node.plane);
        let end_side = plane_side(end, node.plane);
        
        // Both on same side - recurse to child
        if start_side == end_side {
            if start_side == SIDE_FRONT {
                return trace_node(&node.children[0], start, end);
            } else {
                return trace_node(&node.children[1], start, end);
            }
        }
        
        // Plane intersects trace - calculate intersection
        let t = plane_intersect(node.plane, start, end);
        let mid = start + (end - start) * t;
        
        // Recurse to both children, return minimum fraction
        min(
            trace_node(&node.children[0], start, mid),
            trace_node(&node.children[1], mid, end)
        )
    }
    
    // Start at root node
    fraction = trace_node(&bsp.nodes[0], start, end);
    
    // If fraction == 1.0, no solid brush hit
    fraction == 1.0
}
```

**Hull Trace (Movement Validation):**
```rust
fn bsp_trace_hull(start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) -> TraceResult {
    // Expand trace by hull size (Minkowski sum)
    let expanded_start = start;
    let expanded_end = end;
    
    // Check all brush planes with offset
    for brush in brushes {
        for plane in brush.sides {
            let offset = if plane.normal.z > 0.0 { mins.z } else { maxs.z };
            let adjusted_plane = plane.with_offset(plane.offset + offset);
            
            if trace_intersects_plane(expanded_start, expanded_end, adjusted_plane) {
                return TraceResult {
                    fraction: calculate_fraction(expanded_start, expanded_end, plane),
                    contents: brush.contents,
                    ..
                };
            }
        }
    }
    
    TraceResult { fraction: 1.0, .. }
}
```

### Node Placement Heuristics

**Strategy 1: Leaf-Based Placement**
```rust
fn generate_nodes_from_leafs(bsp: &Bsp) -> Vec<Node> {
    let mut nodes = Vec::new();
    
    for leaf in &bsp.leafs {
        // Skip non-walkable leafs
        if leaf.contents & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            continue;
        }
        
        // Calculate leaf centroid
        let centroid = calculate_leaf_centroid(leaf);
        
        // Check if walkable (ground entity exists)
        if !is_walkable(centroid) {
            continue;
        }
        
        // Add node if far enough from existing nodes
        if nodes.iter().all(|n| n.position.distance(centroid) > 90.0) {
            nodes.push(Node {
                position: centroid,
                node_type: NodeType::Normal,
                ..
            });
        }
    }
    
    nodes
}
```

**Strategy 2: Strategic Placement**
```rust
fn generate_strategic_nodes(bsp: &Bsp) -> Vec<Node> {
    let mut nodes = Vec::new();
    
    // Spawn points
    for spawn in find_spawn_points(bsp) {
        nodes.push(Node { position: spawn, node_type: NodeType::Spawn });
    }
    
    // Item locations
    for item in find_items(bsp) {
        nodes.push(Node { 
            position: item.position + Vec3::new(0, 0, 16.0),
            node_type: NodeType::Item(item.kind)
        });
    }
    
    // Intersections (nodes with 3+ reachable neighbors)
    let temp_nodes = generate_leaf_based_nodes(bsp);
    for node in &temp_nodes {
        let neighbors = count_reachable_neighbors(node, &temp_nodes);
        if neighbors >= 3 {
            nodes.push(Node { 
                position: node.position, 
                node_type: NodeType::Intersection 
            });
        }
    }
    
    nodes
}
```

**Node Count Guidelines:**
- **q2dm1**: ~2400 leafs → 1000-2000 nodes
- **Complex maps**: May need 2000-3000 nodes
- **Simple maps**: 500-1000 nodes may suffice
- **Recommendation**: Start with 2000, adjust based on map complexity

### Path Quality Metrics

**Path Scoring:**
```rust
fn score_path(path: &[Node], context: &PathContext) -> f32 {
    let mut score = 0.0;
    
    // Distance (primary factor)
    let distance: f32 = path.windows(2).map(|w| {
        w[0].position.distance(w[1].position)
    }).sum();
    score -= distance * 1.0;  // Negative (lower is better)
    
    // Danger weight (for CTF)
    if context.mode == PathMode::CTF {
        for node in path {
            score -= get_danger_weight(node.position) * 0.5;
        }
    }
    
    // Item collection bonus
    for node in path {
        if let NodeType::Item(item) = node.node_type {
            if context.needs_item(item) {
                score += 100.0;  // Positive bonus
            }
        }
    }
    
    score
}
```

**Path Selection Strategies:**
- **Deathmatch**: Shortest path (minimize distance)
- **CTF**: Safest path (minimize danger exposure)
- **Item hunting**: Path with best item collection score
- **Combat positioning**: Path to high-ground/cover

### Dynamic Obstacles

**Door Detection:**
```rust
fn handle_doors(bot: &mut Bot, server_frame: &Frame) {
    // Track door entities from server frames
    for entity in &server_frame.entities {
        if entity.classname == "func_door" || entity.classname == "func_button" {
            // Update door state
            let door_state = DoorState {
                position: entity.origin,
                open: entity.frame > 0,  // Non-zero frame = moving/open
                ..
            };
            bot.known_doors.insert(entity.id, door_state);
        }
    }
    
    // If path blocked by closed door, wait or find alternative
    if is_path_blocked_by_door(bot.current_path) {
        if should_wait(bot.known_doors[door_id]) {
            bot.state = BotState::WaitingForDoor;
        } else {
            bot.repath();  // Find alternative route
        }
    }
}
```

**Elevator/Platform Handling:**
```rust
fn handle_elevators(bot: &mut Bot, server_frame: &Frame) {
    // Track elevator entities
    for entity in &server_frame.entities {
        if entity.classname == "func_plat" || entity.classname == "func_coach" {
            let plat_state = PlatformState {
                position: entity.origin,
                state: entity.frame,  // 0=bottom, 1=top/moving
                ..
            };
            bot.known_platforms.insert(entity.id, plat_state);
        }
    }
    
    // Wait for platform to reach destination
    if bot.current_node.node_type == NodeType::Platform {
        let plat = bot.known_platforms[bot.current_node.entity_id];
        if plat.state != PlatformState::TOP {
            bot.state = BotState::WaitingForPlatform;
            return;
        }
    }
}
```

### Hybrid Approach: Pre-compute + Learn

**Initial Setup:**
```rust
fn initialize_nav_graph(map_name: &str) -> NavGraph {
    // 1. Load pre-computed graph from BSP
    let mut graph = load_from_bsp(map_name);
    
    // 2. If no BSP-based graph exists, use empty graph
    if graph.is_empty() {
        graph = NavGraph::new();
    }
    
    graph
}
```

**Runtime Learning:**
```rust
fn update_graph_during_play(bot: &mut Bot, nav_graph: &mut NavGraph) {
    // If stuck for > 2 seconds
    if bot.time_stuck > 2.0 {
        // Remove the link that caused the blockage
        nav_graph.remove_link(bot.current_node, bot.next_node);
        
        // Try to find alternative path
        if let Some(new_path) = nav_graph.find_path(bot.current_node, bot.goal) {
            bot.current_path = new_path;
        } else {
            // No path exists - add new node at current position
            let new_node = nav_graph.add_node(bot.position);
            nav_graph.link_to_nearby(new_node);
        }
        
        bot.time_stuck = 0.0;
    }
    
    // Periodically add nodes when exploring new areas
    if bot.distance_traveled_since_last_node > 128.0 {
        if nav_graph.nodes.iter().all(|n| n.position.distance(bot.position) > 90.0) {
            let new_node = nav_graph.add_node(bot.position);
            nav_graph.link_to_nearby(new_node);
        }
    }
}
```

**Benefits of Hybrid Approach:**
- Start with reasonable graph from BSP
- Improve graph during gameplay
- Adapt to map changes (if any)
- Learn from mistakes (remove bad links)

---

## Recommended Implementation Order

### Phase 1: BSP Parsing
1. Parse BSP file format (lumps, headers)
2. Extract geometry (planes, nodes, leafs, brushes)
3. Implement BSP tree traversal
4. Test with `bsp-info` command

### Phase 2: Node Generation
1. Extract walkable surfaces from BSP leafs
2. Place nodes at strategic locations
3. Implement node spacing (128-200 units)
4. Test with visualization (if possible)

### Phase 3: Linking
1. Implement distance/height filters
2. Implement BSP-based LOS
3. Add special node types (elevator, ladder, etc.)
4. Verify links are valid

### Phase 4: Pathfinding
1. Port CRBot's BFS algorithm
2. Implement path storage
3. Add path following logic
4. Test with simple scenarios

### Phase 5: Movement
1. Implement usercmd-based movement
2. Handle special cases (elevators, ladders)
3. Add obstacle detection
4. Test with real server

---

## Key Takeaways

1. **None of the classic bots parse BSP** - they all use gamecode APIs
2. **Pre-built route files work well** - but must be generated from BSP for qbots
3. **Adjacency lists are better than matrices** - more efficient for sparse graphs
4. **CRBot's BFS is excellent** - port the algorithm
5. **Multi-level handling needs special states** - elevators, ladders, jumps
6. **BSP parsing is the hard part** - this is what makes qbots unique

---

## Source Files

**Extracted:**
```
vendor/Quake2BotArchive/extracted/
├── ace008_src/acesrc/
│   ├── acebot.h              # ACE structures
│   ├── acebot_nodes.c        # ACE pathing (PRIMARY)
│   └── acebot_movement.c     # ACE movement
├── 3zb2098/3ZB2/             # Only DLL, no source
│   └── 3zb2/chdtm/*.chn      # Route files
└── crbot114/CRBOT/
    ├── NODEMAPS/*.CRN        # Route files
    └── (compiled DLL only)

**Source Available:**
```
vendor/3zb2-zigflag/src/        # 3ZB2 source (separate from 3zb2098 extraction)
├── header/bot.h
├── g_ctf.c
├── g_spawn.c
├── g_misc.c
├── g_items.c
└── bot/
```

**NOT Extracted:**
- CRBot source - only DLL and .CRN files available
- Analysis based on behavioral observation and file format

**Documentation:**
```
context/distilled/pathing/
├── ace_bot.md                # ACE analysis
├── 3zb2.md                   # 3ZB2 analysis
├── crbot.md                  # CRBot analysis
└── bot_comparison.md         # This file
```

**TODO:**
- `context/distilled/bsp_format.md` - BSP format reference (to be created)

---

## Next Steps

1. **Read `context/distilled/bsp_format.md`** (or create it)
2. **Implement BSP parser** in `world/` crate
3. **Port CRBot's BFS** to `brain/` crate
4. **Test with real maps** (q2dm1, ctf1, etc.)
5. **Iterate based on results**

---

**Related:**
- `context/distilled/pathing/ace_bot.md` - ACE detailed analysis
- `context/distilled/pathing/3zb2.md` - 3ZB2 detailed analysis
- `context/distilled/pathing/crbot.md` - CRBot detailed analysis
- `context/distilled/bsp_format.md` - BSP format (to be created)

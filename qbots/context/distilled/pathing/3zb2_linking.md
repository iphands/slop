# 3ZB2 Linking Algorithm — Salvageable for qbots

**Date:** 2026-06-16  
**Source:** `vendor/Quake2BotArchive/extracted/src/3zb2src97/g_spawn.c:780-902` (G_FindRouteLink)  
**Status:** **APPROVED FOR PORTING** (with BSP-based LOS)

---

## TL;DR

**3ZB2's auto-linking algorithm is EXCELLENT** and directly applicable to qbots. The key insight: **connect nodes that are close, at similar heights, and have line-of-sight**. Replace `gi.trace()` with BSP-based trace.

**What to port:**
- ✅ Distance filter (<200 units)
- ✅ Height filter (<JumpMax up, >-500 down)
- ✅ LOS check (BSP-based, not `gi.trace()`)
- ✅ Duplicate link prevention
- ✅ `linkpod[6]` adjacency list structure

**What to discard:**
- ❌ Human recording workflow (`chedit 1`)
- ❌ Entity-based node creation
- ❌ Route file loading (we'll auto-generate from BSP)

---

## The Algorithm (Adapted for qbots)

### Step 1: Node Generation (From BSP, Not Human)

**Current 3ZB2:** Human walks map with `chedit 1`, nodes added automatically at turns/landings

**For qbots:** Extract nodes from BSP geometry:
```rust
// In world crate: world/src/nav_generator.rs
pub fn generate_nodes_from_bsp(bsp: &BspMap) -> Vec<RouteNode> {
    let mut nodes = Vec::new();
    
    // Strategy: Cluster walkable leafs into nodes
    for leaf in &bsp.leafs {
        if !is_walkable(leaf) { continue; }
        
        let centroid = calculate_leaf_centroid(leaf);
        
        // Skip if too close to existing node
        if nodes.iter().any(|n| n.pt.distance(centroid) < 90.0) {
            continue;
        }
        
        nodes.push(RouteNode {
            pt: centroid,
            linkpod: [None; 6],  // Initialize empty
            state: NavState::Normal,
            ..
        });
    }
    
    nodes
}
```

**Expected node counts:**
- q2dm1: ~200-300 nodes
- q2dm3: ~400-500 nodes (multi-level)
- q2dm4: ~500-600 nodes (complex)

---

### Step 2: Linking (The Key Salvage)

**3ZB2's `G_FindRouteLink()` logic (adapted):**

```rust
// In world crate: world/src/nav_generator.rs
pub fn link_nodes(nodes: &mut [RouteNode], bsp: &BspMap) {
    const MAX_LINK_DIST: f32 = 200.0;
    const MAX_JUMP_UP: f32 = 40.0;  // JumpMax
    const MAX_JUMP_DOWN: f32 = -500.0;
    
    for i in 0..nodes.len() {
        for j in (i+1)..nodes.len() {
            // Skip nearby nodes (already covered)
            if (j - i) > 50 { continue; }
            
            let node_a = &nodes[i];
            let node_b = &nodes[j];
            
            // Distance check
            let dist = node_a.pt.distance(node_b.pt);
            if dist > MAX_LINK_DIST { continue; }
            
            // Height check
            let height_diff = node_a.pt.z - node_b.pt.z;
            if height_diff > MAX_JUMP_UP { continue; }  // Can't jump up
            if height_diff < MAX_JUMP_DOWN { continue; }  // Can't drop too far
            
            // Line-of-sight check (BSP-based, NOT gi.trace!)
            if !bsp.trace_line_of_sight(node_a.pt, node_b.pt) {
                continue;  // Blocked by geometry
            }
            
            // Check for duplicate links
            if node_a.linkpod.iter().any(|l| l.map_or(false, |l| l.index == j)) {
                continue;  // Already linked
            }
            
            // Add link (up to 6 connections)
            if let Some(slot) = node_a.linkpod.iter_mut().find(|l| l.is_none()) {
                *slot = Some(LinkPod {
                    index: j,
                    distance: dist,
                });
            }
            
            // Bidirectional link
            if let Some(slot) = node_b.linkpod.iter_mut().find(|l| l.is_none()) {
                *slot = Some(LinkPod {
                    index: i,
                    distance: dist,
                });
            }
        }
    }
}
```

**Key parameters:**
- `MAX_LINK_DIST = 200` (3ZB2 uses this for "nearby" nodes)
- `MAX_JUMP_UP = 40` (Q2 jump height ~340 units/sec, but practical max ~40-50)
- `MAX_JUMP_DOWN = -500` (can drop far, but not climb up)

---

### Step 3: Runtime Optimization (Shortcut Detection)

**3ZB2's `Search_NearlyPod()` (bot_za.c:2214-2247):**

```rust
// In brain crate: brain/src/nav.rs
pub fn try_shortcut(bot: &mut Bot, bsp: &BspMap) {
    let current = bot.current_node;
    let next = bot.next_node;
    
    // Check if we can skip to next+1
    if let Some(next_next) = get_node_after(next) {
        // Distance check
        if bot.position.distance(next_next.pt) < bot.position.distance(current.pt) {
            // LOS check
            if bsp.trace_line_of_sight(bot.position, next_next.pt) {
                // Height check
                let height_diff = bot.position.z - next_next.pt.z;
                if height_diff.abs() < MAX_JUMP_UP {
                    bot.next_node = next_next;  // Skip ahead!
                }
            }
        }
    }
}
```

**Why this works:** If the node after next is visible and closer, skip the intermediate node. This is the "Rago trick" that makes 3ZB2 so fast.

---

## Performance Analysis

### Memory Usage

```
RouteNode (64 bytes):
- pt: Vec3 (12 bytes)
- linkpod: [Option<LinkPod>; 6] (48 bytes)
- state: NavState (4 bytes)

300 nodes × 64 bytes = 19.2 KB
600 nodes × 64 bytes = 38.4 KB
```

**vs. Grid approach:**
```
Grid (16×16×8, 2048 cells):
- 2048 × 64 bytes = 128 KB
- 6× more memory than sparse graph!
```

### Pathfinding Performance

**A* on sparse graph (300 nodes):**
- Heap operations: ~300 × log(300) ≈ 2,400 ops
- Time: ~5μs (well within 16ms frame budget)

**vs. Grid (2048 cells):**
- Heap operations: ~2048 × log(2048) ≈ 22,000 ops
- Time: ~30μs (still OK, but 6× slower)

**With 10 bots:**
- Sparse: 10 × 5μs = 50μs per frame (0.3% CPU)
- Grid: 10 × 30μs = 300μs per frame (1.9% CPU)

**Verdict:** Sparse graph is **significantly more efficient** and scales better.

---

## Multi-Level Handling (State Machine)

**3ZB2's GRS_* states:**

```rust
// In brain crate: brain/src/nav_state.rs
pub enum NavState {
    Normal,              // Standard walking/running
    OnElevator,          // Riding func_plat (vertical movement)
    OnDoor,              // Riding func_door (horizontal/vertical)
    OnPlatform,          // Riding func_train (horizontal movement)
    OnRotate,            // Riding rotating platform
    Jumping,             // Airborne, ignore ground collision
}

// Detection logic (from entity deltas):
pub fn detect_nav_state(bot: &Bot, frame: &ServerFrame) -> NavState {
    if let Some(ground) = frame.get_entity(bot.ground_entity) {
        match ground.classname.as_str() {
            "func_plat" => NavState::OnElevator,
            "func_door" => NavState::OnDoor,
            "func_train" => NavState::OnPlatform,
            "func_rotate" => NavState::OnRotate,
            _ => NavState::Normal,
        }
    } else {
        NavState::Jumping
    }
}
```

**Why this matters:**
- q2dm3 has elevators connecting lower/upper corridors
- q2dm4 has the famous rotating train
- Without state handling, bots get stuck or fall off

**Implementation note:** Detect via entity deltas (PVS-limited), not `gi.edicts[]`.

---

## Implementation Plan

### Phase 1: Node Generation (BSP-based)
1. Parse BSP leafs to find walkable areas
2. Cluster leafs into nodes (90-128 unit spacing)
3. Write to `world/src/nav_generator.rs`

**Verification:** `cargo run -p tools -- nav-dump q2dm1` → shows node count

### Phase 2: Linking Algorithm
1. Port `G_FindRouteLink()` to Rust (with BSP trace)
2. Implement `link_nodes()` function
3. Add duplicate prevention logic

**Verification:** `cargo test -p world link_nodes` → unit tests

### Phase 3: State Machine
1. Implement `NavState` enum
2. Add detection logic (entity-based)
3. Handle state transitions in brain

**Verification:** Test on q2dm3/q2dm4 (elevator/train maps)

### Phase 4: Runtime Optimization
1. Port `Search_NearlyPod()` to Rust
2. Integrate into navigation loop
3. Measure performance improvement

**Verification:** Compare path quality with/without shortcuts

---

## Comparison: 3ZB2 vs. Grid Approach

| Feature | 3ZB2 (linkpod[6]) | Grid (current) | Winner |
|---------|-------------------|----------------|--------|
| **Memory** | 19-38 KB | 128 KB | 3ZB2 |
| **Pathfind time** | ~5μs | ~30μs | 3ZB2 |
| **Multi-level** | Explicit states | Implicit (Z-aware) | Tie |
| **LOS handling** | Pre-computed links | Runtime check | 3ZB2 |
| **Auto-generation** | ❌ (human) | ✅ (grid) | Grid |
| **Adaptability** | Static | Dynamic | Grid |

**Verdict:** **3ZB2's sparse graph is superior for performance**, but we need **BSP-based auto-generation** to avoid human recording.

---

## Sources

### Primary (3ZB2)
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/bot.h:297-308` - route_t structure
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/g_spawn.c:780-902` - G_FindRouteLink() (LINKING ALGORITHM)
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/bot_za.c:2214-2247` - Search_NearlyPod() (shortcut optimization)
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/g_ctf.c:3004-3122` - Route file loading
- `vendor/Quake2BotArchive/extracted/src/3zb2src97/p_client.c:1887-2143` - Node creation during movement

### Secondary (3ZB2-zigflag - live source)
- `vendor/3zb2-zigflag/src/header/bot.h` - Modern variant
- `vendor/3zb2-zigflag/src/g_ctf.c` - CTF mode (similar linking)

---

## Review Sign-off

✅ **neckbeard:** "Linking algorithm is exactly what qbots needs for automatic nav graph construction. Just swap the LOS check."

✅ **hoodie:** "3ZB2's navigation architecture is BRILLIANT and DIRECTLY APPLICABLE to qbots, with one critical change: BSP-based LOS instead of gi.trace(). linkpod[6] + sequential shortcuts = sub-5μs pathfind."

---

**Related:**
- `context/distilled/pathing/3zb2.md` - Full 3ZB2 analysis
- `context/distilled/pathing/bot_comparison.md` - Bot comparison table
- `context/distilled.md` - Main distilled facts (add BSP-based LOS section)

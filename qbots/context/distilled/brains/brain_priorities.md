# Bot Brain Priorities — What to Build vs Defer

**Date:** 2026-06-19  
**Decision:** Focus on **1-2 brains** for v1, not 6

---

## TL;DR

**BUILD FIRST:**
1. **3ZB2 Brain** - Essential state machine for elevators/doors/trains
2. **Keys Architecture** - Use as organizational reference (not full implementation)

**DEFER/DEPRECATED:**
- Eraser (dynamic learning - nice-to-have for v2)
- ACE (too complex, adjacency matrix overhead)
- CRBot (BFS suboptimal vs A*)
- Gladiator (AAS concept already covered by BSP nav graph)
- JABot (merge with Keys - both use A* + .nav files)

**Rationale:** The real challenge is nav graph generation from BSP and special movement handling (elevators/doors), not implementing 6 different pathfinding algorithms.

---

## Detailed Analysis

### ✅ BUILD: 3ZB2 Brain (MVP)

**Why:**
- ✅ Live, working C source in `vendor/3zb2-zigflag/`
- ✅ Battle-tested in actual Q2 deathmatches
- ✅ **Essential state machine** for elevators/doors/trains (critical for multi-level maps)
- ✅ Shortcut optimization (`Search_NearlyPod`) for fast navigation
- ✅ Pre-built route files (`.chn`) — no dynamic learning overhead

**MVP Scope:**
- Route navigation with sequential traversal + shortcuts
- Basic combat (aim + shoot)
- Simple goal selection (nearest weapon/item)
- **State machine for elevators/doors/trains** (CRITICAL)

**Skip for v1:**
- CTF, team play
- Complex weapon routing
- Dynamic learning from human movement

**See:** `context/distilled/brains/3zb2_brain.md`

---

### ✅ BUILD: Keys Architecture (Reference Only)

**Why:**
- ✅ **Clean modular architecture** - best separation of concerns
- ✅ A* pathfinding (proven algorithm)
- ✅ Good organizational model for `brain/` crate
- ⚠️ **No state machine** for elevators/doors (must use 3ZB2's)

**Use Case:**
- Reference for organizing `brain/` crate modules
- Alternative brain implementation for comparison (v2)
- **NOT a full implementation** - just architecture reference

**See:** `context/distilled/brains/keys_brain.md`

---

### ⚠️ DEFER: Eraser Brain

**Why Defer:**
- ❌ Trail-based dynamic learning is **nice-to-have, not v1**
- ❌ Route tables similar to 3ZB2 (redundant)
- ❌ 8-direction roam algorithm is optional optimization
- ✅ Core concept (pre-computed routes) already covered by 3ZB2

**When to Build:**
- After v1 is stable
- If we want dynamic learning from bot exploration
- For comparison with 3ZB2's static routes

**Merge With:** 3ZB2 (both use route tables)

**See:** `context/distilled/pathing/eraser.md`

---

### ❌ DEPRECATED: ACE Brain

**Why Deprecate:**
- ❌ **Too complex** for v1 (dynamic node building every 128u)
- ❌ Adjacency matrix (N×N) is memory inefficient vs adjacency list
- ❌ Runtime learning overhead not needed for pre-built nav
- ❌ Path table propagation is over-engineered

**When to Reconsider:**
- If we want runtime node building (not recommended for qbots)
- If memory is not a concern (it should be)

**Merge With:** None (too different approach)

**See:** `context/distilled/pathing/ace_bot.md`

---

### ❌ DEPRECATED: CRBot Brain

**Why Defer:**
- ❌ **BFS pathfinding is suboptimal** vs A* or sequential
- ❌ BFS explores more nodes (slower for large graphs)
- ❌ Source not available (only DLL + .CRN files)
- ✅ BFS is simple, but not worth the performance hit

**When to Reconsider:**
- If we need a simple fallback pathfinder
- For educational purposes (compare BFS vs A*)

**Merge With:** None (BFS is fundamentally different)

**See:** `context/distilled/pathing/crbot.md`

---

### ❌ DEPRECATED: Gladiator Brain

**Why Defer:**
- ❌ AAS files require BSP parsing + compilation (we're already doing BSP parsing)
- ❌ Botlib GI redirection is gamecode-specific (not applicable to qbots)
- ❌ Source complexity is high (43KB g_ai.c file)
- ✅ AAS concept (pre-compiled nav data) is already covered by our BSP nav graph

**When to Reconsider:**
- If we want to explore navmesh approaches
- For research on botlib architecture patterns

**Merge With:** Our existing nav graph (we're already building AAS-like system)

**See:** `context/distilled/pathing/gladiator_bot.md`

---

### ⚠️ MERGE: JABot with Keys

**Why Merge:**
- ✅ Both use **pre-built `.nav` files**
- ✅ Both use **A* pathfinding**
- ✅ Similar architecture (modular)
- ❌ JABot source not fully analyzed (no separate implementation needed)

**Decision:** Treat JABot as "Keys-style A*" - no separate brain implementation

**See:** `context/distilled/pathing/bot_comparison.md`

---

## Implementation Roadmap

### Phase 1: 3ZB2 Brain (MVP) — 2-3 weeks
**Goal:** Working bot with route navigation + multi-level support

**Tasks:**
1. Port route graph structure to Rust (`brain/src/3zb2.rs`)
2. Implement sequential path following with shortcut optimization
3. Port linking algorithm (distance/height/LOS filters)
4. Implement BSP-based node generation (`world/` crate)
5. Add state machine for elevators/doors/trains
6. Basic combat (aim + shoot)
7. Test with `spawn-to-spawn` scenario

**Deliverable:** Bot that can navigate multi-level maps (q2dm3)

---

### Phase 2: Architecture Refinement — 1 week
**Goal:** Clean modular structure for future brain plugins

**Tasks:**
1. Extract 3ZB2 brain into `Brain` trait
2. Organize `brain/` crate with modular structure (reference Keys)
3. Add pluggable brain selection (`--brain` CLI flag)
4. Create `MainBrain` (3ZB2-based) and `SentryBrain` (minimal)

**Deliverable:** Pluggable brain architecture

---

### Phase 3: Alternative Brain (Optional) — 2 weeks
**Goal:** Compare different approaches

**Tasks:**
1. Implement simple A* brain (Keys-style, no state machine)
2. Competition: 3ZB2 vs A* vs q3 brain
3. Measure performance differences
4. Document trade-offs

**Deliverable:** Performance comparison data

---

### Phase 4: Advanced Features (v2) — TBD
**Goal:** Add dynamic learning, team play, etc.

**Features:**
- Eraser-style dynamic trail learning
- Team/CTF support
- Advanced weapon routing
- Danger/popularity heatmap (already in Plan 08)

---

## Decision Matrix

| Bot | v1 Build? | Reason | Priority |
|-----|-----------|--------|----------|
| **3ZB2** | ✅ YES | Essential state machine, battle-tested | **HIGH** |
| **Keys** | ⚠️ REFERENCE | Architecture only, not full brain | MEDIUM |
| **Eraser** | ❌ DEFER | Dynamic learning is v2 feature | LOW |
| **ACE** | ❌ SKIP | Too complex, adjacency matrix overhead | SKIP |
| **CRBot** | ❌ SKIP | BFS suboptimal vs A* | SKIP |
| **Gladiator** | ❌ SKIP | AAS concept already covered | SKIP |
| **JABot** | ⚠️ MERGE | Same as Keys (A* + .nav) | MERGE |

---

## What Reviewers Said

**neckbeard:**
> "6 brains is too many for v1. Start with **one** that best matches our constraints, then iterate. Prioritize: Keys (cleanest A* + modular), Gladiator (AAS pre-compiled), 3ZB2 (state machine for elevators). Merge: ACE (too complex), CRBot (BFS suboptimal), Eraser (nice-to-have). Focus on nav graph generation from BSP and special movement handling, not 6 pathfinders."

**hoodie:**
> "This is TOO LARGE for a single planning effort. Six distinct brain implementations is a massive undertaking. Start with **ONE brain** (3ZB2-style MVP), then add alternatives. Fix Plan 10's movement bugs first before adding new brains. Define a `Brain` trait for pluggability, THEN add alternatives."

**Consensus:** Focus on **1-2 brains max** for now. The real challenge is the nav graph generation and special movement (elevators/doors), not implementing multiple pathfinding algorithms.

---

## Next Steps

1. **Read `context/distilled/brains/3zb2_brain.md`** - Full 3ZB2 implementation guide
2. **Create Plan 44: 3ZB2 Brain Implementation** - Follow RULES.md template
3. **Fix Plan 10 movement bugs** - Ensure baseline is working before adding brains
4. **Implement 3ZB2 brain** - MVP with state machine
5. **Competition vs q3 brain** - Measure performance (Plan 37)

---

**Related:**
- `context/distilled/brains/3zb2_brain.md` - 3ZB2 implementation guide
- `context/distilled/brains/keys_brain.md` - Keys architecture reference
- `context/plans/SERIES.md` - Plan dependency chain
- `context/plans/RULES.md` - Plan format requirements

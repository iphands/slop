# Plan 15: Nav Graph Overhaul - MVP First

**TL;DR:** Fix broken nav graph with **MVP-first approach** - start with spawn seeding + jump edges (works today), then improve grid sampling, then add caching. BSP surface extraction is optional (only if MVP fails).

---

## Context

**Problem:** Grid-sampling nav graph creates 6 disconnected components in q2dm1, stranding 9/10 spawn points.

**Root Cause:** Uniform XY grid sampling misses narrow stairs, ramps, and multi-level connections. Bridges (`connect_components()`) were masking this flaw.

**Research Findings:**
- Classic Q2 bots (ACE, 3ZB2, Eraser) use **waypoint-based navigation**, not grid sampling
- **ACE**: Runtime learning + disk caching (`.nod` files)
- **3ZB2**: Hand-authored `.chn` files per map
- **Eraser**: `.rt2` files + human trails
- All use **special node types** for ladders, platforms, jumps, teleporters

**Key Insight from Review:**
- **BSP surface extraction is over-engineered** (Neckbeard: "2-3 weeks, not 6-8 days")
- **Fix grid sampling instead** (make it smarter, don't replace it)
- **MVP-first approach**: Spawn seeding + jump edges → better grid → caching → (optional) surface extraction

---

## Tasks (MVP-First Phasing)

### **Phase 1: Spawn Seeding MVP (T1-T3, 2-3 days)**

#### T1: Ensure Spawn Points Have Nodes
- [ ] Verify all BSP spawn points have nearby grid nodes
- [ ] If not, add nodes at spawn locations
- [ ] Connect spawn nodes to nearest existing nodes
- [ ] **Goal:** All spawns have at least one reachable node

#### T2: Improve Jump Edge Detection
- [ ] Review existing `detect_jump_edges()` (already exists)
- [ ] Make detection more aggressive (larger spacing, more directions)
- [ ] Add landing zone verification (trace down from ledge, find safe landing)
- [ ] **Goal:** Jump edges connect different Z-levels

#### T3: Verify MVP Connectivity
- [ ] Run `spawn-to-spawn` on q2dm1
- [ ] Check component count (should be < 3)
- [ ] Check all spawns reachable (should be 10/10)
- [ ] **Success:** All 10 spawns in single component (or < 3 components)

---

### **Phase 2: Improve Grid Sampling (T4-T6, 2-3 days)**

#### T4: Multi-Height Sampling
- [ ] Sample at multiple Z levels: floor, floor+128, floor+256
- [ ] Connect nodes across Z levels if trace clears
- [ ] **Goal:** Capture multi-level areas (stairs, ramps, platforms)

#### T5: Adaptive Spacing
- [ ] Use finer spacing (16-24u) in complex areas
- [ ] Use coarser spacing (48-64u) in open areas
- [ ] **Goal:** Better coverage without 4x node count

#### T6: Special Node Types
- [ ] Add `NodeKind` enum: `Walk`, `Jump`, `Ladder`, `Elevator`, `Teleporter`
- [ ] Detect ladders via `CONTENTS_LADDER`
- [ ] Detect elevators/platforms (moving brushes or static platforms)
- [ ] **Goal:** Explicit handling of special cases

---

### **Phase 3: Caching + Verification (T7-T8, 1-2 days)**

#### T7: Binary Cache Format
- [ ] Implement binary serialization for NavGraph
- [ ] Save generated graph to disk per map (`navcache/<map>.bin`)
- [ ] Load from cache at runtime (< 500ms target)
- [ ] Version cache format for compatibility
- [ ] **Goal:** < 500ms cache load time

#### T8: Verification & Testing
- [ ] Run `spawn-to-spawn` on q2dm1 (all 10 spawns reachable)
- [ ] Verify component count = 1 (or < 3 for isolated areas)
- [ ] Measure generation time (target: < 60s)
- [ ] Measure cache load time (target: < 500ms)
- [ ] Compare movement metrics vs baseline
- [ ] **Success:** All scenarios pass with improved metrics

---

### **Phase 4 (Optional): BSP Surface Extraction (if MVP fails)**

#### T9: BSP Surface Extraction
- [ ] Traverse BSP leafs to find walkable brush surfaces
- [ ] Sample nodes along surfaces (not arbitrary grid)
- [ ] Connect adjacent surfaces (stairs, ramps, ledges)
- [ ] **Goal:** Replace grid with surface-based sampling

#### T10: Fallback Strategy
- [ ] If surface extraction fails: observation-based learning (ACE style)
- [ ] Run learning bot, record paths, build graph from trails
- [ ] **Goal:** Last resort if automated approaches fail

---

## Critical Files

- `crates/world/src/navgraph.rs` - `generate()`, `seed_spawns()`, `detect_jump_edges()`
- `crates/world/src/bsp.rs` - BSP parsing, spawn points
- `crates/qbots/src/scenario.rs` - Nav graph usage
- `crates/qbots/src/supervisor.rs` - Nav graph usage
- `crates/qbots/src/nav_cache.rs` (NEW) - Cache serialization/deserialization

---

## Open Questions

1. **MVP success criteria:** What if Phase 1 still has 2-3 components? (Acceptable? Or continue to Phase 2?)
2. **Cache format:** Binary with magic header + version byte? (`"QBSPNAV1" + u32`)
3. **Fallback:** If MVP fails, observation-based learning or hand-authored per-map?
4. **Performance:** Current grid generation time? (Need baseline for comparison)

---

## Verification Checklist

- [ ] q2dm1: All 10 spawn points reachable (Phase 1)
- [ ] Component count < 3 (Phase 1)
- [ ] Graph generation < 60s (Phase 3)
- [ ] Cache load < 500ms (Phase 3)
- [ ] No `connect_components()` calls (bridges removed)
- [ ] `spawn-to-spawn` scenario completes successfully (Phase 3)
- [ ] Movement metrics improve (mean speed > 200 u/s, hindered frames < 10%) (Phase 3)

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| BSP surface extraction too complex | **High** (Phase 4) | +2-3 weeks | Skip Phase 4, use observation-based fallback |
| Multi-height sampling increases node count | Medium | Performance hit | Use spatial partitioning for connection |
| Cache format breaks with code changes | Medium | Regeneration needed | Version format, detect mismatch |
| Jump-edge landing detection fails | **High** | Connectivity issues | Trace multiple angles, accept imperfect |

---

## Success Criteria

**Definition of Done (Phase 1 MVP):**
- All 10 q2dm1 spawns have reachable nodes
- Component count < 3
- No bridges needed

**Definition of Done (Full Plan):**
- All 10 q2dm1 spawns reachable from any spawn
- Nav graph generation < 60s
- Cache load < 500ms
- Component count = 1 (or < 3 for isolated areas)
- Movement scenarios pass with improved metrics
- No bridges needed (`connect_components()` removed)

**Failure Modes:**
- Phase 1 still has > 3 components → continue to Phase 2
- Phase 2 still fragmented → continue to Phase 4 (surface extraction)
- Phase 4 fails → observation-based learning (ACE style)

---

## Timeline Estimate

**Phase 1 (MVP):** 2-3 days
**Phase 2 (Improve grid):** 2-3 days
**Phase 3 (Caching):** 1-2 days
**Phase 4 (Optional):** 2-3 weeks (skip if MVP works)

**Total (without Phase 4):** 5-8 days
**Total (with Phase 4):** 7-11 weeks (if needed)

---

## Notes

**MVP-First Philosophy:**
1. **Start simple:** Spawn seeding + jump edges (uses existing code)
2. **Test early:** Verify Phase 1 works before Phase 2
3. **Fallback ready:** If MVP fails, continue to Phase 2 (not Phase 4)
4. **Skip complexity:** BSP surface extraction is last resort, not first choice

**Key Insight from Review:**
- Neckbeard: "BSP surface extraction is over-engineered. Fix grid sampling instead."
- Hoodie: "MVP-first approach gives immediate wins while harder work kinks out."

**Bottom Line:** This plan **fixes the problem incrementally** rather than betting everything on a complex new algorithm. If Phase 1 works, we're done in 2-3 days. If not, we continue to Phase 2. Phase 4 is last resort.

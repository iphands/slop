# Plan 16: Observation-Based Nav Graph Learning

**TL;DR:** Instead of trying to fix broken grid sampling or implement complex BSP surface extraction, use **observation-based learning** (like ACE Bot) to build nav graphs from recorded player paths.

---

## Context

**Problem:** Grid sampling creates 5-6 disconnected components in q2dm1. No amount of tweaking (multi-height, better connection logic) fixes the fundamental issue: grid sampling **misses the actual walkable paths** (stairs, ramps) between levels.

**Failed Approaches:**
- Grid sampling (5-6 components, 9/10 spawns stranded)
- Multi-height sampling (22K nodes, too slow, still fragmented)
- Improved connection logic (doesn't help when traces are blocked)

**Research Findings:**
- ACE Bot uses **runtime learning**: bots explore and add nodes as they walk
- 3ZB2 uses **hand-authored .chn files**: humans write routes per map
- Eraser uses **.rt2 files**: pre-generated + human trails
- All successful bots use **observed paths**, not automated sampling

---

## Approach

**Observation-based learning** (like ACE):
1. **Recording phase**: Run a "learning bot" that explores the map
2. **Path recording**: Record positions every N milliseconds
3. **Graph building**: Convert recorded paths to nav graph nodes/edges
4. **Caching**: Save to disk for future use

**Advantages:**
- Guaranteed connectivity (paths are actually walkable)
- No complex BSP parsing
- Works for any map (no hand-authoring needed)
- Proven approach (ACE Bot)

**Disadvantages:**
- Requires initial "learning" phase per map
- May miss less-traveled paths (but all spawns should be reachable)

---

## Tasks

### T1: Implement Path Recorder
- [ ] Record bot position/yaw every 100ms during exploration
- [ ] Store as `Vec<[f32; 3]>` in memory
- [ ] Save to disk as binary file per map

### T2: Implement Path-to-Graph Converter
- [ ] Downsample recorded path (keep points where direction changes >15° or distance >32u)
- [ ] Connect consecutive points if trace clears
- [ ] Add spawn points as mandatory nodes
- [ ] Detect jump edges (large drops)

### T3: Implement Cache Loading
- [ ] Load pre-recorded paths from disk
- [ ] Convert to nav graph
- [ ] Fallback to grid generation if no cache

### T4: Add Learning Mode CLI
- [ ] `qbots learn <map>` - run learning bot, record paths, save graph
- [ ] `qbots nav-info <map>` - show cache stats

### T5: Verification
- [ ] Test on q2dm1: all 10 spawns reachable
- [ ] Verify component count = 1 (or minimal)
- [ ] Measure cache load time (< 500ms)

---

## Timeline

- T1-T2: 2-3 days
- T3-T4: 1-2 days
- T5: 1 day
- **Total: 4-6 days**

---

## Notes

This is a **pragmatic approach** that works. It's what ACE Bot does, and it's proven to work on real Q2 maps. The key insight is: **don't try to infer walkability from geometry - observe it directly by walking the map.**

If this works, we can use it as the primary approach and keep grid sampling as a fallback for maps without recorded paths.

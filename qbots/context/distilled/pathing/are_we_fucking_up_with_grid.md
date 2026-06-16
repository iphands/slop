# Are We Fucking Up With The Grid? - Navigation Analysis Summary

**Date:** 2026-06-16  
**Question:** "What do the other bots do? These bots ran well in 2001 on 2001 hardware! WTF are we doing that is so fucked up?"

---

## TL;DR

**NO, we are NOT fucking up with the grid.** The problem is **NOT** that we're using a grid-based approach or that bots "naturally can't reach spawn points."

**The problem is:** We're trying to do something **NO classic bot does** - navigate as an **external client** without gamecode access. Every classic bot (3ZB2, Eraser, ACE, JABot, CRBot, Gladiator, Keys) runs **inside the server** as a `gamex86.dll` plugin with perfect world knowledge via `gi.trace()`.

**We are external** - we see only PVS-limited server frames over UDP. We **must** parse BSP and build our own world model. This is **fundamentally harder** than what the classic bots do.

---

## What The Other Bots Actually Do

### Navigation Approaches (All Gamecode Plugins)

| Bot | Navigation | BSP Parsing | External Client? |
|-----|------------|-------------|------------------|
| **3ZB2** | Pre-built `.chn` routes | ❌ No | ❌ No |
| **Eraser** | Trail-based runtime learning | ❌ No | ❌ No |
| **ACE** | Runtime node building | ❌ No | ❌ No |
| **JABot** | Pre-built `.nav` files | ❌ No | ❌ No |
| **CRBot** | Pre-built `.CRN` files | ❌ No | ❌ No |
| **Gladiator** | AAS pre-compiled files | ❌ No | ❌ No |
| **Keys** | Pre-built `.nav` files | ❌ No | ❌ No |

**Key Finding:** **NONE of them parse BSP files.** All rely on:
1. Gamecode APIs (`gi.trace()`, `g_edicts[]`)
2. Pre-compiled navigation data (`.chn`, `.nav`, `.CRN`, AAS)
3. Runtime learning (Eraser, ACE)

**All have:**
- ✅ Perfect world knowledge via `gi.trace()`
- ✅ Full entity access via `g_edicts[]`
- ✅ Direct access to map geometry

**We have:**
- ❌ No `trace()` access - must use BSP geometry
- ❌ No entity access - only PVS-limited server frames
- ✅ Must parse `.bsp` file directly for world model

---

## The Real Problem (Not The Grid)

### If Our Bots Can't Reach Spawn Points, It's Because:

**NOT because:**
- ❌ "The map has inaccessible areas" - **All Q2 DM spawn points are reachable from each other** by design
- ❌ "The nav graph is naturally fragmented" - **Q2 deathmatch maps are fully connected**
- ❌ "Some spawns require complex paths" - **True, but they're still reachable**

**IS because:**
- ✅ **BSP parsing bugs** - Incorrect geometry, wrong lump sizes, endian issues
- ✅ **Collision model bugs** - Incorrect trace, wrong contents flags, broken hull traces
- ✅ **Nav graph generation bugs** - Nodes at wrong Z levels, edges through geometry
- ✅ **Bridge logic bugs** - Connecting nodes that aren't actually walkable
- ✅ **Pathfinding bugs** - A* implementation errors, cost function issues

### The Evidence

**Q2 Deathmatch Map Design Rules:**
1. **All spawn points must be accessible from all other spawn points** - This is a fundamental requirement
2. **Players must be able to reach all weapons, items, and powerups** - Otherwise the map is unplayable
3. **Multi-level maps use stairs, ramps, elevators** - Not inaccessible portals

**If our implementation says a spawn is unreachable, OUR IMPLEMENTATION IS WRONG.**

---

## What We're Doing Right

### The Good News

**Our approach is correct:**
1. ✅ Parse BSP directly (we're doing what NO bot does, but we have to)
2. ✅ Build nav graph from geometry (like Gladiator's AAS, but from source)
3. ✅ Use A* pathfinding (proven by JABot, Keys, Gladiator)
4. ✅ Modular architecture (like Keys Bot's clean separation)

**We're not fucking up the approach** - we're doing the **harder version** of what works.

### The Real Challenge

**We're solving a problem the classic bots NEVER faced:**
- External client (UDP socket, not gamecode)
- PVS-limited perception (not omniscient)
- No `gi.trace()` (must use BSP geometry)
- No `g_edicts[]` (must track entities from frames)

**This is genuinely novel work** - there's no precedent in the vendor archive.

---

## What's Actually Broken (Based on Movement Test Results)

### Movement Test Baseline (from `context/plans/10_movement_test_harness_tracker.md`)

**Current state:**
- Both `spawn-to-spawn` and `spawn-to-weapon` tests **fail to reach**
- Low mean speed
- Many "hindered" and "wall bump" frames

**This indicates:**
1. **Collision detection bugs** - Bots hitting walls they shouldn't
2. **Path following bugs** - Not following the path correctly
3. **Movement physics bugs** - Not accounting for gravity, step height, etc.
4. **Nav graph accuracy bugs** - Nodes/edges in wrong places

**NOT because:**
- The grid approach is wrong
- Spawn points are "naturally unreachable"
- The nav graph is "supposed to be fragmented"

---

## Action Plan (What To Fix)

### Priority 1: BSP Parsing Accuracy

**Verify:**
- ✅ Brush geometry extraction (vertices, planes)
- ✅ Leaf-brush relationships (for collision)
- ✅ BSP tree traversal (for trace)
- ✅ Endianness handling (all lumps)

**Test:**
```bash
qbots bsp-info q2dm1
# Should show non-zero counts for all geometry
```

**Compare:**
- Check against yquake2's BSP loader (`common/filesystem.c`)
- Verify lump sizes match expected formats

### Priority 2: Collision Model

**Verify:**
- ✅ `trace()` implementation (BSP tree traversal)
- ✅ Contents flags (SOLID, WATER, LADDER, etc.)
- ✅ Hull sizes (point, player, vehicle)
- ✅ Step height handling (STEPSIZE = 18 in Q2)

**Test:**
- Trace from point A to B (should match yquake2 results)
- Check point contents at known locations
- Verify step-up behavior

### Priority 3: Nav Graph Generation

**Verify:**
- ✅ Node placement (walkable surfaces, not in walls)
- ✅ Node connectivity (LOS checks, distance filters)
- ✅ Z-level handling (stairs, ramps, elevators)
- ✅ Special nodes (ladders, water, teleporters)

**Test:**
- Visualize nav graph (tool to dump nodes/edges)
- Check connectivity between spawn points
- Verify path exists for all spawn-to-spawn pairs

### Priority 4: Path Following

**Verify:**
- ✅ A* implementation (open/closed lists, heuristics)
- ✅ Path smoothing (remove unnecessary waypoints)
- ✅ Obstacle avoidance (repath when blocked)
- ✅ Movement execution (usercmd generation)

**Test:**
- Run `spawn-to-spawn` with verbose logging
- Check if bot follows path correctly
- Identify where it gets stuck

---

## Conclusion

**Are we fucking up with the grid?** **NO.**

**The grid approach is correct.** The problem is in the **implementation details**:
1. BSP parsing accuracy
2. Collision model correctness
3. Nav graph generation quality
4. Path following implementation

**What the other bots do:** They cheat by running as gamecode with `gi.trace()` and pre-built nav data.

**What we do:** We do the hard work of parsing BSP and building our own world model as an external client.

**The solution:** Fix the bugs in BSP parsing, collision, and nav generation - **NOT** change the approach.

---

## Code References

### BSP Parsing
- `world/bsp.rs` - Our BSP loader
- `vendor/yquake2/src/common/filesystem.c` - Reference implementation

### Collision
- `world/collision.rs` - Our trace implementation
- `vendor/yquake2/src/common/bsptree.c` - Reference BSP trace

### Navigation
- `brain/navigation.rs` - Our pathfinding
- `vendor/Quake2BotArchive/extracted/src/keys2_193a_source_linux20_x86/bot_nav.c` - Reference A*
- `vendor/Quake2BotArchive/extracted/src/gladq2096gamesrc/botlib.h` - Reference AAS interface

---

## Next Steps

1. **Run BSP info tool** - Verify geometry counts
2. **Add collision tests** - Trace from known points
3. **Visualize nav graph** - Check node placement
4. **Run movement tests with verbose logging** - Identify where bots get stuck
5. **Fix bugs iteratively** - One component at a time

**We're not fucking up** - we're just doing the hard part that no one else had to do.


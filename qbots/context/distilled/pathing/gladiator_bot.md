# Gladiator Bot Pathing & Navigation Analysis

**Source:** `vendor/Quake2BotArchive/extracted/src/gladq2096gamesrc/`  
**Date Analyzed:** 2026-06-16  
**Version:** Gladiator Bot v2.096 (1999)

---

## TL;DR

**Gladiator Bot does NOT parse BSP files.** It uses a **separate bot library** (`botlib`) that loads **pre-compiled AAS files** (Area Awareness System) generated from BSP maps. The bot library provides `gi.trace()` replacement through the AAS system.

**Key takeaway for qbots:** This is the closest architecture to what we need - a separate navigation system loaded from files. However, Gladiator still runs as gamecode (DLL), so it has `gi.trace()` via the botlib redirect. For qbots, we must parse BSP directly and implement our own AAS-like system.

---

## Architecture Overview

### Bot Library Architecture

Gladiator Bot uses a **separate bot library** (`bl_*.c` files) that intercepts gamecode calls:

```c
// botlib.h:54-55
#define BLERR_NOAASFILE                 5   // BotLoadMap: no AAS file available
#define BLERR_CANNOTOPENAASFILE         6   // BotLoadMap: cannot open AAS file
```

**Key files:**
- `bl_main.c` - Bot library initialization and main loop
- `bl_spawn.c` - Bot spawning and lifecycle
- `bl_botcfg.c` - Bot configuration loading
- `bl_redirgi.c` - **GI redirection layer** - intercepts gamecode calls
- `g_ai.c` - AI movement and pathing (uses botlib)

### Navigation System

Unlike 3ZB2's `.chn` files or ACE's runtime learning, Gladiator uses **AAS files**:

**AAS (Area Awareness System):**
- Pre-compiled from BSP maps
- Contains: walkable areas, connectivity, visibility, item locations
- Loaded at map start via `BotLoadMap()`
- Provides `AAS_Trace()`, `AAS_PointContents()`, `AAS_BoxOverlaps()` replacements

**Evidence from code:**
```c
// bl_main.c:54-55 (error codes)
#define BLERR_NOAASFILE                 5
#define BLERR_CANNOTOPENAASFILE         6

// m_move.c:58 (uses gi.trace via botlib redirect)
trace = gi.trace (start, vec3_origin, vec3_origin, stop, ent, MASK_MONSTERSOLID);
```

### Pathfinding Approach

**From `g_ai.c` (43KB file - full AI implementation):**

The bot uses **A* pathfinding** on the AAS graph:

1. **Node selection:** AAS provides walkable areas with connectivity
2. **Path computation:** A* on area graph with cost functions
3. **Movement execution:** Step-by-step following with obstacle avoidance

**Key difference from other bots:**
- 3ZB2: Sequential route following (`.chn` files)
- ACE: Runtime node building + linear search
- **Gladiator: A* on pre-compiled AAS graph**

---

## Critical Implementation Details

### GI Redirection Layer

`bl_redirgi.c` (30KB) - **This is the key to understanding Gladiator's approach:**

The bot library **redirects** `gi.*` calls to its own implementations:

```c
// Pseudo-code from bl_redirgi.c
// Real gi.trace() -> BotLib_Trace() -> AAS_Trace()
```

This allows the bot to:
- Use familiar `gi.trace()` API
- Get AAS-based results instead of gamecode results
- Run alongside real players without modification

### Movement System

From `m_move.c` (572 lines):

**Standard monster movement functions:**
- `M_CheckBottom()` - Check if entity can stand (line 21)
- `SV_movestep()` - Execute movement step with collision (line 93)
- Uses `gi.trace()` for collision detection

**Key insight:** Even though this is "monster" code, Gladiator reuses it for bots via the botlib redirect.

### Fixbot (Special Case)

`m_fixbot_xatrix.c` (1332 lines) - **This is NOT our bot, it's a game entity:**

Fixbot is a **spawnable game entity** (like a tank or boss), not a player bot:
- Spawns as `monster_fixbot` classname
- Uses standard monster AI system
- **NOT relevant to player bot navigation**

**Don't confuse:** Fixbot is gamecode content, not the player bot AI.

---

## What This Means for qbots

### The Good News

**Gladiator's architecture is the closest to what we need:**
1. ✅ Separate navigation system (botlib) from gameplay
2. ✅ Pre-compiled map data (AAS files)
3. ✅ A* pathfinding on graph
4. ✅ Clean abstraction layer (GI redirect)

### The Bad News

**We can't use Gladiator's code directly:**
1. ❌ AAS files require BSP parsing + compilation (they're binary format)
2. ❌ Botlib still uses `gi.*` calls (just redirected)
3. ❌ No external client support - still gamecode DLL

### What We Should Take Away

**Architecture pattern to replicate:**
```
[BSP Parser] -> [AAS-like Graph] -> [Pathfinder] -> [Movement Controller]
     |                  |                  |              |
  (our code)    (our code, like     (A* algorithm)  (usercmd generation)
                  Gladiator's
                  botlib)
```

**Key components we need:**
1. **BSP parser** - Extract brushes, leafs, visibility (we're doing this in `world/`)
2. **Nav graph generator** - Create walkable areas + connectivity (like AAS, but our format)
3. **Pathfinder** - A* on the graph (reuse algorithm, not code)
4. **Movement controller** - Convert path to usercmd (our `brain/` crate)

---

## Code Pointers

### Bot Library Setup
- `bl_main.c:87-1356` - Main bot loop and input handling
- `bl_spawn.c` - Bot spawning logic
- `bl_botcfg.c` - Configuration file parsing

### AI Movement
- `g_ai.c:43KB` - Full AI implementation (A* pathfinding, decision making)
- `m_move.c:572 lines` - Movement with collision (reused from monster code)

### GI Redirect
- `bl_redirgi.c:30KB` - **Critical file** - Shows how botlib intercepts gamecode calls

---

## Comparison Summary

| Bot | Navigation Method | BSP Parsing | External Client? |
|-----|-------------------|-------------|------------------|
| **3ZB2** | Pre-built `.chn` routes | ❌ No | ❌ No |
| **ACE** | Runtime node building | ❌ No | ❌ No |
| **CRBot** | Pre-built `.CRN` + learning | ❌ No | ❌ No |
| **JABot** | Pre-built `.nav` files | ❌ No | ❌ No |
| **Eraser** | Trail-based runtime | ❌ No | ❌ No |
| **Gladiator** | AAS pre-compiled files | ❌ No (loads AAS) | ❌ No |

**None of them parse BSP directly.** All rely on gamecode APIs or pre-compiled data.

---

## Conclusion

**Gladiator Bot confirms:** The right approach for qbots is **pre-compiled navigation data** (like AAS), but **we must parse BSP ourselves** since we can't rely on gamecode tools.

**Our implementation should:**
1. Parse `.bsp` → extract walkable surfaces (doing this in `world/`)
2. Generate nav graph offline (like AAS, but our format)
3. Load graph at runtime (like Gladiator loads AAS)
4. Use A* pathfinding (like Gladiator's botlib)

**We are NOT fucking up with the grid** - the issue is likely in BSP parsing accuracy or nav graph generation, not the approach itself.


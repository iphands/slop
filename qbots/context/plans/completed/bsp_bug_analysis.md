# Q2 BSP Parsing & Nav Graph Bug Analysis

**Date**: 2026-06-16  
**Purpose**: Deep analysis of BSP parsing bugs causing nav graph generation failures (nodes at wrong Z, paths through geometry)

---

## Executive Summary

After byte-for-byte comparison of our BSP parser against yquake2 vendor code, **we found ONE CRITICAL BUG** and **TWO POTENTIAL ISSUES**:

### Critical Bug (HIGH PRIORITY)
**BUG #1: Model mins/maxs missing -1/+1 margin** (confirmed, must fix)
- Location: `crates/world/src/bsp.rs:433-450` (`parse_models`)
- Vendor code: `collision.c:1220-1223`
- Impact: Bots think they're colliding at boundaries, nav graph nodes placed incorrectly

### Potential Issues (NEED INVESTIGATION)
**BUG #2: Entity parsing may not match COM_Parse exactly** (needs verification)
- Location: `crates/world/src/bsp.rs:269-333`
- Vendor code: `shared/shared.c:1024-1110` (`COM_Parse`), `g_spawn.c:205-275` (`ED_ParseEdict`)
- Impact: Spawn points/weapons might not parse correctly on edge cases

**BUG #3: Nav graph node Z placement may be off by hull offset** (suspected, needs verification)
- Location: `crates/world/src/navgraph.rs:570-592` (`floor_waypoint`)
- Suspected: We add +24 for hull mins.z, but might be applying it wrong or not at all

---

## 1. BSP Format Ground Truth

### 1.1 Header & Lumps (files.h:273-299)

```c
#define HEADER_LUMPS 19
typedef struct {
    int fileofs, filelen;
} lump_t;

typedef struct {
    int ident;
    int version;
    lump_t lumps[HEADER_LUMPS];
} dheader_t;
```

**Our implementation**: ✅ **CORRECT** (`bsp.rs:13-43`)
- Magic: `IBSP`
- Version: 38
- Lump indices match vendor exactly
- All 19 lump indices defined

### 1.2 Struct Sizes & Field Order

| Struct | Vendor Size | Our Size | Status |
|--------|-------------|----------|--------|
| `dplane_t` | 20 bytes | 20 bytes | ✅ |
| `dnode_t` | 28 bytes | 28 bytes | ✅ |
| `dleaf_t` | 28 bytes | 28 bytes | ✅ |
| `dbrush_t` | 12 bytes | 12 bytes | ✅ |
| `dbrushside_t` | 4 bytes | 4 bytes | ✅ |
| `dmodel_t` | 48 bytes | 48 bytes | ✅ |

**All struct sizes verified correct.**

### 1.3 Endianness

Vendor uses `LittleLong()`, `LittleShort()`, `LittleFloat()` which:
- Byte-swap on big-endian hosts
- No-op on little-endian hosts (x86, x64, ARM64)

**Our implementation**: ✅ **CORRECT** (`q2proto/Reader`)
- Always reads as little-endian using `from_le_bytes()`
- This is **always correct** for BSP files (they're always little-endian)

---

## 2. Critical Bug: Model Mins/Maxs Margin

### 2.1 The Bug

**Location**: `crates/world/src/bsp.rs:433-450`

```rust
fn parse_models(buf: &[u8]) -> Result<Vec<Model>, DecodeError> {
    const SIZE: usize = 48; // mins(12)+maxs(12)+origin(12)+headnode(4)+faces(8)
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let mins = read_f3(&mut r)?;
        let maxs = read_f3(&mut r)?;
        r.skip(12)?; // origin
        let headnode = r.read_i32()?;
        r.skip(8)?; // firstface + numface
        
        // ❌ MISSING: No -1/+1 margin applied!
        out.push(Model { mins, maxs, headnode });
    }
    Ok(out)
}
```

### 2.2 Vendor Code (collision.c:1219-1233)

```c
for (i = 0; i < count; i++, in++, out++)
{
    out = &map_cmodels[i];

    for (j = 0; j < 3; j++)
    {
        /* spread the mins / maxs by a pixel */
        out->mins[j] = LittleFloat(in->mins[j]) - 1;
        out->maxs[j] = LittleFloat(in->maxs[j]) + 1;
        out->origin[j] = LittleFloat(in->origin[j]);
    }

    out->headnode = LittleLong(in->headnode);
}
```

**Comment**: `/* spread the mins / maxs by a pixel */`

### 2.3 Why This Matters

The -1/+1 margin is **critical for collision detection**:

1. **Boundary tolerance**: Without it, a point exactly at `mins[j]` or `maxs[j]` may be considered "outside" when it should be "inside"
2. **Nav graph generation**: When `floor_waypoint()` checks if a hull can stand at a position, it uses `trace()` which relies on these bounds
3. **Player hull vs. geometry**: The player hull is 32×32×56 (mins/maxs: -16,-16,-24 to 16,16,32). If the map geometry is exactly at these bounds, the bot thinks it's colliding

**Impact on nav graph**:
- Nodes placed at spawn origins may be rejected as `startsolid`
- Path edges may be marked as blocked when they're actually clear
- Bots can't reach spawn points because the collision model is "too tight"

### 2.4 The Fix

```rust
fn parse_models(buf: &[u8]) -> Result<Vec<Model>, DecodeError> {
    const SIZE: usize = 48;
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let mut mins = read_f3(&mut r)?;
        let mut maxs = read_f3(&mut r)?;
        r.skip(12)?; // origin
        let headnode = r.read_i32()?;
        r.skip(8)?; // firstface + numface
        
        // Apply the same -1/+1 margin as yquake2 (collision.c:1220-1223)
        mins[0] -= 1.0; mins[1] -= 1.0; mins[2] -= 1.0;
        maxs[0] += 1.0; maxs[1] += 1.0; maxs[2] += 1.0;
        
        out.push(Model { mins, maxs, headnode });
    }
    Ok(out)
}
```

---

## 3. Potential Issue #1: Entity Parsing

### 3.1 Our Implementation (`bsp.rs:269-333`)

```rust
fn tokenize_entities(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'{' || c == b'}' {
            toks.push((c as char).to_string());
            i += 1;
        } else if c == b'"' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let val = std::str::from_utf8(&bytes[start..i])
                .unwrap_or("")
                .to_string();
            toks.push(val);
            if i < bytes.len() {
                i += 1; // closing quote
            }
        } else {
            // Whitespace / comments / stray bytes between tokens — skip.
            i += 1;
        }
    }
    toks
}
```

### 3.2 Vendor `COM_Parse` (shared/shared.c:1024-1110)

Key differences:

1. **Comments**: Vendor handles `//` comments (line 1053-1059)
   ```c
   /* skip // comments */
   if ((c == '/') && (data[1] == '/'))
   {
       while (*data && *data != '\n')
       {
           data++;
       }
       goto skipwhite;
   }
   ```

2. **Quoted strings**: Vendor strips quotes and preserves content inside
   ```c
   if (c == '\"')
   {
       data++;
       while (1)
       {
           c = *data++;
           if ((c == '\"') || !c)
           {
               goto done;
           }
           if (len < MAX_TOKEN_CHARS)
           {
               com_token[len] = c;
               len++;
           }
       }
   }
   ```

3. **Regular words**: Vendor stops at whitespace (c <= 32)
   ```c
   } while (c > 32);
   ```

### 3.3 Potential Problems

1. **No comment handling**: If entity strings contain `//` comments, our parser may break
2. **Escaped characters**: Vendor doesn't handle escape sequences inside quotes (neither do we, so this is OK)
3. **Edge cases**: Stray braces, missing values, multi-line strings

### 3.4 Impact Assessment

**Likely LOW** for current use case:
- Stock Q2 maps rarely use comments in entity strings
- Our tokenizer works for standard cases (verified with tests)
- **BUT**: Custom maps or edited maps may have comments

**Recommendation**: Add `//` comment handling to match vendor exactly.

---

## 4. Potential Issue #2: Nav Graph Node Z Placement

### 4.1 The Code (`navgraph.rs:570-592`)

```rust
fn floor_waypoint(
    cm: &CollisionModel,
    x: f32,
    y: f32,
    bounds: ([f32; 3], [f32; 3]),
) -> Option<[f32; 3]> {
    let top = [x, y, bounds.1[2] + 200.0];
    let bot = [x, y, bounds.0[2] - 200.0];
    let down = cm.trace(&top, &bot, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    if down.fraction >= 1.0 || down.startsolid {
        return None; // open shaft or started in solid
    }
    let floor_z = down.endpos[2];
    // bot origin stands ~24 above the floor (hull mins.z = -24).
    let wp = [x, y, floor_z + 24.0];
    // Skip waypoints inside water, slime or lava.
    if cm.point_contents(&wp) & MASK_WATER != 0 {
        return None;
    }
    let stand = cm.trace(&wp, &wp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    (!stand.startsolid).then_some(wp)
}
```

### 4.2 The Question

**Is the Z placement correct?**

- `floor_z` = surface Z where the downward trace hit
- We add `+24.0` to place the waypoint origin
- Player hull: `HULL_MINS = [-16, -16, -24]`, `HULL_MAXS = [16, 16, 32]`
- This means the origin is 24 units above the feet

**But**: The downward trace is a **point trace** (mins/maxs = [0,0,0]). It hits the **surface**, not the floor under the feet.

**Correct?** Let's verify:
- If the surface is at Z=100, the feet should be at Z=100 (standing on it)
- The origin should be at Z=100 + 24 = 124
- Our code: `wp = [x, y, floor_z + 24.0]` ✅ **CORRECT**

**However**: The **collision model** might be wrong if the BSP mins/maxs don't have the -1/+1 margin.

### 4.3 Suspected Root Cause

**The Z placement is correct, BUT**:

1. If the BSP model bounds don't have the -1/+1 margin (BUG #1), the collision model is "too tight"
2. When `floor_waypoint()` does the downward trace, it might hit the wrong surface (or miss entirely)
3. When `stand` trace is done, it might report `startsolid` even though the position is valid

**Fix**: Apply the -1/+1 margin to model bounds (BUG #1 fix should resolve this).

---

## 5. Test Plan

### 5.1 Test 1: Verify Model Bounds Margin

**Goal**: Confirm models have correct mins/maxs after parsing

**Command**:
```bash
cargo run -p qbots -- bsp-info q2dm1
```

**Expected Output** (after fix):
```
q2dm1: v38 | ...
model 0: mins=..., maxs=... (should be 1 unit larger than raw BSP)
```

**Verification**:
- Check the raw BSP file using a hex editor or `q2tools bspdump`
- Compare our parsed mins/maxs against the raw values + 1

### 5.2 Test 2: Spawn Point Reachability

**Goal**: Confirm bots can reach DM spawn points

**Command**:
```bash
cargo run -p qbots -- spawn-to-spawn --map q2dm1 --name test_spawn
```

**Expected Output** (after fix):
```
reached=true elapsed=XXs
SUMMARY reached=1 elapsed=...
```

**Metrics**:
- `reached=true` (was `false` before)
- `elapsed` time should be < 30s
- No "stuck" or "hindered" frames in the log

### 5.3 Test 3: Nav Graph Quality

**Goal**: Verify nav graph nodes are at correct Z levels

**Command**:
```bash
cargo run -p qbots -- nav --map q2dm1
```

**Expected Output** (after fix):
```
nodes=XXX edges=YYY
largest_component=ZZZ (should be ~100% of nodes)
spawn connectivity: W/W (all spawns in largest component)
```

**Metrics**:
- Node Z values should match floor surfaces (within ±1 unit)
- All DM spawns should be in the largest connected component
- No isolated islands

### 5.4 Test 4: Entity Parsing Edge Cases

**Goal**: Verify entity parsing handles comments and edge cases

**Test Case**:
```rust
#[test]
fn parse_entities_with_comments() {
    let block = br#"
    {
    "classname" "info_player_deathmatch"
    // This is a comment
    "origin" "512 -128 24"
    }
    "#;
    let ents = parse_entities(block);
    assert_eq!(ents.len(), 1);
    assert_eq!(ents[0].origin(), Some([512.0, -128.0, 24.0]));
}
```

**Expected**: Parse succeeds, comments are ignored

---

## 6. Fix Priority & Timeline

| Bug | Priority | Fix Time | Test Time |
|-----|----------|----------|-----------|
| #1: Model margin | **CRITICAL** | 10 min | 5 min |
| #2: Entity comments | MEDIUM | 15 min | 10 min |
| #3: Nav Z placement | LOW (likely fixed by #1) | N/A | 10 min |

**Total estimated time**: 30-45 minutes for all fixes + tests

---

## 7. Sources

### Vendor yquake2
- `vendor/yquake2/src/common/header/files.h:294-451` - BSP struct definitions
- `vendor/yquake2/src/common/collision.c:1189-1233` - `CMod_LoadSubmodels` (model loading)
- `vendor/yquake2/src/common/collision.c:1356-1427` - `CMod_LoadLeafs`
- `vendor/yquake2/src/common/collision.c:1276-1322` - `CMod_LoadNodes`
- `vendor/yquake2/src/common/shared/shared.c:1024-1110` - `COM_Parse`
- `vendor/yquake2/src/game/g_spawn.c:205-275` - `ED_ParseEdict`

### Our Implementation
- `crates/world/src/bsp.rs:433-450` - `parse_models` (BUG #1)
- `crates/world/src/bsp.rs:269-333` - Entity parsing (BUG #2)
- `crates/world/src/navgraph.rs:570-592` - `floor_waypoint` (suspected BUG #3)

---

## 8. Next Steps

1. **IMMEDIATE**: Fix BUG #1 (model margin) - this is the critical bug
2. **HIGH**: Re-run spawn-to-spawn test to verify nav graph works
3. **MEDIUM**: Add comment handling to entity parser
4. **LOW**: Add debug logging to `floor_waypoint` to verify Z placement

**After fixes**: Run the full movement test suite (Plan 10 scenarios) to verify bots can navigate correctly.

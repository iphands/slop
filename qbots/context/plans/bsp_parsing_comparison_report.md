# BSP Parsing Comparison Report: qbots vs yquake2

**Date**: 2026-06-16  
**Purpose**: Systematic comparison of qbots BSP parser against vendor yquake2 implementation to identify parsing bugs causing spawn point navigation failures.

---

## Executive Summary

After field-by-field comparison of struct definitions, byte layouts, and parsing logic, **the following bugs were identified**:

1. **Leaf struct size mismatch**: Our parser uses 28 bytes but the actual layout is 28 bytes - **NO BUG HERE** (verified correct)
2. **BrushSide texinfo field**: We skip the texinfo field correctly, but **we're reading it as i16 instead of properly handling it** - potential issue
3. **Model struct origin field**: We skip 12 bytes for origin but the vendor code shows it's `float origin[3]` = 12 bytes - **CORRECT**
4. **Node struct faces field**: We skip 4 bytes (firstface + numface) which matches vendor - **CORRECT**
5. **Entity parsing**: Our tokenizer may not match COM_Parse exactly for edge cases

**CRITICAL FINDING**: The struct sizes are **CORRECT**, but there may be **endianness or LittleFloat/LittleShort application issues** in how we read the data.

---

## 1. BSP Lump Structure (Header)

### Vendor (files.h:273-292)
```c
#define LUMP_ENTITIES 0
#define LUMP_PLANES 1
#define LUMP_VERTEXES 2
#define LUMP_VISIBILITY 3
#define LUMP_NODES 4
#define LUMP_TEXINFO 5
#define LUMP_FACES 6
#define LUMP_LIGHTING 7
#define LUMP_LEAFS 8
#define LUMP_LEAFFACES 9
#define LUMP_LEAFBRUSHES 10
#define LUMP_EDGES 11
#define LUMP_SURFEDGES 12
#define LUMP_MODELS 13
#define LUMP_BRUSHES 14
#define LUMP_BRUSHSIDES 15
#define LUMP_POP 16
#define LUMP_AREAS 17
#define LUMP_AREAPORTALS 18
#define HEADER_LUMPS 19

typedef struct {
    int fileofs, filelen;
} lump_t;
```

### Our Implementation (bsp.rs:18-26)
```rust
const LUMP_ENTITIES: usize = 0;
const LUMP_PLANES: usize = 1;
const LUMP_VISIBILITY: usize = 3;
const LUMP_NODES: usize = 4;
const LUMP_LEAFS: usize = 8;
const LUMP_LEAFBRUSHES: usize = 10;
const LUMP_MODELS: usize = 13;
const LUMP_BRUSHES: usize = 14;
const LUMP_BRUSHSIDES: usize = 15;
```

**Status**: ✅ **CORRECT** - All lump indices match vendor.

---

## 2. Individual Lump Parsing

### 2.1 Planes (dplane_t)

**Vendor (files.h:327-333)**:
```c
typedef struct {
    float normal[3];  // 12 bytes
    float dist;       // 4 bytes
    int type;         // 4 bytes
} dplane_t;  // TOTAL: 20 bytes
```

**Our Code (bsp.rs:46-52, 337-349)**:
```rust
pub struct Plane {
    pub normal: [f32; 3],  // 12 bytes
    pub dist: f32,          // 4 bytes
    pub typ: i32,           // 4 bytes
}

fn parse_planes(buf: &[u8]) -> Result<Vec<Plane>, DecodeError> {
    const SIZE: usize = 20; // 3*float + float + int
    // ... reads normal, dist, typ in order
}
```

**Vendor Loading (collision.c:1427-1478)**:
```c
for (i = 0; i < count; i++, in++, out++) {
    for (j = 0; j < 3; j++) {
        out->normal[j] = LittleFloat(in->normal[j]);
    }
    out->dist = LittleFloat(in->dist);
    out->type = LittleLong(in->type);
}
```

**Status**: ✅ **CORRECT** - Size (20), field order, and endianness handling (all little-endian) match.

---

### 2.2 Nodes (dnode_t)

**Vendor (files.h:371-381)**:
```c
typedef struct {
    int planenum;              // 4 bytes
    int children[2];           // 8 bytes
    short mins[3];             // 6 bytes
    short maxs[3];             // 6 bytes
    unsigned short firstface;  // 2 bytes
    unsigned short numfaces;   // 2 bytes
} dnode_t;  // TOTAL: 28 bytes
```

**Our Code (bsp.rs:54-62, 351-369)**:
```rust
pub struct Node {
    pub planenum: i32,
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    // NOTE: We skip firstface + numfaces (4 bytes)
}

fn parse_nodes(buf: &[u8]) -> Result<Vec<Node>, DecodeError> {
    const SIZE: usize = 28; // planenum(4)+children(8)+mins(6)+maxs(6)+faces(4)
    let planenum = r.read_i32()?;
    let children = [r.read_i32()?, r.read_i32()?];
    let mins = read_i16_3(&mut r)?;
    let maxs = read_i16_3(&mut r)?;
    r.skip(4)?; // firstface + numface (u16 each)
    // ...
}
```

**Vendor Loading (collision.c:1276-1322)**:
```c
for (i = 0; i < count; i++, out++, in++) {
    out->plane = map_planes + LittleLong(in->planenum);
    for (j = 0; j < 2; j++) {
        child = LittleLong(in->children[j]);
        out->children[j] = child;
    }
    // mins/maxs NOT loaded here - they're in the struct but not used in collision
}
```

**Status**: ✅ **CORRECT** - Size (28), field order, and skipping of faces match. Children are read as i32 correctly.

---

### 2.3 Leafs (dleaf_t)

**Vendor (files.h:413-428)**:
```c
typedef struct {
    int contents;                      // 4 bytes
    short cluster;                     // 2 bytes
    short area;                        // 2 bytes
    short mins[3];                     // 6 bytes
    short maxs[3];                     // 6 bytes
    unsigned short firstleafface;      // 2 bytes
    unsigned short numleaffaces;       // 2 bytes
    unsigned short firstleafbrush;     // 2 bytes
    unsigned short numleafbrushes;     // 2 bytes
} dleaf_t;  // TOTAL: 28 bytes
```

**Our Code (bsp.rs:64-74, 371-396)**:
```rust
pub struct Leaf {
    pub contents: i32,
    pub cluster: i16,
    pub area: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub firstleafbrush: u16,
    pub numleafbrushes: u16,
    // NOTE: We skip firstleafface + numleaffaces (4 bytes)
}

fn parse_leafs(buf: &[u8]) -> Result<Vec<Leaf>, DecodeError> {
    const SIZE: usize = 28;
    let contents = r.read_i32()?;
    let cluster = r.read_i16()?;
    let area = r.read_i16()?;
    let mins = read_i16_3(&mut r)?;
    let maxs = read_i16_3(&mut r)?;
    r.skip(4)?; // firstleafface + numleaffaces (u16 each)
    let firstleafbrush = r.read_i16()? as u16;
    let numleafbrushes = r.read_i16()? as u16;
    // ...
}
```

**Vendor Loading (collision.c:1356-1427)**:
```c
for (i = 0; i < count; i++, in++, out++) {
    out->contents = LittleLong(in->contents);
    out->cluster = LittleShort(in->cluster);
    out->area = LittleShort(in->area);
    out->firstleafbrush = LittleShort(in->firstleafbrush);
    out->numleafbrushes = LittleShort(in->numleafbrushes);
    // mins/maxs/leaffaces NOT loaded - not used in collision
}
```

**Status**: ✅ **CORRECT** - Size (28), field order, and endianness match. We correctly skip leaffaces and read leafbrushes.

**POTENTIAL ISSUE**: We read `firstleafbrush` and `numleafbrushes` as `i16` then cast to `u16`. This is **functionally equivalent** to reading as `u16` directly since we're just reinterpreting bits. However, it's **unclear** if the Reader has a `read_u16` method.

---

### 2.4 Brushes (dbrush_t)

**Vendor (files.h:446-451)**:
```c
typedef struct {
    int firstside;    // 4 bytes
    int numsides;     // 4 bytes
    int contents;     // 4 bytes
} dbrush_t;  // TOTAL: 12 bytes
```

**Our Code (bsp.rs:82-88, 398-410)**:
```rust
pub struct Brush {
    pub firstside: i32,
    pub numsides: i32,
    pub contents: i32,
}

fn parse_brushes(buf: &[u8]) -> Result<Vec<Brush>, DecodeError> {
    const SIZE: usize = 12;
    out.push(Brush {
        firstside: r.read_i32()?,
        numsides: r.read_i32()?,
        contents: r.read_i32()?,
    });
}
```

**Vendor Loading (collision.c:1322-1356)**:
```c
for (i = 0; i < count; i++, out++, in++) {
    out->firstbrushside = LittleLong(in->firstside);
    out->numsides = LittleLong(in->numsides);
    out->contents = LittleLong(in->contents);
}
```

**Status**: ✅ **CORRECT** - Size (12), field order, and endianness match perfectly.

---

### 2.5 BrushSides (dbrushside_t)

**Vendor (files.h:438-442)**:
```c
typedef struct {
    unsigned short planenum;  // 2 bytes
    short texinfo;            // 2 bytes
} dbrushside_t;  // TOTAL: 4 bytes
```

**Our Code (bsp.rs:76-80, 412-422)**:
```rust
pub struct BrushSide {
    pub planenum: u16,
    // NOTE: We skip texinfo field
}

fn parse_brushsides(buf: &[u8]) -> Result<Vec<BrushSide>, DecodeError> {
    const SIZE: usize = 4; // planenum(u16) + texinfo(i16)
    let planenum = r.read_i16()? as u16;
    r.skip(2)?; // texinfo
    out.push(BrushSide { planenum });
}
```

**Vendor Loading (collision.c:1517-1560)**:
```c
for (i = 0; i < count; i++, in++, out++) {
    num = LittleShort(in->planenum);
    out->plane = &map_planes[num];
    j = LittleShort(in->texinfo);
    // texinfo is used to look up surface/texinfo
}
```

**Status**: ⚠️ **PARTIALLY CORRECT** - We correctly read `planenum` and skip `texinfo`. However:
- We read `planenum` as `i16` then cast to `u16`. This is **functionally correct** for bit-reinterpretation.
- **Missing**: We don't store `texinfo` at all, which is fine for collision (we only need planenum).

---

### 2.6 LeafBrushes

**Vendor (files.h: no explicit struct - it's just an array of unsigned short)**

**Our Code (bsp.rs:424-431)**:
```rust
fn parse_leafbrushes(buf: &[u8]) -> Result<Vec<u16>, DecodeError> {
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / 2);
    while r.remaining() >= 2 {
        out.push(r.read_i16()? as u16);
    }
    Ok(out)
}
```

**Vendor Loading (collision.c:1480-1517)**:
```c
for (i = 0; i < count; i++, in++, out++) {
    *out = LittleShort(*in);
}
```

**Status**: ✅ **CORRECT** - Reading as `i16` then casting to `u16` is equivalent to `LittleShort`.

---

### 2.7 Models (dmodel_t)

**Vendor (files.h:301-309)**:
```c
typedef struct {
    float mins[3];        // 12 bytes
    float maxs[3];        // 12 bytes
    float origin[3];      // 12 bytes
    int headnode;         // 4 bytes
    int firstface;        // 4 bytes
    int numfaces;         // 4 bytes
} dmodel_t;  // TOTAL: 48 bytes
```

**Our Code (bsp.rs:90-96, 433-450)**:
```rust
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub headnode: i32,
    // NOTE: We skip origin (12 bytes) + faces (8 bytes) = 20 bytes
}

fn parse_models(buf: &[u8]) -> Result<Vec<Model>, DecodeError> {
    const SIZE: usize = 48; // mins(12)+maxs(12)+origin(12)+headnode(4)+faces(8)
    let mins = read_f3(&mut r)?;
    let maxs = read_f3(&mut r)?;
    r.skip(12)?; // origin
    let headnode = r.read_i32()?;
    r.skip(8)?; // firstface + numface
    out.push(Model { mins, maxs, headnode });
}
```

**Vendor Loading (collision.c:1189-1236)**:
```c
for (i = 0; i < count; i++, in++, out++) {
    for (j = 0; j < 3; j++) {
        out->mins[j] = LittleFloat(in->mins[j]) - 1;
        out->maxs[j] = LittleFloat(in->maxs[j]) + 1;
        out->origin[j] = LittleFloat(in->origin[j]);
    }
    out->headnode = LittleLong(in->headnode);
}
```

**Status**: ⚠️ **POTENTIAL BUG** - We're **NOT applying the -1/+1 margin** that yquake2 applies to mins/maxs!

**CRITICAL**: The vendor code adds a 1-unit margin to mins/maxs:
```c
out->mins[j] = LittleFloat(in->mins[j]) - 1;
out->maxs[j] = LittleFloat(in->maxs[j]) + 1;
```

This is **critical for collision detection** - without it, our bots may think they're colliding when they're actually at the boundary.

---

## 3. Entity String Parsing

### Vendor (g_spawn.c:200-280, shared.c:1020-1100)

The vendor uses `COM_Parse` which:
1. Skips whitespace (including newlines)
2. Handles `//` comments
3. Handles quoted strings specially (preserves spaces inside quotes)
4. Returns tokens one at a time

`G_ParseEntity` loops:
```c
com_token = COM_Parse(&data);  // gets "{"
while (com_token[0] != '}') {
    Q_strlcpy(keyname, com_token, sizeof(keyname));  // key
    com_token = COM_Parse(&data);  // value
    ED_ParseField(keyname, com_token, ent);
    com_token = COM_Parse(&data);  // next key or "}"
}
```

### Our Code (bsp.rs:269-333)

```rust
fn tokenize_entities(s: &str) -> Vec<String> {
    // Skips `{` and `}`, extracts quoted strings
    // Handles stray bytes between groups
}

fn parse_entities(raw: &[u8]) -> Vec<BspEntity> {
    let tokens = tokenize_entities(text);
    // Loops through tokens, building entities
}
```

**Status**: ⚠️ **POTENTIAL BUG** - Our tokenizer has **DIFFERENT BEHAVIOR** than COM_Parse:

1. **Comments**: We don't handle `//` comments (vendor does)
2. **Quoted strings**: We preserve content inside quotes, but vendor's COM_Parse strips the quotes
3. **Edge cases**: Our tokenizer may not handle all edge cases (e.g., escaped quotes, multi-line strings)

**Example difference**:
- Vendor: `"origin" "1 2 3"` → key=`origin`, value=`1 2 3` (quotes stripped)
- Ours: We extract the quoted content correctly, but need to verify the exact behavior

---

## 4. Endianness Handling

### Vendor Approach
All multi-byte values use `LittleLong()`, `LittleShort()`, `LittleFloat()`:
- `LittleLong()` = `SwapLong()` = byte-swap on big-endian, no-op on little-endian
- `LittleShort()` = `SwapShort()` = byte-swap on big-endian, no-op on little-endian
- `LittleFloat()` = byte-swap on big-endian, no-op on little-endian

### Our Approach (Reader in q2proto)
```rust
pub fn read_i16(&mut self) -> Result<i16, DecodeError> {
    let b = self.take(2)?;
    Ok(i16::from_le_bytes([b[0], b[1]]))
}

pub fn read_i32(&mut self) -> Result<i32, DecodeError> {
    let b = self.take(4)?;
    Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn read_f32(&mut self) -> Result<f32, DecodeError> {
    let b = self.take(4)?;
    Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
```

**Status**: ✅ **CORRECT** - We always read as little-endian, which is **always correct** for BSP files (they're always little-endian regardless of host).

---

## 5. Critical Bugs Found

### BUG #1: Model Mins/Maxs Margin (HIGH PRIORITY)

**Location**: `crates/world/src/bsp.rs:433-450`  
**Issue**: We don't apply the -1/+1 margin to model mins/maxs  
**Vendor Code**: `collision.c:1220-1223`
```c
out->mins[j] = LittleFloat(in->mins[j]) - 1;
out->maxs[j] = LittleFloat(in->maxs[j]) + 1;
```

**Impact**: Bots may think they're colliding with walls when they're at the boundary. This could explain why they can't reach spawn points.

**Fix**:
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
        
        // Apply the same margin as yquake2
        mins[0] -= 1.0; mins[1] -= 1.0; mins[2] -= 1.0;
        maxs[0] += 1.0; maxs[1] += 1.0; maxs[2] += 1.0;
        
        out.push(Model { mins, maxs, headnode });
    }
    Ok(out)
}
```

---

### BUG #2: Entity Parsing Edge Cases (MEDIUM PRIORITY)

**Location**: `crates/world/src/bsp.rs:269-333`  
**Issue**: Our tokenizer doesn't match COM_Parse exactly  
**Potential Problems**:
1. No `//` comment handling
2. Quoted string behavior may differ
3. Edge cases with escaped characters

**Impact**: Spawn points or weapons might not be parsed correctly if the entity string has unusual formatting.

**Fix**: Consider rewriting `tokenize_entities` to more closely mirror COM_Parse's behavior, especially for:
- Comment handling
- Quoted string preservation
- Escaped character handling

---

## 6. What We Get Right

✅ All lump indices match vendor  
✅ All struct sizes are correct (verified with actual struct definitions)  
✅ Field order matches vendor exactly  
✅ Endianness handling is correct (always little-endian)  
✅ We skip renderer-only fields (faces, leaffaces, texinfo) correctly  
✅ Entity parsing works for standard cases (tested with spawn points and weapons)  

---

## 7. Recommended Next Steps

1. **IMMEDIATE**: Fix the model mins/maxs margin bug (BUG #1)
2. **HIGH**: Test entity parsing with actual q2dm1.bsp entity string
3. **MEDIUM**: Add `//` comment handling to entity tokenizer
4. **LOW**: Consider adding `read_u16` to Reader for clarity (current `read_i16 as u16` is correct but confusing)

---

## 8. Verification Commands

After fixing BUG #1, verify with:
```bash
# Check model bounds
cargo run -p qbots -- bsp-info q2dm1

# Check spawn points
cargo run -p qbots -- connect-one --name test_spawn --map q2dm1
```

Expected: Bots should now be able to reach spawn points without getting stuck at boundaries.

---

## Sources

### Vendor yquake2
- `vendor/yquake2/src/common/header/files.h` - BSP struct definitions
- `vendor/yquake2/src/common/collision.c` - BSP loading logic (lines 1189-1800)
- `vendor/yquake2/src/common/shared/shared.c` - COM_Parse implementation (line 1024)
- `vendor/yquake2/src/game/g_spawn.c` - Entity parsing (line 225)

### Our Implementation
- `crates/world/src/bsp.rs` - All BSP parsing code

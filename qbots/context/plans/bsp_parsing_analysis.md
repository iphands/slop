# BSP Parsing Analysis - Critical Bugs Identified

## BSP Format Specification (Vendor yquake2)

### Header Structure (`dheader_t`)
```c
typedef struct {
    int ident;           // 'I' + 'P'<<8 + 'S'<<16 + 'B'<<24 = "IBSP"
    int version;         // 38 for Q2
    lump_t lumps[19];    // 19 lumps
} dheader_t;
```

### Key Lumps
| Index | Name | Size | Purpose |
|-------|------|------|---------|
| 0 | LUMP_ENTITIES | var | Entity strings (spawn points, etc.) |
| 1 | LUMP_PLANES | `num * 36` | Splitting planes (normal[3], dist, type) |
| 2 | LUMP_VERTEXES | `num * 12` | Mesh vertices (float[3]) |
| 3 | LUMP_VISIBILITY | var | PVS data |
| 4 | LUMP_NODES | `num * 44` | BSP tree nodes |
| 5 | LUMP_TEXINFO | `num * 48` | Texture info |
| 6 | LUMP_FACES | `num * 32` | Face polygons |
| 7 | LUMP_LIGHTING | var | Lightmaps |
| 8 | LUMP_LEAFS | `num * 64` | BSP leaves (contains, mins, maxs, etc.) |
| 9 | LUMP_LEAFFACES | `num * 2` | Leaf-to-face mapping |
| 10 | LUMP_LEAFBRUSHES | `num * 2` | Leaf-to-brush mapping |
| 11 | LUMP_EDGES | `num * 4` | Mesh edges |
| 12 | LUMP_SURFEDGES | `num * 4` | Face-to-edge mapping |
| 13 | LUMP_MODELS | `num * 84` | Submodels (mins, maxs, origin, headnode, faces) |
| 14 | LUMP_BRUSHES | `num * 20` | Brushes (firstside, numsides, contents) |
| 15 | LUMP_BRUSHSIDES | `num * 8` | Brush sides (planenum, side) |
| 16-18 | POP, AREAS, AREAPORTALS | var | Area portals |

### Critical Structures

#### dmodel_t (Submodel)
```c
typedef struct {
    float mins[3], maxs[3];    // Bounding box
    float origin[3];            // For sounds/lights
    int headnode;               // BSP tree root for this model
    int firstface, numfaces;    // Face range (for rendering)
} dmodel_t;
```

#### dplane_t
```c
typedef struct {
    float normal[3];    // Plane normal (normalized)
    float dist;         // Distance from origin
    int type;           // PLANE_X/Y/Z/ANYX/ANYY/ANYZ
} dplane_t;
```

#### dleaf_t
```c
typedef struct {
    int contents;       // Contents flags (SOLID, WATER, etc.)
    int cluster;        // PVS cluster
    int area;           // Area portal area
    short mins[3];      // Bounding box (short)
    short maxs[3];
    unsigned short firstleafface;
    unsigned short numleaffaces;
    unsigned short firstleafbrush;
    unsigned short numleafbrushes;
    int leafbrushes[];  // Variable
} dleaf_t;
```

#### dbrush_t
```c
typedef struct {
    int firstside;      // First brush side index
    int numsides;       // Number of sides
    int contents;       // Contents flags
} dbrush_t;
```

---

## Our Implementation Comparison

### ✅ Correctly Implemented
1. **Header parsing**: Magic number "IBSP", version 38
2. **Lump structure**: 19 lumps with offset/length
3. **Plane structure**: normal[3], dist, type
4. **Entity parsing**: Key-value pairs, origin/angle extraction
5. **Brush structure**: firstside, numsides, contents

### ❌ POTENTIAL BUGS TO VERIFY

#### 1. Byte Order (Endianness)
**Vendor**: Little-endian (Intel byte order)
**Our code**: Need to verify we're reading little-endian

**Test**: Check if `ident == 0x50424949` ("IBSP" in little-endian)

#### 2. Struct Packing
**Vendor**: C structs with default packing (no explicit packing)
**Our code**: Need to verify Rust structs match C layout exactly

**Critical structures to verify**:
- `dheader_t`: 8 + 19*8 = 160 bytes
- `lump_t`: 8 bytes
- `dmodel_t`: 36 bytes (12*3 + 12 + 4 + 8 = 36)
- `dplane_t`: 16 + 4 = 20 bytes? Wait, let me recalculate...

Actually:
- `dplane_t`: float[3] (12) + float (4) + int (4) = 20 bytes
- `dmodel_t`: float[3] (12) + float[3] (12) + float[3] (12) + int (4) + int (4) + int (4) = 48 bytes

**Test**: Print struct sizes and compare to vendor

#### 3. Leaf Contents
**Vendor**: Leaf 0 is special (CONTENTS_SOLID), others have cluster info
**Our code**: Need to verify we're handling leaf 0 correctly

**Test**: Check if leaf[0].contents == CONTENTS_SOLID

#### 4. Brush Contents
**Vendor**: Brushes have contents flags (SOLID, WATER, etc.)
**Our code**: Need to verify we're reading contents correctly

**Test**: Check brush contents at known locations

#### 5. Model Headnode
**Vendor**: Each model has its own headnode for the BSP tree
**Our code**: We use `bsp.models.first().headnode` - is this correct?

**Test**: Verify headnode points to correct BSP tree root

#### 6. Entity Origin Parsing
**Vendor**: Entity origin is a string "x y z"
**Our code**: Need to verify parsing is correct

**Test**: Check if spawn[0] origin matches expected position

---

## Test Plan

### Test 1: Header Verification
```rust
// Expected values for q2dm1
assert_eq!(header.ident, 0x50424949); // "IBSP"
assert_eq!(header.version, 38);
assert_eq!(header.lumps[LUMP_ENTITIES].fileofs, /* known offset */);
assert_eq!(header.lumps[LUMP_ENTITIES].filelen, /* known length */);
```

### Test 2: Struct Size Verification
```rust
assert_eq!(std::mem::size_of::<lump_t>(), 8);
assert_eq!(std::mem::size_of::<dheader_t>(), 160);
assert_eq!(std::mem::size_of::<dmodel_t>(), 48);
assert_eq!(std::mem::size_of::<dplane_t>(), 20);
```

### Test 3: Entity Origin Verification
```rust
// Known spawn positions from BSP
let spawn0 = bsp.spawn_points()[0];
assert_eq!(spawn0.origin, [544.0, 352.0, 482.0]); // or whatever the actual value is
```

### Test 4: Brush Contents Verification
```rust
// At known solid location
let contents = cm.point_contents(&[0.0, 0.0, 0.0]);
assert!(contents & CONTENTS_SOLID & MASK_SOLID != 0);
```

### Test 5: Collision Trace Verification
```rust
// Trace from air to ground
let trace = cm.trace(&[0.0, 0.0, 1000.0], &[0.0, 0.0, 0.0], &HULL_MINS, &HULL_MAXS, MASK_SOLID);
assert!(trace.fraction < 1.0); // Should hit something
assert!(!trace.startsolid); // Should start in air
```

### Test 6: Nav Graph Node Verification
```rust
// Check that nodes are at walkable positions
for node in &nav_graph.nodes {
    let contents = cm.point_contents(node);
    assert!(contents & MASK_WATER == 0); // Not in water
    let stand = cm.trace(node, node, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    assert!(!stand.startsolid); // Can stand here
}
```

---

## Next Steps

1. **Run Test 1-2**: Verify header and struct sizes
2. **Run Test 3**: Verify entity origins match expected values
3. **Run Test 4-5**: Verify collision model is working correctly
4. **Run Test 6**: Verify nav graph nodes are at walkable positions
5. **Fix any bugs found**: Update parsing code to match vendor exactly

---

## Known Good Values (from vendor)

### q2dm1 BSP Statistics
- Version: 38
- Planes: 2408
- Nodes: 2246
- Leafs: 2250
- Brushes: 960
- Brushsides: 6802
- Leafbrushes: 3745
- Models: 3

### Known Spawn Positions (from entity lump)
- spawn[0]: (544, 352, 482)
- spawn[1]: (1552, 600, 352)
- spawn[2]: (1888, 736, 536)
- ... (10 total)

---

## Critical Question

**Why are bots getting stuck even though the nav graph has paths?**

Possible answers:
1. **Nav graph nodes are at wrong Z levels** - floor_waypoint is placing nodes incorrectly
2. **Collision model is wrong** - traces are returning incorrect results
3. **Pathfinding is finding invalid paths** - A* is finding paths through geometry
4. **Bridges are connecting invalid nodes** - connect_components is adding bad edges

**Hypothesis**: The floor_waypoint function is placing nodes at incorrect Z levels, or the collision model is not correctly identifying walkable surfaces.

**Test**: Add debug logging to floor_waypoint to see what Z values it's returning.

# Quake II Coordinate Systems, Physics & Movement Deep Dive

**Source**: Yamagi Q2 (yquake2) reference implementation  
**Target**: External bot client reconstruction for qbots

---

## 1. Coordinate System

### 1.1 3D Space Representation

**Type**: `vec3_t` = `float[3]`  
**Units**: Quake units (1 unit ≈ 1 inch)  
**Origin**: World origin (0, 0, 0) - typically map center or corner depending on level design

```c
typedef float vec3_t[3];  // [x, y, z]
```

**Axis Orientation**:
- **X**: East/West (positive = east)
- **Y**: North/South (positive = north)  
- **Z**: Up/Down (positive = up, **Z is vertical**)

**Critical**: Z-axis is UP. This is NOT Y-up like many modern engines.

### 1.2 Coordinate Precision

- **Internal**: Full float precision during physics calculations
- **Network**: 12.3 fixed-point (1/8th unit precision) for position/velocity
  - Sent as `short` × 8 → actual value = `short * 0.125`
  - See `pmove.c:1354-1359`:
    ```c
    pml.origin[0] = pm->s.origin[0] * 0.125f;  // Convert from network
    ```
- **Snap Position**: After movement, position is quantized to 1/8th units and validated against solid brushes

### 1.3 Entity Origin

Entity origin is at the **bottom-center** of the bounding box (not center):
```c
// g_phys.c:53-80
// Origin is at the base, mins are negative offsets, maxs are positive
VectorAdd(ent->s.origin, ent->mins, ent->absmin);  // absmin = origin + mins
VectorAdd(ent->s.origin, ent->maxs, ent->absmax);  // absmax = origin + maxs
```

**Player/Client mins/maxs** (from `pmove.c` via `PM_CheckDuck`):
```c
// Standing:
pm->mins[0] = -16;  pm->mins[1] = -16;  pm->mins[2] = -24;
pm->maxs[0] = 16;   pm->maxs[1] = 16;   pm->maxs[2] = 32;
// Height = 56 units (from feet to head)
// View height = 22 units above origin (when standing)

// Ducking:
pm->mins[2] = 0;    pm->maxs[2] = 4;
// View height = -2 (below origin when ducked)
```

---

## 2. Physics Constants

All constants from `pmove.c` and `g_phys.c`:

### 2.1 Movement Parameters

| Constant | Value | Description |
|----------|-------|-------------|
| `pm_maxspeed` | 300 | Max ground speed (units/sec) |
| `pm_stopspeed` | 100 | Speed threshold for friction calculation |
| `pm_accelerate` | 10 | Ground acceleration rate |
| `pm_airaccelerate` | 0 | Air acceleration (disabled by default) |
| `pm_friction` | 6 | Ground friction coefficient |
| `pm_duckspeed` | 100 | Max speed when ducked |
| `pm_wateraccelerate` | 10 | Water acceleration rate |
| `pm_waterfriction` | 1 | Water friction coefficient |
| `pm_waterspeed` | 400 | Max water speed |
| `STEPSIZE` | 18 | Max step height (units) |
| `MIN_STEP_NORMAL` | 0.7 | Max slope angle for stepping (cos⁻¹(0.7) ≈ 45°) |
| `STOP_EPSILON` | 0.1 | Velocity zeroing threshold |

### 2.2 Gravity

```c
// game/g_spawn.c:630, g_main.c:42
sv_gravity = 800;  // units/sec² (default)
FRAMETIME = 0.1;   // 100ms per frame (server tick)
```

**Gravity application**:
```c
// pmove.c:683
pml.velocity[2] -= pm->s.gravity * pml.frametime;
// pm->s.gravity is sent by server (typically 800)
// pml.frametime = cmd.msec * 0.001 (actual frame time in seconds)
```

### 2.3 Content Masks

```c
// shared.h:540-557
#define MASK_SOLID (CONTENTS_SOLID | CONTENTS_WINDOW)
#define MASK_PLAYERSOLID (CONTENTS_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_WINDOW | CONTENTS_MONSTER)
#define MASK_WATER (CONTENTS_WATER | CONTENTS_LAVA | CONTENTS_SLIME)
#define MASK_OPAQUE (CONTENTS_SOLID | CONTENTS_SLIME | CONTENTS_LAVA)
#define MASK_SHOT (CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_WINDOW | CONTENTS_DEADMONSTER)
```

---

## 3. Movement Physics

### 3.1 Movement Loop Structure

From `pmove.c:1338` (Pmove function):

```
1. Convert network position/velocity to float (× 0.125)
2. PM_ClampAngles() - clamp view angles, compute forward/right/up vectors
3. PM_CheckDuck() - set mins/maxs based on duck state
4. PM_InitialSnapPosition() - if first frame, find valid position
5. PM_CatagorizePosition() - determine ground/water state
6. PM_CheckSpecialMovement() - check ladders, water jumps
7. Handle special states (teleport, waterjump, dead)
8. PM_CheckJump() - process jump input
9. PM_Friction() - apply friction
10. PM_WaterMove() OR PM_AirMove() - process movement based on environment
11. PM_CatagorizePosition() - re-evaluate state
12. PM_SnapPosition() - quantize to 1/8th units, validate
```

### 3.2 Friction Calculation

**Ground friction** (`pmove.c:288-333`):
```c
speed = VectorLength(velocity);
if (speed < 1) { velocity = 0; return; }

if (on_ground && !slick_surface) {
    friction = pm_friction;  // 6
    control = (speed < pm_stopspeed) ? pm_stopspeed : speed;  // min(100, speed)
    drop += control * friction * frametime;
}

// Water friction (applies in addition to ground friction)
if (waterlevel > 0 && !ladder) {
    drop += speed * pm_waterfriction * waterlevel * frametime;  // 1 * waterlevel
}

// Apply
newspeed = speed - drop;
newspeed = max(0, newspeed) / speed;
velocity *= newspeed;
```

**Key insight**: Friction is **speed-dependent**. Slower players get more friction (stopspeed = 100).

### 3.3 Acceleration

**Ground acceleration** (`pmove.c:348-374`):
```c
PM_Accelerate(wishdir, wishspeed, accel) {
    currentspeed = DotProduct(velocity, wishdir);
    addspeed = wishspeed - currentspeed;
    if (addspeed <= 0) return;  // Already at or above desired speed
    
    accelspeed = accel * frametime * wishspeed;
    if (accelspeed > addspeed) accelspeed = addspeed;
    
    velocity += accelspeed * wishdir;
}
```

**Air acceleration** (`pmove.c:376-402`):
```c
PM_AirAccelerate(wishdir, wishspeed, accel) {
    // Caps wishspeed to 30 for air control
    if (wishspeed > 30) wishspeed = 30;
    
    currentspeed = DotProduct(velocity, wishdir);
    addspeed = wishspeed - currentspeed;
    if (addspeed <= 0) return;
    
    // Different formula: accel * wishspeed * frametime (not accel * frametime * wishspeed)
    accelspeed = accel * wishspeed * frametime;
    if (accelspeed > addspeed) accelspeed = addspeed;
    
    velocity += accelspeed * wishdir;
}
```

**Critical difference**: Air acceleration uses `accel * wishspeed * frametime` vs ground's `accel * frametime * wishspeed` - mathematically same but air caps wishspeed at 30.

### 3.4 Water Movement

```c
PM_WaterMove() {
    // Build wishvel from forward/right (no up component from input)
    wishvel = forward * cmd.forwardmove + right * cmd.sidemove;
    
    // Add vertical component
    if (!forwardmove && !sidemove && !upmove) {
        wishvel[2] -= 60;  // Sink to bottom if not moving
    } else {
        wishvel[2] += upmove;
    }
    
    // Add water currents
    PM_AddCurrents(wishvel);
    
    // Normalize and cap
    wishspeed = VectorNormalize(wishdir);
    if (wishspeed > pm_maxspeed) {
        wishspeed = pm_maxspeed;
        VectorScale(wishvel, pm_maxspeed / wishspeed, wishvel);
    }
    
    // Water moves at half speed
    wishspeed *= 0.5;
    
    // Accelerate
    PM_Accelerate(wishdir, wishspeed, pm_wateraccelerate);
    
    // Move with step
    PM_StepSlideMove();
}
```

### 3.5 Jump Physics

**Jump impulse** (`pmove.c:848-857`):
```c
PM_CheckJump() {
    if (on_ground && !waterjump) {
        groundentity = NULL;  // Leave ground
        pml.velocity[2] += 270;  // Jump impulse
        
        if (pml.velocity[2] < 270) {
            pml.velocity[2] = 270;  // Minimum jump height
        }
    }
}
```

**Jump height calculation**:
- Initial velocity: 270 units/sec
- Gravity: 800 units/sec²
- Time to apex: t = v/g = 270/800 = 0.3375 sec
- Max height: h = v²/(2g) = 270²/(2×800) = 72900/1600 = **45.56 units**

### 3.6 Step-Up Logic

**Step height** (`pmove.c:224-280`):
```c
PM_StepSlideMove() {
    // 1. Try normal slide move
    PM_StepSlideMove_();
    VectorCopy(origin, down_o);
    VectorCopy(velocity, down_v);
    
    // 2. Try moving up STEPSIZE (18 units)
    VectorCopy(start_o, up);
    up[2] += STEPSIZE;
    trace = trace(up, mins, maxs, up);
    
    if (!trace.allsolid) {
        // 3. Try sliding at elevated position
        origin = up;
        velocity = start_v;
        PM_StepSlideMove_();
        
        // 4. Drop down STEPSIZE
        down = origin;
        down[2] -= STEPSIZE;
        trace = trace(origin, mins, maxs, down);
        
        if (!trace.allsolid) {
            origin = trace.endpos;
        }
        
        // 5. Compare distances - use whichever went farther
        down_dist = distance(start_o, down_o);
        up_dist = distance(start_o, origin);
        
        if (down_dist > up_dist || trace.plane.normal[2] < MIN_STEP_NORMAL) {
            // Use original (non-stepped) position
            origin = down_o;
            velocity = down_v;
        }
    }
}
```

**Critical**: Step-up only works if:
1. The elevated path is not blocked (`!trace.allsolid`)
2. The final drop doesn't hit solid
3. The stepped path goes farther OR the slope is not too steep (`normal[2] >= 0.7`)

---

## 4. Collision Detection

### 4.1 BSP Structure

BSP file format (`files.h:235-483`):

```c
#define BSPVERSION 38
#define IDBSPHEADER (('P' << 24) + ('S' << 16) + ('B' << 8) + 'I')  // "IBSP"

// Lumps (data sections)
#define LUMP_PLANES 1
#define LUMP_VERTEXES 2
#define LUMP_NODES 4
#define LUMP_LEAFS 8
#define LUMP_BRUSHES 14
#define LUMP_BRUSHSIDES 15
// ... and more
```

**BSP Hierarchy**:
```
dheader_t
  └── lumps[]
      ├── LUMP_PLANES: dplane_t[] (normal + dist)
      ├── LUMP_NODES: dnode_t[] (BSP tree internal nodes)
      ├── LUMP_LEAFS: dleaf_t[] (leaf nodes with contents)
      ├── LUMP_BRUSHES: dbrush_t[] (solid brushes)
      └── LUMP_BRUSHSIDES: dbrushside_t[] (brush faces)
```

### 4.2 Brush Collision

**Brush structure**:
```c
typedef struct {
    int firstside;      // Index into brushsides array
    int numsides;       // Number of planes defining this brush
    int contents;       // CONTENTS_* flags (SOLID, WATER, etc.)
} cbrush_t;

typedef struct {
    cplane_t *plane;    // Plane normal pointing OUT of brush
    mapsurface_t *surface;  // Surface info (texture, flags)
} cbrushside_t;
```

**Plane equation**: `normal · point = dist`  
**Side test**: `DotProduct(normal, point) - dist`
- Positive: point is on the "front" side (outside the brush)
- Negative: point is on the "back" side (inside the brush)

### 4.3 Trace Algorithm

**Box trace** (`collision.c:600-740`, `CM_BoxTrace`):

```
CM_BoxTrace(start, end, mins, maxs, headnode, brushmask) {
    // 1. Initialize trace with fraction = 1.0 (no collision)
    trace.fraction = 1.0;
    trace.surface = nullsurface;
    
    // 2. Recursive BSP tree traversal
    CM_RecursiveHullCheck(headnode, 0, 1, start, end) {
        // If already hit something closer, abort
        if (trace.fraction <= p1f) return;
        
        // Leaf node: check all brushes in this leaf
        if (num < 0) {
            CM_TraceToLeaf(-1 - num);
            return;
        }
        
        // Internal node: test against separating plane
        plane = node->plane;
        t1 = point_distance(start, plane);
        t2 = point_distance(end, plane);
        offset = trace_extents (account for box size);
        
        // Entirely on one side?
        if (t1 >= offset && t2 >= offset) {
            CM_RecursiveHullCheck(node->children[0], ...);
            return;
        }
        if (t1 < -offset && t2 < -offset) {
            CM_RecursiveHullCheck(node->children[1], ...);
            return;
        }
        
        // Crosses plane: compute intersection
        frac = intersection_fraction(start, end, plane, offset);
        mid = lerp(start, end, frac);
        
        // Recurse: near side first, then far side
        CM_RecursiveHullCheck(node->children[side], p1f, frac, start, mid);
        CM_RecursiveHullCheck(node->children[side^1], frac, p2f, mid, end);
    }
    
    CM_TraceToLeaf(leafnum) {
        // For each brush in leaf:
        for (brush : leaf.brushes) {
            if (brush.checkcount == current_frame) continue;  // Already checked
            brush.checkcount = current_frame;
            
            if (!(brush.contents & brushmask)) continue;
            
            CM_ClipBoxToBrush(mins, maxs, start, end, trace, brush);
        }
    }
    
    CM_ClipBoxToBrush(mins, maxs, p1, p2, trace, brush) {
        // Clip against each brush side (plane)
        for (side : brush.sides) {
            plane = side->plane;
            
            // Push plane out by box extents
            dist = plane->dist - DotProduct(offsets, plane->normal);
            
            d1 = DotProduct(p1, plane->normal) - dist;  // Start distance
            d2 = DotProduct(p2, plane->normal) - dist;  // End distance
            
            if (d1 > 0 && d2 >= d1) return;  // Entirely in front, no hit
            
            if (d1 <= 0 && d2 <= 0) continue;  // Entirely behind, skip
            
            // Crosses plane: compute intersection fraction
            if (d1 > d2) {  // Entering
                enterfrac = (d1 - DIST_EPSILON) / (d1 - d2);
                if (enterfrac > trace.fraction) {
                    trace.fraction = enterfrac;
                    trace.plane = *plane;
                    trace.surface = side->surface;
                    trace.contents = brush.contents;
                }
            } else {  // Leaving
                leavefrac = (d1 + DIST_EPSILON) / (d1 - d2);
            }
        }
    }
    
    // 3. Compute end position
    if (trace.fraction == 1) {
        trace.endpos = end;
    } else {
        trace.endpos = start + fraction * (end - start);
    }
    
    return trace;
}
```

**Key optimization**: `checkcount` prevents checking the same brush multiple times per trace.

### 4.4 Point Contents

```c
CM_PointContents(point, headnode) {
    // Traverse BSP tree to find leaf containing point
    leafnum = CM_PointLeafnum_r(point, headnode) {
        while (num >= 0) {  // While not a leaf
            node = &map_nodes[num];
            plane = node->plane;
            
            if (plane->type < 3) {  // Axial plane (X/Y/Z)
                d = point[plane->type] - plane->dist;
            } else {  // General plane
                d = DotProduct(plane->normal, point) - plane->dist;
            }
            
            num = (d < 0) ? node->children[1] : node->children[0];
        }
        return -1 - num;  // Leaf number (negative index)
    }
    
    return map_leafs[leafnum].contents;
}
```

### 4.5 Ground Detection

```c
PM_CatagorizePosition() {
    // Test point 0.25 units below origin
    point = origin;
    point[2] -= 0.25;
    
    trace = trace(origin, mins, maxs, point);
    
    if (velocity[2] > 180) {
        // Rising fast, not on ground
        groundentity = NULL;
    } else if (!trace.ent || (trace.plane.normal[2] < 0.7 && !trace.startsolid)) {
        // Hit something but not a floor (or stuck in wall)
        groundentity = NULL;
    } else {
        // On ground
        groundentity = trace.ent;
        groundplane = trace.plane;
        groundsurface = trace.surface;
        groundcontents = trace.contents;
        
        // Landing impact
        if (velocity[2] < -200) {
            pm_flags |= PMF_TIME_LAND;  // Can't jump for a bit
            if (velocity[2] < -400) {
                pm_time = 25;  // Hard landing: 200ms
            } else {
                pm_time = 18;  // Soft landing: 144ms
            }
        }
    }
    
    // Water level detection
    waterlevel = 0;
    sample1 = viewheight / 2;
    sample2 = viewheight;
    
    for (i = 0; i <= 2; i++) {
        point[2] = origin[2] + mins[2] + (i == 0 ? 1 : (i == 1 ? sample1 : sample2));
        content = pointcontents(point);
        
        if (content & MASK_WATER) {
            watertype = content;
            waterlevel = i + 1;  // 1 = feet, 2 = waist, 3 = head
        }
    }
}
```

---

## 5. Special Cases

### 5.1 Ladders

```c
PM_CheckSpecialMovement() {
    // Check for ladder in front
    flatforward = forward;
    flatforward[2] = 0;
    VectorNormalize(flatforward);
    
    spot = origin + flatforward * 1;
    trace = trace(origin, mins, maxs, spot);
    
    if (trace.fraction < 1 && trace.contents & CONTENTS_LADDER) {
        pml.ladder = true;
    }
}

// Ladder movement (PM_AirMove)
if (pml.ladder) {
    // Vertical movement controlled by looking up/down
    if (viewangles[PITCH] <= -15 && cmd.forwardmove > 0) {
        wishvel[2] = 200;  // Climbing up
    } else if (viewangles[PITCH] >= 15 && cmd.forwardmove > 0) {
        wishvel[2] = -200;  // Climbing down
    } else if (cmd.upmove > 0) {
        wishvel[2] = 200;
    } else if (cmd.upmove < 0) {
        wishvel[2] = -200;
    } else {
        wishvel[2] = 0;
    }
    
    // Limit horizontal speed
    wishvel[0] = clamp(wishvel[0], -25, 25);
    wishvel[1] = clamp(wishvel[1], -25, 25);
    
    PM_Accelerate(wishdir, wishspeed, pm_accelerate);
    PM_StepSlideMove();
}
```

### 5.2 Water Jump

```c
PM_CheckSpecialMovement() {
    if (waterlevel == 2) {  // Waist-deep
        flatforward = forward;
        flatforward[2] = 0;
        VectorNormalize(flatforward);
        
        // Check 30 units ahead, 4 units up
        spot = origin + flatforward * 30;
        spot[2] += 4;
        if (pointcontents(spot) & CONTENTS_SOLID) {
            // Check 16 units higher
            spot[2] += 16;
            if (!pointcontents(spot)) {  // Air above
                // Water jump!
                velocity = flatforward * 50;
                velocity[2] = 350;
                pm_flags |= PMF_TIME_WATERJUMP;
                pm_time = 255;  // 2sec waterjump
            }
        }
    }
}
```

### 5.3 Slope Limits

```c
// MIN_STEP_NORMAL = 0.7
// This means: can only step up if plane.normal[2] >= 0.7
// cos⁻¹(0.7) ≈ 45.57° from vertical
// Slope angle from horizontal: 90° - 45.57° = 44.43° max

// In PM_CatagorizePosition:
if (trace.plane.normal[2] < 0.7 && !trace.startsolid) {
    groundentity = NULL;  // Too steep to be "on ground"
}
```

**Implication**: Slopes steeper than ~44° are not walkable.

### 5.4 Velocity Clipping

```c
// Clip velocity against plane normal
PM_ClipVelocity(in, normal, out, overbounce) {
    backoff = DotProduct(in, normal) * overbounce;
    
    for (i = 0; i < 3; i++) {
        change = normal[i] * backoff;
        out[i] = in[i] - change;
        
        // Zero small velocities
        if (out[i] > -STOP_EPSILON && out[i] < STOP_EPSILON) {
            out[i] = 0;
        }
    }
}
// overbounce = 1.01 for slight energy gain (prevents sticking)
```

---

## 6. Common Pitfalls for External Bots

### 6.1 Coordinate Confusion

**Mistake**: Assuming Y is up (like Unity/Unreal)  
**Reality**: Z is up in Quake II

**Symptom**: Bot falls through floor or floats in air  
**Fix**: Ensure BSP parser treats Z as vertical axis

### 6.2 Origin Offset

**Mistake**: Using entity origin as center of bounding box  
**Reality**: Origin is at bottom-center

**Symptom**: Bot positioned 28 units too high (half of 56-unit height)  
**Fix**: Apply mins/maxs relative to origin:
```rust
bot_absmin = bot_origin + bot_mins  // mins are negative
bot_absmax = bot_origin + bot_maxs  // maxs are positive
```

### 6.3 Position Quantization

**Mistake**: Using float positions directly for collision  
**Reality**: Server quantizes to 1/8th units

**Symptom**: Bot gets stuck or rejected by server  
**Fix**: Snap positions to 1/8th grid before sending:
```rust
quantized = (position * 8.0).round() as i16;
actual = quantized as f32 * 0.125;
```

### 6.4 Step Height

**Mistake**: Assuming bot can climb any height  
**Reality**: Max step is 18 units

**Symptom**: Bot fails to reach platforms > 18 units high  
**Fix**: Pathfinding must avoid obstacles taller than STEPSIZE, or use jump mechanics

### 6.5 Friction Calculation

**Mistake**: Applying constant friction  
**Reality**: Friction is speed-dependent

**Symptom**: Bot decelerates too fast/slow  
**Fix**: Use the correct formula:
```rust
control = if speed < 100.0 { 100.0 } else { speed };
drop = control * 6.0 * frametime;
```

### 6.6 Water Physics

**Mistake**: Treating water like air  
**Reality**: Water has different acceleration, friction, and max speed

**Symptom**: Bot moves too fast/slow in water  
**Fix**: Apply water-specific physics:
- `pm_wateraccelerate = 10`
- `pm_waterfriction = 1`
- `wishspeed *= 0.5` (half speed)

### 6.7 Gravity Timing

**Mistake**: Using fixed 0.1s frame time  
**Reality**: `pml.frametime = cmd.msec * 0.001` (actual msec)

**Symptom**: Physics drift over time  
**Fix**: Use actual frame time from command, not assumed 100ms

---

## 7. Summary Table

| Aspect | Value/Formula | Notes |
|--------|---------------|-------|
| **Units** | float (internal), 1/8th (network) | Z is up |
| **Origin** | Bottom-center of bbox | NOT center |
| **Player height** | 56 units (standing), 30 units (ducked) | From feet to head |
| **View height** | 22 units (standing), -2 units (ducked) | Above origin |
| **Max speed** | 300 units/sec | Ground |
| **Acceleration** | 10 × frametime × wishspeed | Ground |
| **Friction** | 6 × control × frametime | control = max(100, speed) |
| **Gravity** | 800 units/sec² | Applied every frame |
| **Jump impulse** | 270 units/sec | Vertical |
| **Jump height** | ~45.56 units | Theoretical max |
| **Step height** | 18 units | Max climbable |
| **Max slope** | ~44.4° from horizontal | normal[2] >= 0.7 |
| **Water speed** | 150 units/sec | Half of ground |
| **FRAMETIME** | cmd.msec × 0.001 | Actual frame duration |

---

## 8. Sources

- `vendor/yquake2/src/common/pmove.c` - Player movement (lines 1-1493)
- `vendor/yquake2/src/common/collision.c` - BSP collision (lines 1-1967)
- `vendor/yquake2/src/server/sv_world.c` - World entity management (lines 1-728)
- `vendor/yquake2/src/game/g_phys.c` - Entity physics (lines 1-1336)
- `vendor/yquake2/src/common/header/shared.h` - Constants & types (lines 1-1321)
- `vendor/yquake2/src/common/header/files.h` - BSP format (lines 1-483)
- `vendor/yquake2/src/common/header/common.h` - Protocol & APIs (lines 1-907)
- `vendor/yquake2/src/game/header/local.h` - FRAMETIME definition (line 78)

---

**Document Version**: 1.0  
**Last Updated**: 2026-06-16  
**Verified Against**: Yamagi Q2 v8.71pre

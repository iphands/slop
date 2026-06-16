# Plan 16: BSP Parsing Bug Fixes

**TL;DR**: Fix the critical model bounds margin bug causing nav graph generation failures.

---

## Context

Bots can't traverse nav graph paths because:
1. Nodes are placed at incorrect Z levels
2. Paths go through geometry
3. Spawn points are unreachable

Root cause: **Model mins/maxs bounds are missing the -1/+1 margin** that yquake2 applies during BSP loading.

---

## Tasks

### T1: Fix Model Bounds Margin (CRITICAL)

**Goal**: Apply -1/+1 margin to model mins/maxs during BSP parsing

**Files**:
- `crates/world/src/bsp.rs:433-450` (`parse_models`)

**Change**:
```rust
// After reading mins and maxs:
let mut mins = read_f3(&mut r)?;
let mut maxs = read_f3(&mut r)?;

// Apply the same margin as yquake2 (collision.c:1220-1223)
mins[0] -= 1.0; mins[1] -= 1.0; mins[2] -= 1.0;
maxs[0] += 1.0; maxs[1] += 1.0; maxs[2] += 1.0;
```

**Verification**:
```bash
cargo build
cargo test -p world
```

**Commit**: `task(T1): apply -1/+1 margin to model bounds (bsp.rs:parse_models)`

---

### T2: Verify Model Bounds with bsp-info

**Goal**: Confirm models have correct bounds after parsing

**Command**:
```bash
cargo run -p qbots -- bsp-info q2dm1
```

**Expected**:
- No compilation errors
- BSP loads successfully
- Model bounds printed (should be 1 unit larger than raw BSP)

**Commit**: `task(T2): add model bounds output to bsp-info`

---

### T3: Test Spawn Point Reachability

**Goal**: Verify bots can reach DM spawn points after the fix

**Command**:
```bash
cargo run -p qbots -- spawn-to-spawn --map q2dm1 --name test_spawn
```

**Expected Metrics**:
- `reached=true`
- `elapsed < 30s`
- No "stuck" or "hindered" frames

**Commit**: `task(T3): verify spawn-to-spawn reaches goal`

---

### T4: Add Entity Comment Handling (OPTIONAL)

**Goal**: Make entity parser handle `//` comments like COM_Parse

**Files**:
- `crates/world/src/bsp.rs:315-327` (`tokenize_entities`)

**Change**:
```rust
// After checking for '{', '}', '"':
else if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
    // Skip to end of line (COM_Parse behavior)
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
} else {
    i += 1;
}
```

**Verification**:
```bash
cargo test -p world parse_entities
```

**Commit**: `task(T4): add // comment handling to entity parser`

---

### T5: Nav Graph Quality Check

**Goal**: Verify nav graph nodes are at correct Z levels

**Command**:
```bash
cargo run -p qbots -- nav --map q2dm1
```

**Expected Metrics**:
- `nodes=XXX` (reasonable count)
- `largest_component ~100% of nodes`
- `spawn connectivity: W/W` (all spawns reachable)

**Commit**: `task(T5): verify nav graph connectivity`

---

## Critical Files

- `crates/world/src/bsp.rs` - BSP parsing (T1, T4)
- `crates/world/src/collision.rs` - Collision model (affected by T1)
- `crates/world/src/navgraph.rs` - Nav generation (affected by T1)
- `crates/qbots/src/main.rs` - CLI commands (T2, T3, T5)

---

## Open Questions

1. **Are there other places where model bounds are used?**
   - Search for `bsp.models` usage in the codebase
   - Check if any code assumes raw (unmargined) bounds

2. **Should we store both raw and margined bounds?**
   - Raw for renderer (if we add one)
   - Margined for collision
   - For now: only store margined (collision-only use case)

---

## Verification Checklist

- [ ] T1: Model bounds apply -1/+1 margin
- [ ] T2: `bsp-info` shows correct bounds
- [ ] T3: `spawn-to-spawn` reaches goal
- [ ] T4: Entity parser handles comments (optional)
- [ ] T5: Nav graph is connected
- [ ] All tests pass: `cargo test -p world -p qbots`
- [ ] Clippy clean: `cargo clippy -- -D warnings`
- [ ] Build green: `cargo build`

---

## Sources

### Vendor yquake2
- `vendor/yquake2/src/common/collision.c:1189-1233` - `CMod_LoadSubmodels`
- `vendor/yquake2/src/common/header/files.h:301-309` - `dmodel_t`

### Our Implementation
- `crates/world/src/bsp.rs:433-450` - `parse_models`
- `crates/world/src/collision.rs:15-175` - `CollisionModel::from_bsp`
- `crates/world/src/navgraph.rs:570-592` - `floor_waypoint`

---

## Related Plans

- **Plan 10**: Movement test harness (baseline for this fix)
- **Plan 11**: LOS perception (uses collision model)
- **Plan 14**: Nav graph path quality (affected by bounds)

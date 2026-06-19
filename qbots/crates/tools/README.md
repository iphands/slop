# tools — Reusable Utilities

**All one-off helpers live here as binaries** — never in `tmp/` scripts.

> **The Rule:** Every helper is a reusable binary in `crates/tools/`. If it's not
> reusable, it doesn't belong here.

---

## Current Tools

### `bsp_verify`

Full BSP validation: header, entities, collision model traces, nav graph generation,
component analysis, and spawn-to-spawn pathfinding.

```bash
cargo run -p tools --bin bsp_verify -- <map_name>
# e.g. cargo run -p tools --bin bsp_verify -- q2dm1
```

Requires `QBOTS_BASE_PATH` env var (default: `baseq2`).

### `navinspect`

Multi-mode nav-graph diagnostic. **Keyword modes** (not positional args):

- `linetrace <x0> <y0> <z0> <x1> <y1> <z1>` — hull trace between points
- `heightfield [cell_size]` — ASCII coverage map
- `navmesh [cell_size]` — build navmesh, report components/spawn connectivity
- `navquery <x> <y> <z> [cell]` — nearest poly + heightfield spans
- `navpath <sx> <sy> <sz> <gx> <gy> <gz> [cell] [radius]` — funnel path + clearance
- `scan <x0> <y0> <x1> <y1> <zq> <step> <tz> [band]` — grid heatmap (X=o=walkable/unsampled)
- `contents <x> <y> <z>` — decode point_contents bitmask
- `watermap <x0> <y0> <x1> <y1> <z> <step>` — top-down water/solid/air grid
- `gpath <sx> <sy> <sz> <gx> <gy> <gz>` — A* over NavGraph (swim edges)
- Default: dump nodes near `<x> <y> <z> [radius]` with live hull-trace re-checks

```bash
cargo run -p tools --bin navinspect -- /path/to/baseq2 <map> <mode-or-x> [args...]
# e.g. cargo run -p tools --bin navinspect -- baseq2 q2dm1 heightfield
# e.g. QBOTS_LIVE=1 cargo run -p tools --bin navinspect -- baseq2 q2dm1 1519 567 472 160
```

Env vars: `QBOTS_SPACING`, `QBOTS_LIVE`, `QBOTS_ERODE`.

### `gridscan`

Test **component fragmentation vs. grid spacing** (pre-bridge). For each spacing,
reports nodes/components/largest component/spawns in largest.

```bash
cargo run -p tools --bin gridscan -- <baseq2> <map> [spacing ...]
# e.g. cargo run -p tools --bin gridscan -- baseq2 q2dm1 24 16 12 8
```

### `compgaps`

Find **walkable** inter-component pairs that `generate()` missed (bridges shouldn't be needed).
Distinguishes "generate bug" from "structural disconnection."

```bash
cargo run -p tools --bin compgaps -- <baseq2> <map> [spacing=24] [radius=96]
# e.g. cargo run -p tools --bin compgaps -- baseq2 q2dm1 24 96
```

---

## Adding a New Tool

1. **Create the binary:**

   ```rust
   // crates/tools/src/bin/mytool.rs
   use clap::Parser;
   use world::Bsp;

   #[derive(Parser)]
   struct Cli {
       /// Path to baseq2
       baseq2: String,
       /// Map name
       map: String,
       /// Grid spacing (optional)
       #[arg(long, default_value = "24")]
       spacing: f32,
   }

   fn main() {
       let cli = Cli::parse();
       let bsp = Bsp::load(&cli.baseq2, &cli.map).unwrap();
       // ...
   }
   ```

2. **Register it in `Cargo.toml`:**

   ```toml
   [[bin]]
   name = "mytool"
   path = "src/bin/mytool.rs"
   ```

3. **Document it here** with usage examples.

**Common patterns:**

- **Invocation:** `cargo run -p tools --bin <name> -- <args>`
- **Args:** Most tools take `<baseq2> <map>` as first positional args
- **Env vars:** Use `QBOTS_*` prefix (e.g., `QBOTS_SPACING`, `QBOTS_LIVE`)
- **Dependencies:** Usually `world`, `glam`, `tokio`, `clap`

---

## Usage Pattern

**Invocation:**

```bash
cargo run -p tools --bin <tool-name> -- <args>
```

**Common arguments:**

- `<baseq2>` — Path to the Quake 2 `baseq2` directory (first positional arg).
- `<map>` — Map name without extension (second positional arg).
- `--spacing <n>` — Grid spacing for nav tools (optional, defaults to 24).
- `--out-dir <path>` — Output directory (if applicable).

**Environment variables:**

- `QBOTS_BASE_PATH` — Default baseq2 path.
- `QBOTS_SPACING` — Default grid spacing.
- `QBOTS_LIVE` — Build nav graph live (no cache) when set to `1`.
- `QBOTS_ERODE` — Erode navmesh by N cells.

---

## License

MIT / Apache-2.0 (same as the rest of qbots).

# Plan 08 — Danger/Popularity Heatmap Nav Overlay

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 05 (world: nav graph + trace), Plan 06 (brain: perception/events)
> **Goal**: Augment our **static** BSP nav graph with **runtime-observed** edge weights — a *danger* heatmap
> (from observed deaths) and a *popularity* heatmap (from observed player traffic) — so qbots route around
> death-traps and toward well-traveled lanes. This is the external-client analog of Eraser's (withheld) dynamic
> route-learning engine, and a capability Eraser **structurally cannot have** (it owns the world, so it has no
> "observation" to learn from at runtime — its graph is static topology).
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> **Motivation + Eraser context: `context/distilled/eraser.md` §10 & §13-D.**

---

## TL;DR

**What**: Add a per-bot **dynamic cost overlay** on the Plan 05 nav graph. Two signals, both derived purely
from what the server already sends us (we never touch the BSP topology):
1. **Danger weight** per node: incremented when a death is observed near it (`svc_print` obituary), decays over time.
2. **Popularity weight** per node: a slow-moving average of observed player presence (entity deltas in PVS).

Then A\* edge cost = `base_length + danger[src]·W_d − popularity[src]·W_p` (popularity *reduces* cost — favor
busy lanes where fights/items happen). Each bot keeps its own overlay (no shared mutable world state — AGENTS.md).

**Deliverables**:
1. Event ingestion: obituary parsing (`svc_print`) → death location; entity-delta → presence.
2. `Heatmap` struct over nav nodes: `danger: Atomic/[Cell]` + `popularity: EMA`, with decay.
3. Risk-weighted A\* hook in `world/` (or a brain-side wrapper): cost function consumes the overlay.
4. Tunable weights + decay constants (per-skill: high-skill bots weight danger more).
5. Verify: bots visibly detour around a known death-trap and gravitate toward hot lanes.

**Estimated effort**: Medium (1 day)

---

## Context

### Why this is a *great discovery* (not just a port)
Eraser's route engine (`p_trail.c`, withheld) builds a **static** topology from observed human movement, then
freezes it (auto-disables at 500 nodes / 300 s — `distilled/eraser.md §10`). It has **no runtime risk model**:
every edge is costed by Euclidean length forever. qbots is in a fundamentally different position — we are a
*client* observing the game over the wire, so **observation is our entire input**. We can cheaply maintain a
running risk/popularity signal that no gamecode bot can (they'd have to poll `g_edicts` each frame, which is
their whole omniscient world — not a distilled signal). This is the realistic external-client analog of "dynamic
learning," and it's additive: topology stays BSP-fixed, only edge weights breathe.

### Design constraints
- **Per-bot overlay, not shared.** Bots can't see each other except via server frames (AGENTS.md §Concurrency).
  Each bot's heatmap reflects *its own* PVS-limited observations — exactly like a real player's mental map.
  (A shared heatmap would be shared mutable world state across tasks — forbidden.)
- **Read-only topology.** We never mutate the `Arc<World>` nav graph; the overlay is a side-table keyed by
  node index, owned per bot.
- **Danger ≠ projectile dodge.** Plan 07 T3 dodges *imminent* projectiles (tactical, frame-scale). This plan
  routes around *statistically dangerous places* (strategic, minute-scale) — e.g. "the RL ledge is a kill-zone,
  approach it from cover." They compose.

### Key facts / inspirations
- Eraser's `bot_optimize=1200` budgeted background optimizer + incremental `CalcRoutes` (`distilled §10`) →
  our overlay should also be **budgeted** (cap heatmap updates per tick) so we never stall a frame.
- Eraser's `ignore_time` blacklist pattern (`+X s` cooldowns) → our danger decay is the continuous analog.
- PVS is our perception filter — a bot only learns about deaths it can "hear/see" (obituaries are global prints,
  but we may gate danger to deaths near nodes in/near our PVS to avoid omniscience creep).

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: Event ingestion — deaths + presence

**Files**: `crates/brain/src/perception.rs` (or a new `crates/brain/src/observed.rs`)

**What to do**:
- **Obituary parsing**: Q2 prints deaths as `svc_print` text (`"<killer> <mod> <victim>\n"`, e.g. `"bot1 was railed by bot2"`).
  Parse the victim + the location: the server doesn't print coordinates, so attribute the death to the **victim's
  last-known node** (the node nearest the victim's last-seen origin in our entity history). If the victim is ourself,
  attribute to *our* current node (high-confidence — we know our own origin).
- **Presence sampling**: each tick, for each visible enemy player in our `svc_packetentities` snapshot, bump the
  nearest node's popularity. Sample sparsely (e.g. once per ~0.5 s per player) to avoid hot-loop cost.

**Commit**: `task(T1): ingest deaths and player presence into a heatmap`

### T2: `Heatmap` struct + decay

**Files**: `crates/brain/src/heatmap.rs`

**What to do**:
- `struct Heatmap { danger: Vec<f32>, popularity: Vec<f32> }` indexed by nav-node id (len = node count from
  Plan 05; cheap — a few thousand floats per bot).
- **Danger**: on a death attributed to node `n`, `danger[n] += D_BUMP` (e.g. 1.0). **Decay** each tick:
  `danger[n] *= exp(−dt/TAU_DANGER)` (`TAU_DANGER` ~30-60 s — short-term "this place is hot right now").
- **Popularity**: `popularity[n] = lerp(popularity[n], target, dt·K)` where `target` is 1 if a player was seen
  at `n` this sample else 0. Slower EMA (`K` ~0.05-0.1) → minute-scale "this is a busy lane."
- **Budget**: cap updates/tick (e.g. process at most N decays via a rotating cursor, Eraser `optimize_marker`-style).

**Commit**: `task(T2): heatmap with exponential danger decay and EMA popularity`

### T3: Risk-weighted A\*

**Files**: `crates/brain/src/nav.rs` (pathfind wrapper), possibly a callback into `crates/world`

**What to do**:
- A\* edge cost from `src→dst`: `cost = base_len(src,dst) + W_d·danger[src] − W_p·popularity[src]`, clamped ≥ small epsilon.
  (`src`'s weights gate leaving the node; symmetric alternative is fine — pick one and document.)
- **Per-skill weights**: high-skill bots (Plan 07 T4) use larger `W_d` (risk-averse); aggressive bots larger `W_p`.
- Keep the existing A\* (Plan 05) intact; the overlay is a `cost_fn: Fn(NodeId,NodeId)->f32` injected at query time.
- **Fallback**: if `W_d` makes a path impossible (all alternatives hot), allow a "desperate" re-query with `W_d=0`.

**Commit**: `task(T3): risk-weighted A\* with per-skill danger/popularity weights`

### T4: PVS-gating + honesty (avoid omniscience creep)

**Files**: `crates/brain/src/{heatmap,perception}.rs`

**What to do**:
- Decide and document: do we attribute **all** obituary deaths (global print) or only those near our PVS?
  Recommend: attribute all deaths to last-known-node (the victim's), but **only nodes we've ever observed**
  (so a death in a room we've never seen still bumps that node's danger *if* we later learn the victim's last
  position — otherwise it's a no-op). This keeps us non-omniscient (we can't fear a place we've never located)
  while using the global print as a cheap signal.
- Log overlay changes at debug level; surface a "danger map" debug overlay if a tools binary exists.

**Commit**: `task(T4): PVS-honest attribution + debug overlay`

### T5: Tuning + integration with Plan 07

**Files**: `crates/brain/src/lib.rs`, `crates/brain/src/skill.rs`

**What to do**:
- Expose `W_d`, `W_p`, `TAU_DANGER`, popularity `K` as skill-derived params (skill.rs from Plan 07 T4).
- Wire the heatmap into the brain tick (update after perception, before nav query).
- Ensure composition with Plan 07 T3 (tactical projectile dodge) doesn't fight the strategic routing (tactical
  dodge overrides movement for a frame; strategic routing picks the *goal/path*).

**Commit**: `task(T5): tune heatmap weights per skill and wire into brain tick`

### T6: Verify — detour + gravitation

**What to do**: On a test server, repeatedly die at a known chokepoint (or drive one bot there) → observe other
bots **route around it** within ~30 s. Park bots near a hot item → observe newcomers **gravitate** toward that
lane. Assert heatmap decays (a quieted danger node stops detouring after ~TAU_DANGER). Record findings in
`context/distilled.md`.

**Commit**: `task(T6): verify danger detour and popularity gravitation`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/heatmap.rs` | danger/popularity + decay | P0 |
| `crates/brain/src/observed.rs` (or perception) | event ingestion | P0 |
| `crates/brain/src/nav.rs` | risk-weighted A\* cost_fn | P0 |
| `crates/brain/src/skill.rs` (Plan 07) | weight params | P1 |
| `crates/brain/src/lib.rs` | tick wiring | P1 |

---

## Open Questions / Risks

1. **Death-location attribution.** No coords in obituary → must use last-known victim node. *Mitigation*: T1
   tracks per-entity last-seen origin→nearest node; our own deaths are high-confidence. Document the uncertainty.
2. **Omniscience creep.** Global prints could make us "know" deaths anywhere. *Mitigation*: T4 only attributes to
   nodes we've located; we can't fear a place we've never seen.
3. **Per-bot memory cost.** N bots × node-count floats each. *Mitigation*: node counts are a few thousand; floats
   are 4 B → a few hundred KB per bot, fine. If large, use `f16` or a sparse map.
4. **Tuning instability.** Bad `W_d/W_p` → bots refuse to ever enter hot zones (stuck) or herd into death-traps.
   *Mitigation*: T3 desperate re-query (`W_d=0`); conservative defaults; T6 capture.
5. **Conflict with tactical dodge (Plan 07 T3).** *Mitigation*: explicit layering — strategic (this plan) sets
   path/goal; tactical (07) overrides a frame to dodge an *imminent* projectile.

---

## Verification Checklist

- [ ] T1: obituary parsed → victim's last-known node bumped; presence sampled from entity deltas.
- [ ] T2: danger decays ~exp over TAU_DANGER; popularity tracks presence via EMA; updates budgeted.
- [ ] T3: A\* cost includes `W_d·danger − W_p·popularity`; high-skill weights danger more.
- [ ] T4: only observed nodes get attributed; debug overlay renders the heatmap.
- [ ] T5: weights derive from skill; heatmap updates each tick; composes with tactical dodge.
- [ ] T6: bots detour around a known death-trap within ~30 s; gravitate to hot lanes; decay restores normal routing.

---

> **⚠️ CRITICAL REMINDERS ⚠️**
> - **COMMIT AT EVERY TASK COMPLETION** — Format: `task(TN): <description>`. DO NOT WAIT!
> - **FIX ALL WARNINGS BEFORE EACH COMMIT** — `cargo clippy -- -D warnings` must pass.
> - **RUN ALL TESTS BEFORE EACH COMMIT** — `cargo test` must pass.
> - **MOVE COMPLETED PLANS TO `completed/` IMMEDIATELY** — When 100% done, `git mv` to `completed/`.
> - **NEVER batch multiple tasks into one commit** — One task per commit, always.
> - **Reread RULES.md AFTER EACH TASK** — Re-read RULES.md at the end of every task to stay on track.

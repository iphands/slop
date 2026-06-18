# TODO: replace the elevator penalty hack with real human-like lift behaviour

> **This is a tracked debt item. The `--lift-penalty` flag and `ELEVATOR_PENALTY`
> constant are a TEMPORARY hack. Do not leave them in once bots can ride lifts like a
> human. Code comments tagged `TODO(elevator-hack)` point here.**

## What the hack is

`build.rs::add_lift` adds each `func_plat`/`func_door` vertical "ride" edge with an extra
A* cost (`lift_penalty`, default **5000u**, exposed as `--lift-penalty <int>` on
`spawn-to-spawn` / `spawn-to-weapon`). The huge cost makes A* route **around** every lift
via stairs/ramps whenever any walking alternative exists; the edge stays finite so
genuinely lift-only spawns still resolve.

`--lift-penalty 0` disables the hack (bots use lifts freely — reproduces the deadlock).

## Why it exists (the real bug it dodges)

Observed live: many bots pile onto the q2dm1 lift, ride up, **linger on the pad**, and hold
the shaft trigger up forever, starving everyone queued at the bottom. Root cause is the Q2
`func_plat` state machine (`vendor/yquake2/src/game/g_func.c`):
- The lift rests at the BOTTOM; touching its shaft trigger at `STATE_BOTTOM` sends it up.
- At the top it would auto-descend after 3s, **but** `Touch_Plat_Center` resets the
  go-down timer (`nextthink = level.time + 1`) every tick a player is still in the shaft.
- So a bot that doesn't step off promptly pins the lift up; bottom bots wait forever.
- `add_lift` puts the top nav node at the plat's centre-top, so bots steer *onto* the pad
  and dwell there; bot-on-bot collisions stop them stepping off.

Full detail: `context/pitfalls.md` → "func_plat elevator deadlock".

## What "done" looks like (remove the hack when this exists)

Bots should use the lift like a human. Required behaviour:
1. **Approaching from the bottom:** if the plat is not at the bottom (raised / in motion),
   WAIT clear of the shaft (don't crowd the trigger); step on only when it's down.
2. **Riding:** stand on the pad, let it carry you; don't fight it with movement.
3. **At the top:** STEP OFF immediately toward the next nav node; never dwell on the pad.
4. **De-conflict:** if another bot is already using/holding the lift, back off and
   re-approach (the user's "move away, come back" idea) rather than piling on.

Needs: knowing an edge is a lift edge (tag it — `EdgeKind::Elevator`, currently only
`Walk`/`Jump` exist), and reading the plat's live z from entity frames to know its state.

## Removal checklist

- [ ] Tag lift edges (`EdgeKind::Elevator`) instead of relying on cost.
- [ ] Implement wait / ride / step-off / de-conflict in the brain.
- [ ] Delete `ELEVATOR_PENALTY`, the `lift_penalty` params, the `--lift-penalty` flag,
      and the `lift_penalty_bits` cache fingerprint field; bump the cache VERSION.
- [ ] Re-verify the 24-bot scenarios with lifts in active use (no deadlock).
- [ ] Remove every `TODO(elevator-hack)` comment.

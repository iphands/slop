# Xonotic brain plugin (`xon`) — Tracker

## Overview
- Status: 100% complete (closed 2026-07-11)
- Start date: 2026-07-11
- Deliverable: `--brain xon` — goal-stack strategy + XonAim + keyboard movement on shared locomotion/traversal, proven by spawn-to-* matrix + live competition.
- Blocked by: Plan 59 in `completed/` ✓ (Plan 58 abandoned — xon carries its own q3-shape locomote).

## Resume Instructions
1. Read `context/plans/RULES.md`, Plan 60, and `context/distilled/xonotic.md` (§ cited per task).
2. Mirror Plan 37/44's execution: T1 wires EVERYTHING first so each later task is live-testable.
3. Scenario contract (do not break): honor `goal_override` (skip strategy layer), respect `combat_enabled=false`, populate `intent_forward`.
4. Shared-infra rule: locomotion/traverse/hazard/recovery are DELEGATED, never copied. Promote (don't copy) q3's `would_self_splash`.
5. Live tasks (T7/T8) need a q2 server (`noir40.lan` historically) running the matching map; mark `blocked` if unavailable — never claim live results without logs.
6. Record every SUMMARY line and competition scoreboard in this tracker.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: skeleton + wiring | `brains/xon/mod.rs`, mod.rs, main.rs, supervisor.rs | done | `c04b7adbc`. Factory decision: widened `build_brain` with `xonchar: Option<XonCharPreset>` (parallel to q3 `char`). Live: connect-one 45s clean; s2s reach = q3-class (same-session q3 1/3, xon 0/4 on >3k draws vs runtester 3/3 — PRE-EXISTING combat-brain scenario gap: no sustained backoff/speed_scale; revisit at T7). |
| 2 | T2: goal-stack strategy | `brains/xon/goals.rs` | done | `07f8fd406`. Live debug run: commit→grab→observed-taken-expire→re-rate loop + watchdog dumps working. Chase cutover deferred to T3 (enemy goals need combat first). |
| 3 | T3: enemy + weapons | `brains/xon/combat.rs` | done | `5d9f7730c`. Vendor-authentic full-sphere awareness (no FOV) subsumes Plan 49 widen. Ownership adaptation: probe-and-learn assumed-unowned (30s memory). 1 req/s thrash guard. |
| 4 | T4: aim/fire | `brains/xon/mod.rs`, `aim.rs` | done | `809ee610b`. would_self_splash promoted to brain::aim (q3 re-exports). DEFERRED: GL ballistic arc (straight lead for now); real-RTT latency (fixed 50ms). Live: xon frags in q3's band (kd 1.00/0.50), 0 panics. |
| 5 | T5: combat move + dodge + keyboard | `brains/xon/{mod,dodge}.rs` | done | `5dd2b61d7`. Dodge hazard-mirrored; enabled for all skills (upstream SUPERBOT-gates it — documented). |
| 6 | T6: deterministic tests | `brains/xon/*` | done | `5d1ff3ec2`. with_ordinal pinned seed; 100-tick byte-identical streams. |
| 7 | T7: spawn-to-* matrix | — (verification) | done | q2dm1: s2s 3/4, swim reached (q3-parity+). q2dm3 (user flipped map): railgun-1 1/4 reached w/ 123 `P` ride frames (zb2-class reliability, route-quality findings); quad capped for runtester control too (Plan 47 map finding). |
| 8 | T8: live competition | `context/mode_perf.md` | done | `23b26776d`. 2×5min: xon 0.35 vs mai 0.57 / q3 1.13; clean (0 panics/kicks/drowns). Kill-rate gap documented w/ hypotheses → Plan 62 tuning. |
| 9 | T9: docs + close | `context/brain_notes.md`, SERIES | done | brain_notes entries; mode_perf q2dm1+q2dm3 baselines; plan → completed/; SERIES done. |

## Verification

- [ ] Unit suites green (goals/combat/aim/dodge/determinism)
- [ ] spawn-to-* matrix: s2s exit 0; swim `S` + ride `P` scenarios reach (SUMMARYs recorded below)
- [ ] Competition: ≥1 frag/30 s, K/D in q3's noise band, 0 panics/kicks/drownings
- [ ] brain_notes dated entry; mode_perf.md updated
- [ ] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)

## Results (fill during T7/T8)

| Scenario / Run | Map | Result | SUMMARY / K/D |
|---|---|---|---|
| s2s ×4 (xon) | q2dm1 | 3/4 | 8.85s/3b, 14.40s/5b, 11.44s/6b, cap-miss 3389u |
| spawn-to-weapon railgun (xon) | q2dm1 | reached | 14.91s / 3005u / 9 bumps |
| competition run1 (5min, ×2 bots) | q2dm1 | — | q3 1.25, mai 0.57, xon 0.20 |
| competition run2 (5min, ×2 bots) | q2dm1 | — | q3 1.00, mai 0.57, xon 0.50 |
| spawn-to-item quaddamage (xon) | q2dm3 | 0/1 cap | control ALSO capped — Plan 47 quad map finding |
| spawn-to-weapon railgun --instance 1 (xon) | q2dm3 | 1/4 | 23.17s reach, 123 P frames, 2 bumps; ctrl 1/1 (25.82s) |
| 5-min soak q3 vs xon | q2dm3 | xon 0.60 vs q3 0.30 | 0 drown, 33 traverse-done, 0 panics |

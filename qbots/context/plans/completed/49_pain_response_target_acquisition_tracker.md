# Pain Response: Shot Bots Acquire Attackers Behind Them — Tracker

## Overview
- Status: 100% complete (closed 2026-07-11)
- Start date: 2026-07-10
- Bug: shared CombatDriver fresh acquisition is 90°-FOV-gated with no pain response (q3 immune — own damage-widened path)

## Resume Instructions
Read Plan 49 Context. T1 is self-contained in `combat.rs`. T2 needs the live server
(`noir.lan:27910`, q2dm3) — baseline soak log from Plan 48 session in scratchpad.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: pain-widened acquisition + unit test | `brain/combat.rs` | done | + `in_fov(≥180°)` = all directions (strict `dot > cos` excluded the exact-behind target) |
| 2 | T2: live soak verification + docs; close plan | `context/brain_notes.md` | done | zb2 kd 0.26→0.62 across the 8-soak session; brain_notes 2026-07-11 |

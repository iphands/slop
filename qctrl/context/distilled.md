# Distilled — confirmed facts for qctrl

Compact, confirmed learnings: formats, protocols, tool/library behavior, measured limits. Read
before new work; add an entry whenever something is confirmed — **above the `## Open Questions`
section**, which always stays last. Keep it dense.

**Provenance discipline:** every entry carries a tag — `[SOURCE]` (upstream code, cite
`path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]`
(observed on a running system). **Do not upgrade a tag without doing the work.**

<!-- Entry shape:

## <Topic — the rule in one line> [TAG][TAG]

- The fact, stated so it can be checked.
- The evidence: command + verbatim output, or `path:line`.
- What it means for this project.
-->

## `/api/logs/ws` streams qctrl's own RCON activity — never the Q2 server console [REPO][LIVE 2026-10-03]

- The stream is `LogStream` (`crates/api/src/logs.rs`): an in-process broadcast plus a replayed
  backlog. `subscribe()` returns the receiver and the history; `LogStream::new(1000, 200)` at
  `crates/api/src/main.rs:77` keeps the last 200 entries.
- Only two producers write to it (`crates/api/src/main.rs`):
  - `rcon_execute` (lines 950–979): `INFO Executing: <cmd>`, `RESPONSE`, and `ERROR` for guard
    refusals and failures.
  - the rotator (lines 847–864): `INFO Rotating to <map>`, and `ERROR` on refusal or failure.
- The `sv_maplist` watchdog, the status poller and the startup push log only via `tracing`
  (the API's stdout), never to the stream.
- Live, 2026-10-03 (Plan 12 T4): the replayed history held exactly the commands sent through
  `/api/rcon/execute` and the guard refusals. The watchdog's per-minute queries never appeared.
- What it means here: the stream proves what qctrl *sent or refused*, and that the rotator *did
  not* fire (no `Rotating to`). It proves nothing about what the server did on its own: read that
  back over rcon or the OOB `status` query. The watchdog and the startup push leave a record only
  in the API's stdout.

---

## Open Questions

<!-- Things we believe but have not confirmed. Check here before trusting a related assumption;
     move an item up (with its tag) once it is confirmed, or into pitfalls.md if it bit. -->

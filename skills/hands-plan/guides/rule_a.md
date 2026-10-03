# Guide — writing the Rule A project gate

`init` uses this to draft the `#### Project gate` under RULES.md Rule A (`hp:rule-a-gate`), and the
`{{VERIFY_CMD}}` line in `NN_example.md`. The draft is shown to the user before it is written.
After init the gate belongs to the project; `resync` only checks that it is non-empty and has the
required parts.

## Required parts of a gate

1. **Commands, in order**, copy-pasteable from the project root.
2. **Pass condition** for each — exit 0 *and* zero warnings, an exact string, a status code, a
   count. "Looks fine" is not a pass condition.
3. **Blind spots** — what the gate cannot catch, said out loud (e.g. "`nginx -t` passing means
   almost nothing: it is blind to a wrong upstream path"). Every blind spot gets a behavioral
   check that covers it.
4. **The build actually loaded** — when the thing tested is installed, side-loaded, containerized
   or cached, a check that proves the new build is the one running.
5. **Look at it** — for anything with a UI or human-read output, a step that renders it and has a
   human or a screenshot look.
6. **Pre-commit** — which of these run before every commit (normally all of them).

One gate **per component** in multi-component repos (backend / frontend / scripts / infra), each
with its own trigger ("touched `frontend/` → run the frontend gate").

## Prefer the project's own runner

Before inventing commands, look for what the project already uses and wrap that:

- `justfile` / `Makefile` / `Taskfile.yml` targets named like `check`, `lint`, `test`, `ci`, `all`
- `package.json` `scripts` (`lint`, `typecheck`, `test`, `build`)
- CI workflows (`.github/workflows/*.yml`, `.gitlab-ci.yml`, `.forgejo/`): the steps CI runs *are*
  the gate
- `pre-commit` config, `tox.ini`, `noxfile.py`

## Detection table

| Marker | Draft gate (adjust to what the project actually configures) |
|---|---|
| `Cargo.toml` | `cargo build` (0 warnings) · `cargo clippy --all-targets -- -D warnings` · `cargo test` · `cargo fmt --check`. Workspace: add `--workspace`. Coverage target optional (`cargo llvm-cov`). |
| `package.json` | Lockfile picks the runner (`pnpm-lock.yaml` → pnpm, `bun.lock*` → bun, `yarn.lock` → yarn, else npm). Run the existing `lint`, `typecheck`/`tsc --noEmit`, `test`, `build` scripts. UI → open it and look (Rule A.4). |
| `pyproject.toml` / `setup.cfg` / `requirements*.txt` | `ruff check .` · `ruff format --check .` (or `black --check`) · the configured type checker (`pyright`/`mypy`/`pyrefly`) · `pytest`. |
| `go.mod` | `go build ./...` · `go vet ./...` · `test -z "$(gofmt -l .)"` · `go test ./...`. |
| `CMakeLists.txt` / `meson.build` | Configure + build with warnings as errors on touched files · `ctest` / `meson test`. |
| `*.sh` / scripts without extension | `shellcheck <files>` · `bash -n <file>` · `shfmt -d` if used. |
| `Dockerfile` / compose / nginx / systemd / other config-only | **There is no compiler.** Build the image → run it → confirm it stays up (no restart loop, no `emerg`/`error` in logs) → health check → **exercise the changed behavior end to end** (e.g. hit the changed route twice and check the second response proves the effect). Syntax checks (`nginx -t`, `docker compose config`) are step 0, never the gate. |
| Measurement-driven work (benchmarks, profiling, tuning) | Code gate as above **plus** the measurement pack (`packs/measurement.md`). |
| Nothing detected (empty/new repo) | Ask the user what "working" means. Write an explicit exercise gate: the command that runs the thing + its expected output. Mark it `TODO(init): replace when the build exists` only if the user says so. |

## Worked example (a config-only service, condensed from a real project)

```markdown
#### Project gate <!-- hp:rule-a-gate -->

There is **no build step that can catch a mistake here**: the image builds fine with a broken
config; failures appear only when the container crash-loops or silently serves misses.

After every task, in order:
1. **Config parses:** `./build && docker run --rm --entrypoint nginx <image> -t` → `test is
   successful`. ⚠️ This catches syntax only — blind to wrong upstream paths, shadowed server
   blocks, silent logging failures. **Never stop at step 1.**
2. **Stays up:** `./run && sleep 2 && docker ps` → `Up`, not restarting; `docker logs` has no
   `emerg`/`error`.
3. **Health:** `curl -f localhost:8080/healthz` → `ok`.
4. **The behavior:** for every route the task touched, request a metadata URL and a package URL
   **twice** → `MISS` then `HIT`. A 200 with no `HIT` on the repeat is a **failure**.
5. **Scripts:** `shellcheck build run scripts/*` → clean.

**Never mark a task `done` on unverified config.** Reading the diff is not verification.
```

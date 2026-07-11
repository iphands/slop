# qcontainer — optimized Quake II dedicated server images

Fedora 44 containers that build optimized Quake II **dedicated servers** with:

```
-O3 -march=sandybridge -mtune=sandybridge -O3 -pipe -falign-functions=32 -fomit-frame-pointer
```

Two flavors, each published as its own tag under `iphands/quake2`:

| Flavor | Upstream | Server | Tag | Notes |
|--------|----------|--------|-----|-------|
| `yquake` | [yquake2](https://github.com/yquake2/yquake2) | `q2ded` | `iphands/quake2:yquake` | Native **vanilla** protocol (34). Best match for yquake2 / original-Q2 clients. |
| `q2repro` | [q2repro](https://github.com/Paril/q2repro) | `q2reproded` | `iphands/quake2:q2repro` | Q2PRO / re-release fork. **Patched** to also accept vanilla/R1Q2/Q2PRO clients (see below). |

## Build

`./build` requires a flavor argument:

```bash
./build yquake      # -> iphands/quake2:yquake      + :yquake-<ref>
./build q2repro     # -> iphands/quake2:q2repro     + :q2repro-<ref>
```

Bump the upstream version per flavor via env (any git ref — tag, branch, sha):

```bash
YQUAKE2_REF=QUAKE2_8_60 ./build yquake
Q2REPRO_REF=nightly     ./build q2repro
```

Defaults live at the top of `./build` (and as `ARG *_REF` in each Dockerfile):
`yquake` → `QUAKE2_8_70`, `q2repro` → the `rerelease-game` branch. `ccache` is
kept in a persistent BuildKit cache mount, so version bumps only recompile what
changed.

## Publish (you run this)

```bash
./publish            # publish every flavor built locally
./publish q2repro    # publish only the named flavor(s)
```

For each flavor it pushes the moving `:<flavor>` tag plus any local
`:<flavor>-<ref>` tags.

## Patches

Each flavor applies `patches/<flavor>/*.patch` (sorted) to the freshly cloned
source, in its own build layer (editing a patch doesn't force a re-clone).

- **`patches/q2repro/0001-accept-legacy-protocols.patch`** — upstream q2repro
  accepts *only* its own `Q2P_PROTOCOL_Q2REPRO`, so a vanilla client (yquake2,
  original Q2 = wire protocol 34) is rejected with `Unsupported protocol 2.`
  ("2" is q2proto's internal enum for VANILLA, not the wire number). This patch
  widens the server's accept-list to also include vanilla, R1Q2 and Q2PRO.
  Caveat: the re-release game's extended content may not fully represent over
  the vanilla protocol, so vanilla clients get a best-effort experience.

Drop a new `.patch` (unified diff, `-p1`, i.e. `git diff` output) into the
flavor's dir to add more.

## Run

Game data (pak files) is mounted read-only at
`/usr/share/games/quake2/baseq2`; a config mounted at `.../baseq2/server.cfg`
is `+exec`'d on boot. Extra run args are forwarded to the server binary. Point
your launch at the flavor tag you want:

```bash
podman create --replace --name quakeii -it \
    -v /main/scratch/games/q2/baseq2:/usr/share/games/quake2/baseq2:ro \
    -v /main/docker/quakeii/dm.cfg:/usr/share/games/quake2/baseq2/server.cfg:ro \
    -p 0.0.0.0:27910:27910/udp \
    iphands/quake2:yquake      # or iphands/quake2:q2repro
```

## Layout notes

- **yquake**: `q2ded` + `game.so` live in `/opt/yquake2` (the binary dir), which
  yquake2 searches for `game.so` — so the read-only data mount only needs pak
  files. `$XDG_DATA_HOME` is pre-created because yquake2's `Sys_Mkdir` is
  non-recursive.
- **q2repro**: built system-wide (`prefix=/usr`); `q2reproded` in `/usr/bin`,
  game libs in `/usr/lib*/q2repro/baseq2/`. The server loads the game lib from
  that baked `libdir`, not from `basedir`, so the read-only data mount only
  needs pak files. The entrypoint points `basedir` at the mount and `homedir`
  at writable `/opt/q2repro`.

`/opt/<flavor>/BUILT_FROM` records the exact upstream commit in each image.

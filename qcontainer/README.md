# qcontainer — optimized Q2REPRO dedicated server

A Fedora 44 container that builds an optimized
[q2repro](https://github.com/Paril/q2repro) (Paril's Q2PRO / Quake II
re-release fork) **dedicated server** (`q2reproded` + game libraries) with:

```
-O3 -march=sandybridge -mtune=sandybridge -O3 -pipe -falign-functions=32 -fomit-frame-pointer
```

Both the classic (`gamex86_64.so`) and the re-release (`game_x86_64.so`) game
libraries are built and installed; the server loads the re-release one first.

## Build

```bash
./build
```

Bump the game version (any git ref — branch, the rolling `nightly` tag, or a
commit sha):

```bash
Q2REPRO_REF=nightly ./build
Q2REPRO_REF=1523f11 ./build
```

The default ref lives at the top of `./build` (and as `ARG Q2REPRO_REF` in the
`Dockerfile`), defaulting to the `rerelease-game` default branch. `ccache` is
kept in a persistent BuildKit cache mount, so version bumps only recompile what
changed.

## Publish

```bash
./publish        # pushes iphands/quake2:<ref> and :latest  (you run this)
```

## Run

Game data (pak files) is mounted read-only at
`/usr/share/games/quake2/baseq2`; a config mounted at `.../baseq2/server.cfg`
is `+exec`'d on boot. Extra run args are forwarded to `q2reproded`.

```bash
podman create --replace --name quakeii -it \
    -v /main/scratch/games/q2/baseq2:/usr/share/games/quake2/baseq2:ro \
    -v /main/docker/quakeii/dm.cfg:/usr/share/games/quake2/baseq2/server.cfg:ro \
    -p 0.0.0.0:27910:27910/udp \
    iphands/quake2
```

## Layout notes

q2repro is built **system-wide** (`prefix=/usr`): the server lands at
`/usr/bin/q2reproded` and the game libs at `/usr/lib*/q2repro/baseq2/`. The
server loads the game lib from that baked `libdir` (not from `basedir`), so the
read-only data mount at `/usr/share/games/quake2/baseq2` only needs the pak
files. The entrypoint points `basedir` at that mount and `homedir` at the
writable `/opt/q2repro`. `/opt/q2repro/BUILT_FROM` records the exact upstream
commit.

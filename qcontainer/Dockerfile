# syntax=docker/dockerfile:1.7
#
# Optimized Q2REPRO (Paril's Q2PRO / re-release fork) dedicated server,
# built on Fedora 44.  https://github.com/Paril/q2repro
#
# Only the dedicated server (q2reproded) and the game libraries are built --
# no client/renderer -- which keeps build deps and the final image small.
# Both the classic game lib (gamex86_64.so) and the re-release game lib
# (game_x86_64.so) are installed; the server loads the re-release one first.
#
# Layer caching strategy:
#   - System deps install in their own layer (rarely invalidated).
#   - The clone/submodule checkout is keyed on Q2REPRO_REF, so bumping the
#     version only invalidates from that layer down.
#   - ccache lives in a BuildKit cache mount that survives across builds, so a
#     version bump only recompiles the objects that changed.

ARG FEDORA_VERSION=44

########################################
# Build stage
########################################
FROM fedora:${FEDORA_VERSION} AS build

# Bump this (or pass --build-arg Q2REPRO_REF=...) to change the version.
# Any git ref works: the default branch (rerelease-game), the rolling
# "nightly" tag, or a commit sha. Submodules are re-synced to the ref.
ARG Q2REPRO_REF=rerelease-game

# Requested optimization flags. buildtype=plain (below) adds no optimization
# flags of its own, so these are the only ones in effect.
#
# (-fvisibility is intentionally omitted: meson.build sets
# gnu_symbol_visibility:'hidden' per target, which would override it anyway,
# and hidden is correct here -- the game libs export GetGameAPI explicitly.)
ARG Q2_CFLAGS="-O3 -march=sandybridge -mtune=sandybridge -O3 -pipe -falign-functions=32 -fomit-frame-pointer"

# meson/ninja + C & C++ toolchains (re-release game is C++), ccache, and the
# minimal server deps: zlib, plus fmt/jsoncpp for the re-release game lib.
RUN dnf -y install --setopt=install_weak_deps=False \
        meson ninja-build gcc gcc-c++ git ccache \
        zlib-devel fmt-devel jsoncpp-devel pkgconf-pkg-config \
    && dnf clean all

ENV CCACHE_DIR=/ccache

# Clone at the requested ref (with submodules: q2proto + rerelease-game) in its
# own layer so a version bump is isolated for caching.
WORKDIR /src
RUN git clone --recurse-submodules https://github.com/Paril/q2repro . \
    && git checkout --detach "${Q2REPRO_REF}" \
    && git submodule update --init --recursive \
    && git rev-parse HEAD > /src/BUILT_FROM

# Configure a server-only, system-wide build (prefix=/usr). buildtype=plain
# means our CFLAGS/CXXFLAGS are the only optimization flags. Client video/audio
# backends are disabled so no SDL/GL/X11/etc. is needed.
RUN --mount=type=cache,target=/ccache \
    CC="ccache gcc" CXX="ccache g++" \
    CFLAGS="${Q2_CFLAGS}" CXXFLAGS="${Q2_CFLAGS}" \
    meson setup build --buildtype=plain \
        -Dsystem-wide=true -Dprefix=/usr \
        -Dsdl2=disabled -Dx11=disabled -Dwayland=disabled \
        -Dopenal=disabled -Dsoftware-sound=disabled \
        -Davcodec=disabled -Dlibpng=disabled -Dlibjpeg=disabled \
        -Dlibcurl=disabled -Dclient-ui=false \
    && meson compile -C build \
    && DESTDIR=/stage meson install -C build \
    && ccache -s

########################################
# Runtime stage
########################################
FROM fedora:${FEDORA_VERSION} AS runtime

LABEL org.opencontainers.image.title="q2repro-server" \
      org.opencontainers.image.description="Optimized Q2REPRO dedicated server (Fedora 44, -march=sandybridge)" \
      org.opencontainers.image.source="https://github.com/Paril/q2repro"

# Runtime libs: zlib for the server, fmt/jsoncpp/libstdc++ for the re-release
# game lib. Both stages are fedora:44 so SONAMEs match. Create an unprivileged
# user with a writable home for the server's write dir.
RUN dnf -y install --setopt=install_weak_deps=False \
        zlib fmt jsoncpp libstdc++ \
    && dnf clean all \
    && useradd --system --create-home --home-dir /opt/q2repro q2repro

# Install the staged tree (/usr/bin/q2reproded + /usr/lib*/q2repro/baseq2/*.so).
# The binary bakes its own LIBDIR/DATADIR, so copying the whole tree keeps the
# game-lib search path correct without hardcoding lib vs lib64.
COPY --from=build /stage/                 /
COPY --from=build /src/BUILT_FROM          /opt/q2repro/BUILT_FROM
COPY entrypoint.sh                         /usr/local/bin/entrypoint.sh

RUN mkdir -p /usr/share/games/quake2/baseq2 \
    && chmod +x /usr/local/bin/entrypoint.sh \
    && chown -R q2repro:q2repro /opt/q2repro

ENV HOME=/opt/q2repro
USER q2repro
WORKDIR /opt/q2repro

EXPOSE 27910/udp
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

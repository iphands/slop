# syntax=docker/dockerfile:1.7
#
# Fedora 44 with dnf already pointed at the pkgcache proxy.
#
# Two uses:
#   * a base image -- FROM it and every `dnf install` in the child build pulls
#     through the LAN cache instead of hammering a Fedora mirror
#   * a client to exercise the proxy with (`dnf install` here is the real Rule A
#     test for the /fedora/ routes, metadata and .rpm alike)
#
# The cache endpoint is baked at build time (ARG CACHE, default = the live
# homelab deployment) and recorded in PKGCACHE_URL so a running container can say
# what it was built against. Re-point a running container -- or a child build --
# with  CACHE=http://other:port pkgcache-setup .
#
# No dnf metadata is baked in: pkgcache-setup runs with --no-makecache, so the
# image builds with no network at all and never ships stale repodata.
ARG FEDORA_RELEASE=44
FROM docker.io/library/fedora:${FEDORA_RELEASE}

ARG CACHE=http://noir.lan:3129

LABEL org.opencontainers.image.title="pkgcache-fedora" \
      org.opencontainers.image.description="Fedora 44 with dnf pre-configured against the pkgcache LAN proxy" \
      org.opencontainers.image.source="https://github.com/iphands/slop"

COPY pkgcache-setup /usr/local/bin/pkgcache-setup
RUN chmod 0755 /usr/local/bin/pkgcache-setup \
    && CACHE="${CACHE}" /usr/local/bin/pkgcache-setup --no-makecache

ENV PKGCACHE_URL=${CACHE}

CMD ["/bin/bash"]

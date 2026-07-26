# syntax=docker/dockerfile:1.7
#
# Ubuntu 22.04 (jammy) with apt already pointed at the pkgcache proxy.
#
# Two uses:
#   * a base image -- FROM it and every `apt-get install` in the child build pulls
#     through the LAN cache instead of archive.ubuntu.com
#   * a client to exercise the proxy with (`apt-get install` here is the real
#     Rule A test for the /ubuntu/ and /ubuntu-security/ routes)
#
# The cache endpoint is baked at build time (ARG CACHE, default = the live homelab
# deployment) and recorded in PKGCACHE_URL so a running container can say what it
# was built against. Re-point a running container -- or a child build -- with
# CACHE=http://other:port pkgcache-setup .
#
# No apt lists are baked in: pkgcache-setup runs with --no-update, so the image
# builds with no network and never ships stale indices. The child build's first
# `apt-get update` fetches them through the cache.
ARG UBUNTU_RELEASE=22.04
FROM docker.io/library/ubuntu:${UBUNTU_RELEASE}

ARG CACHE=http://noir.lan:3129

LABEL org.opencontainers.image.title="pkgcache-ubuntu" \
      org.opencontainers.image.description="Ubuntu 22.04 with apt pre-configured against the pkgcache LAN proxy" \
      org.opencontainers.image.source="https://github.com/iphands/slop"

COPY pkgcache-setup /usr/local/bin/pkgcache-setup
RUN chmod 0755 /usr/local/bin/pkgcache-setup \
    && CACHE="${CACHE}" /usr/local/bin/pkgcache-setup --no-update

ENV PKGCACHE_URL=${CACHE}

CMD ["/bin/bash"]

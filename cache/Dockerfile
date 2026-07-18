# syntax=docker/dockerfile:1.7
#
# Reverse caching proxy for Debian & Fedora package repositories.
# Based on the unprivileged nginx image so it runs happily as an arbitrary
# non-root --user ${APP_UID}:${APP_GID} (see ../run). The master listens on
# 8080 and every writable path lives under the mounted /var/cache/nginx volume
# or /tmp, so no root is needed at runtime.
FROM docker.io/nginxinc/nginx-unprivileged:1.27-alpine

LABEL org.opencontainers.image.title="pkgcache" \
      org.opencontainers.image.description="Reverse caching proxy for Debian (apt) and Fedora (dnf) package repositories" \
      org.opencontainers.image.source="https://github.com/iphands/slop"

# Drop the stock default server (also on :8080) so it can't shadow ours, then
# install our config. Done as root; runtime user is overridden by ./run anyway.
USER root
RUN rm -f /etc/nginx/conf.d/default.conf
COPY nginx.conf /etc/nginx/nginx.conf
COPY conf.d/pkgcache.conf /etc/nginx/conf.d/pkgcache.conf
USER nginx

EXPOSE 8080

# busybox wget ships in the alpine base; curl does not.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1

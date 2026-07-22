# content-sync runtime — Alpine + static musl binary (no glibc).
#
# Multi-arch note (Podman/Docker on amd64 building arm64):
#   Foreign-arch `RUN` needs QEMU binfmt. We avoid that by preparing
#   certs/user on $BUILDPLATFORM, then only COPY into the target image.
#
# Build context must contain:
#   binaries/amd64/content-sync
#   binaries/arm64/content-sync
# (staged by scripts/registry-image-push.sh)

# ── Prepare arch-independent files on the host/build platform ──
FROM --platform=$BUILDPLATFORM docker.io/library/alpine:latest AS prepare

RUN apk add --no-cache ca-certificates \
 && adduser -D -h /data -u 1000 appsync \
 && mkdir -p /data/.content-sync /data/files \
 && chown -R appsync:appsync /data

# ── Final image for TARGETPLATFORM (amd64 / arm64) ─────────────
FROM docker.io/library/alpine:latest

ARG TARGETARCH
ARG APP_VERSION=0.0.0

LABEL org.opencontainers.image.title="content-sync" \
      org.opencontainers.image.description="Bidirectional raw-file sync with Web UI" \
      org.opencontainers.image.version="${APP_VERSION}" \
      org.opencontainers.image.source="https://github.com/manhavn/content-sync"

# Text files / cert bundle from prepare (no foreign-arch execution)
COPY --from=prepare /etc/ssl/certs /etc/ssl/certs
COPY --from=prepare /usr/share/ca-certificates /usr/share/ca-certificates
COPY --from=prepare /etc/ca-certificates.conf /etc/ca-certificates.conf
COPY --from=prepare /etc/passwd /etc/passwd
COPY --from=prepare /etc/group /etc/group
COPY --from=prepare /etc/shadow /etc/shadow
COPY --from=prepare /data /data

# Static musl binary for this TARGETARCH (amd64 | arm64)
COPY --chmod=755 binaries/${TARGETARCH}/content-sync /usr/local/bin/content-sync

# Config + default watch roots live under $HOME → mount a volume at /data
ENV HOME=/data \
    RUST_LOG=info \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

WORKDIR /data
USER appsync

# Web UI default — 8080 matches Cloud Run's common containerPort / PORT default.
# Override: … content-sync serve --bind 0.0.0.0:$PORT --no-log
EXPOSE 8080

# Persist config.sqlite, tokens, pid, logs, and per-connection files
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/content-sync"]
CMD ["serve", "--bind", "0.0.0.0:8080", "--no-log"]

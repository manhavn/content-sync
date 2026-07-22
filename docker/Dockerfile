# content-sync runtime — Alpine + static musl binary (no glibc).
# Multi-arch: buildx sets TARGETARCH=amd64|arm64.
#
# Build context must contain:
#   binaries/amd64/content-sync
#   binaries/arm64/content-sync
# (staged by scripts/docker-push.sh)

FROM docker.io/library/alpine:latest

ARG TARGETARCH
ARG APP_VERSION=0.0.0

LABEL org.opencontainers.image.title="content-sync" \
      org.opencontainers.image.description="Bidirectional raw-file sync with Web UI" \
      org.opencontainers.image.version="${APP_VERSION}" \
      org.opencontainers.image.source="https://github.com/manhavn/content-sync"

# ca-certificates for HTTPS to remote DBs; static binary otherwise has no dynamic deps
RUN apk add --no-cache ca-certificates \
 && adduser -D -h /data -u 1000 appsync \
 && mkdir -p /data/.content-sync /data/files \
 && chown -R appsync:appsync /data

COPY binaries/${TARGETARCH}/content-sync /usr/local/bin/content-sync
RUN chmod 755 /usr/local/bin/content-sync

# Config + default watch roots live under $HOME → mount a volume at /data
ENV HOME=/data \
    RUST_LOG=info

WORKDIR /data
USER appsync

# Web UI default — 8080 matches Cloud Run's common containerPort / PORT default.
# Override: docker run ... content-sync serve --bind 0.0.0.0:$PORT --no-log
EXPOSE 8080

# Persist config.sqlite, tokens, pid, logs, and per-connection files
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/content-sync"]
CMD ["serve", "--bind", "0.0.0.0:8080", "--no-log"]

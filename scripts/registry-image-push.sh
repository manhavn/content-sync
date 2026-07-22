#!/usr/bin/env bash
# Build Linux musl binaries → Alpine runtime image → push to container registries.
#
# Pipeline:
#   1) scripts/build-release-multi.sh --only *linux-musl  (skippable)
#   2) Stage binaries into docker/binaries/{amd64,arm64}/
#   3) Container build from docker/Dockerfile (alpine:latest)
#        engine: podman (preferred) or docker/buildx — see --engine / CONTAINER_ENGINE
#   4) login + push tags to one or more hubs
#
# Supported hubs (--registry / --to):
#   dockerhub | docker.io     Docker Hub          docker.io/<user>/<image>
#   ghcr | github             GitHub Packages     ghcr.io/<owner>/<image>
#   gcr                       Google GCR (legacy) gcr.io/<project>/<image>
#   gar | artifact            Artifact Registry   <region>-docker.pkg.dev/<project>/<repo>/<image>
#   quay                      Quay.io             quay.io/<org>/<image>
#   custom                    Any registry host   <host>/<path>/<image>
#
# Auth (env or flags — prefer env so tokens stay out of shell history):
#   Docker Hub:  DOCKERHUB_USER + DOCKERHUB_TOKEN   (or --username/--token)
#   GHCR:        GHCR_USER + GHCR_TOKEN              (PAT with read:packages, write:packages, delete:packages)
#   GCR/GAR:     GCP_SA_KEY_FILE (JSON SA key)  or  use gcloud:
#                  gcloud auth configure-docker
#                  gcloud auth configure-docker REGION-docker.pkg.dev
#                Optional: GCP_PROJECT, GCP_REGION, GCP_REPOSITORY
#   Quay:        QUAY_USER + QUAY_TOKEN
#   Custom:      REGISTRY_USER + REGISTRY_TOKEN + REGISTRY_HOST
#
# Google registries (quick guide):
#   • Prefer Artifact Registry (GAR), not legacy gcr.io.
#   • Create a Docker repo:  gcloud artifacts repositories create content-sync \
#         --repository-format=docker --location=REGION --project=PROJECT
#   • Image URL: REGION-docker.pkg.dev/PROJECT/REPO/IMAGE:TAG
#       e.g.  us-central1-docker.pkg.dev/my-proj/content-sync/content-sync:0.1.0
#   • Auth options:
#       A) gcloud auth login && gcloud auth configure-docker REGION-docker.pkg.dev
#       B) Service account JSON:
#            export GCP_SA_KEY_FILE=./sa.json
#            # script logs in with user=_json_key password=@sa.json
#   • Cloud Run: container port 8080 (image CMD already binds 0.0.0.0:8080 --no-log).
#   • Mount a volume/PVC at /data for ~/.content-sync persistence (config, tokens, files).
#
# Usage examples:
#   # Local image only (host arch), no push — Docker or Podman
#   scripts/registry-image-push.sh --no-push --load
#   scripts/registry-image-push.sh --engine podman --no-push --load
#
#   # Docker Hub
#   export DOCKERHUB_USER=myuser DOCKERHUB_TOKEN=***
#   scripts/registry-image-push.sh --to dockerhub --image content-sync
#   scripts/registry-image-push.sh --engine podman --to dockerhub
#
#   # GitHub Container Registry
#   export GHCR_USER=myuser GHCR_TOKEN=ghp_***
#   scripts/registry-image-push.sh --to ghcr --image content-sync
#
#   # Google Artifact Registry
#   export GCP_PROJECT=my-proj GCP_REGION=us-central1 GCP_REPOSITORY=content-sync
#   export GCP_SA_KEY_FILE=./sa-key.json   # or pre-run gcloud auth configure-docker
#   scripts/registry-image-push.sh --to gar --image content-sync
#
#   # Multi-hub in one go
#   scripts/registry-image-push.sh --to dockerhub,ghcr --skip-binary-build
#
#   # Custom registry
#   export REGISTRY_HOST=registry.example.com REGISTRY_USER=u REGISTRY_TOKEN=t
#   scripts/registry-image-push.sh --to custom --image org/content-sync
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"

BIN_NAME="content-sync"
PKG_VERSION="$(
  grep -E '^version\s*=' "$ROOT/Cargo.toml" | head -1 \
    | sed -E 's/^version\s*=\s*"([^"]+)".*/\1/'
)"
PKG_VERSION="${PKG_VERSION:-0.0.0}"

DOCKER_DIR="$ROOT/docker"
STAGE_DIR="$DOCKER_DIR/binaries"
DIST_DIR="${DIST_DIR:-$ROOT/dist}"

# Defaults
IMAGE_NAME="${IMAGE_NAME:-content-sync}"
TAGS=("${PKG_VERSION}" "latest")
PLATFORMS="linux/amd64,linux/arm64"
SKIP_BINARY_BUILD=0
SKIP_TEST=0
NO_PUSH=0
DRY_RUN=0
LOAD_LOCAL=0
TO_REGISTRIES=()
# Container engine: auto | docker | podman  (env CONTAINER_ENGINE also works)
ENGINE_PREF="${CONTAINER_ENGINE:-auto}"
CTR=""   # resolved command: docker or podman
# Shared / per-hub credentials (flags fill these; env used as fallback)
FLAG_USER=""
FLAG_TOKEN=""
FLAG_NAMESPACE=""   # dockerhub user / ghcr owner / quay org
GCP_PROJECT_FLAG=""
GCP_REGION_FLAG=""
GCP_REPO_FLAG=""
CUSTOM_HOST_FLAG=""
EXTRA_BUILD_ARGS=()

usage() {
  # Header comment block only (stop before script body)
  sed -n '2,/^set -euo pipefail$/p' "$0" | head -n -1 | sed 's/^# \?//'
  cat <<'EOF'

Flags:
  --image NAME              Image name (default: content-sync)
  --tag TAG[,TAG...]        Tags (default: <Cargo version>,latest)
  --platforms LIST          platforms (default: linux/amd64,linux/arm64)
  --engine auto|docker|podman
                            Container engine (default: auto — podman if usable, else docker)
                            Env: CONTAINER_ENGINE=podman|docker
  --to HUB[,HUB...]         dockerhub,ghcr,gcr,gar,quay,custom (repeatable)
  --registry HUB            alias for a single --to
  --username USER           override username for the selected hub(s)
  --token TOKEN             override token/password (prefer env vars)
  --namespace NS            Docker Hub user / GHCR owner / Quay org (default: username)
  --project ID              GCP project (gcr/gar)
  --region REGION           GCP region for Artifact Registry (e.g. us-central1)
  --repository NAME         GAR repository name (default: content-sync or image name)
  --registry-host HOST      custom registry host (e.g. registry.example.com)
  --skip-binary-build       use existing dist/*-linux-musl binaries
  --skip-test               pass --skip-test to multi build
  --no-push                 build but do not push
  --load                    load into local engine (single platform / host arch)
  --dist-dir DIR            binary dist dir (default: ./dist)
  --dry-run                 print actions only
  -h, --help                this help

Podman notes:
  scripts/registry-image-push.sh --engine podman --to dockerhub
  Multi-arch uses: podman build --platform … --manifest … && podman manifest push
  Login: podman login (same username/token env vars as Docker)
EOF
  exit "${1:-0}"
}

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'warn: %s\n' "$*" >&2; }
err()  { printf 'error: %s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

die() { err "$*"; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage 0 ;;
    --image) IMAGE_NAME="${2:-}"; shift 2 ;;
    --image=*) IMAGE_NAME="${1#--image=}"; shift ;;
    --tag|--tags)
      IFS=',' read -r -a TAGS <<< "${2:-}"
      shift 2
      ;;
    --tag=*|--tags=*)
      IFS=',' read -r -a TAGS <<< "${1#*=}"
      shift
      ;;
    --platforms) PLATFORMS="${2:-}"; shift 2 ;;
    --platforms=*) PLATFORMS="${1#--platforms=}"; shift ;;
    --to)
      IFS=',' read -r -a _to <<< "${2:-}"
      TO_REGISTRIES+=("${_to[@]}")
      shift 2
      ;;
    --to=*)
      IFS=',' read -r -a _to <<< "${1#--to=}"
      TO_REGISTRIES+=("${_to[@]}")
      shift
      ;;
    --registry) TO_REGISTRIES+=("${2:-}"); shift 2 ;;
    --registry=*) TO_REGISTRIES+=("${1#--registry=}"); shift ;;
    --username|--user) FLAG_USER="${2:-}"; shift 2 ;;
    --username=*|--user=*) FLAG_USER="${1#*=}"; shift ;;
    --token|--password) FLAG_TOKEN="${2:-}"; shift 2 ;;
    --token=*|--password=*) FLAG_TOKEN="${1#*=}"; shift ;;
    --namespace|--org|--owner) FLAG_NAMESPACE="${2:-}"; shift 2 ;;
    --namespace=*|--org=*|--owner=*) FLAG_NAMESPACE="${1#*=}"; shift ;;
    --project) GCP_PROJECT_FLAG="${2:-}"; shift 2 ;;
    --project=*) GCP_PROJECT_FLAG="${1#--project=}"; shift ;;
    --region) GCP_REGION_FLAG="${2:-}"; shift 2 ;;
    --region=*) GCP_REGION_FLAG="${1#--region=}"; shift ;;
    --repository) GCP_REPO_FLAG="${2:-}"; shift 2 ;;
    --repository=*) GCP_REPO_FLAG="${1#--repository=}"; shift ;;
    --registry-host) CUSTOM_HOST_FLAG="${2:-}"; shift 2 ;;
    --registry-host=*) CUSTOM_HOST_FLAG="${1#--registry-host=}"; shift ;;
    --skip-binary-build|--skip-build) SKIP_BINARY_BUILD=1; shift ;;
    --skip-test) SKIP_TEST=1; shift ;;
    --no-push) NO_PUSH=1; shift ;;
    --load) LOAD_LOCAL=1; shift ;;
    --engine)
      ENGINE_PREF="${2:-}"
      shift 2
      ;;
    --engine=*)
      ENGINE_PREF="${1#--engine=}"
      shift
      ;;
    --dist-dir) DIST_DIR="${2:-}"; shift 2 ;;
    --dist-dir=*) DIST_DIR="${1#--dist-dir=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    *)
      err "unknown arg: $1"
      usage 1
      ;;
  esac
done

# Normalize hub aliases
normalize_hub() {
  case "$(echo "$1" | tr '[:upper:]' '[:lower:]')" in
    dockerhub|docker.io|docker|hub) echo dockerhub ;;
    ghcr|github|ghcr.io) echo ghcr ;;
    gcr|gcr.io) echo gcr ;;
    gar|artifact|artifactregistry|pkg.dev) echo gar ;;
    quay|quay.io) echo quay ;;
    custom) echo custom ;;
    "") return 1 ;;
    *)
      warn "unknown hub '$1' — treating as custom host name"
      echo "custom:$1"
      ;;
  esac
}

NORMALIZED=()
for h in "${TO_REGISTRIES[@]+"${TO_REGISTRIES[@]}"}"; do
  [[ -z "${h// }" ]] && continue
  NORMALIZED+=("$(normalize_hub "$h")")
done
TO_REGISTRIES=("${NORMALIZED[@]+"${NORMALIZED[@]}"}")

if [[ ${#TO_REGISTRIES[@]} -eq 0 && "$NO_PUSH" -eq 0 ]]; then
  # Default: build local only if no registry requested
  warn "no --to registry specified; building only (--no-push implied). Use --to dockerhub|ghcr|gar|..."
  NO_PUSH=1
fi

HOST_ARCH="$(uname -m)"
case "$HOST_ARCH" in
  x86_64|amd64) HOST_DOCKER_ARCH=amd64 ;;
  aarch64|arm64) HOST_DOCKER_ARCH=arm64 ;;
  *) HOST_DOCKER_ARCH=amd64; warn "unknown host arch $HOST_ARCH, defaulting staging to amd64" ;;
esac

run() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '[dry-run]'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi
  "$@"
}

engine_usable() {
  local e="$1"
  have "$e" || return 1
  # docker needs a daemon; podman works rootless without long-running daemon
  if [[ "$e" == "docker" ]]; then
    docker info >/dev/null 2>&1 || return 1
  elif [[ "$e" == "podman" ]]; then
    # `podman info` can be slow first time; --format keeps it light
    podman info >/dev/null 2>&1 || return 1
  fi
  return 0
}

resolve_engine() {
  local pref
  pref="$(echo "${ENGINE_PREF:-auto}" | tr '[:upper:]' '[:lower:]')"

  # Dry-run: pick an engine name without requiring a live daemon
  if [[ "$DRY_RUN" -eq 1 ]]; then
    case "$pref" in
      docker|podman) CTR="$pref" ;;
      auto|"")
        if have podman; then CTR=podman
        elif have docker; then CTR=docker
        else CTR=podman
        fi
        ;;
      *) die "unknown --engine '$ENGINE_PREF' (use auto|docker|podman)" ;;
    esac
    log "container engine: $CTR (dry-run)"
    return 0
  fi

  case "$pref" in
    docker|podman)
      engine_usable "$pref" || die "$pref not available or not usable (is it installed / running?)"
      CTR="$pref"
      ;;
    auto|"")
      if engine_usable podman; then
        CTR=podman
      elif engine_usable docker; then
        CTR=docker
      else
        die "neither podman nor docker is usable. Install one, or set --engine podman|docker"
      fi
      ;;
    *)
      die "unknown --engine '$ENGINE_PREF' (use auto|docker|podman)"
      ;;
  esac
  log "container engine: $CTR"
}

# CTR-agnostic login: docker login / podman login
ctr_login() {
  local host="$1" user="$2"
  # password on stdin
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] $CTR login $host -u $user --password-stdin"
    return 0
  fi
  "$CTR" login "$host" -u "$user" --password-stdin
}

# ── 1) Binaries (musl) ─────────────────────────────────────────
build_binaries() {
  local targets=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
  if [[ "$LOAD_LOCAL" -eq 1 ]]; then
    # single-arch local load: only need host musl
    if [[ "$HOST_DOCKER_ARCH" == "arm64" ]]; then
      targets=(aarch64-unknown-linux-musl)
    else
      targets=(x86_64-unknown-linux-musl)
    fi
  fi

  local only
  only=$(IFS=,; echo "${targets[*]}")
  log "building musl binaries: $only"
  local args=(--only "$only" --out-dir "$DIST_DIR")
  [[ "$SKIP_TEST" -eq 1 ]] && args+=(--skip-test)
  run "$ROOT/scripts/build-release-multi.sh" "${args[@]}"
}

stage_binaries() {
  log "staging binaries → $STAGE_DIR"
  run mkdir -p "$STAGE_DIR/amd64" "$STAGE_DIR/arm64"

  local amd64_src arm64_src
  amd64_src="$DIST_DIR/${BIN_NAME}-v${PKG_VERSION}-x86_64-unknown-linux-musl"
  arm64_src="$DIST_DIR/${BIN_NAME}-v${PKG_VERSION}-aarch64-unknown-linux-musl"

  if [[ -f "$amd64_src" ]]; then
    run cp -f "$amd64_src" "$STAGE_DIR/amd64/content-sync"
    run chmod +x "$STAGE_DIR/amd64/content-sync"
    log "  amd64 ← $amd64_src"
  else
    warn "missing $amd64_src (linux/amd64 image arch will fail if selected)"
  fi
  if [[ -f "$arm64_src" ]]; then
    run cp -f "$arm64_src" "$STAGE_DIR/arm64/content-sync"
    run chmod +x "$STAGE_DIR/arm64/content-sync"
    log "  arm64 ← $arm64_src"
  else
    warn "missing $arm64_src (linux/arm64 image arch will fail if selected)"
  fi

  if [[ "$DRY_RUN" -eq 0 \
    && ! -f "$STAGE_DIR/amd64/content-sync" \
    && ! -f "$STAGE_DIR/arm64/content-sync" ]]; then
    die "no staged binaries; run without --skip-binary-build or place musl builds in $DIST_DIR"
  fi
}

# ── 2) Image refs & login ──────────────────────────────────────
resolve_user() {
  local hub="$1"
  if [[ -n "$FLAG_USER" ]]; then echo "$FLAG_USER"; return; fi
  case "$hub" in
    dockerhub) echo "${DOCKERHUB_USER:-${DOCKER_USER:-}}" ;;
    ghcr)      echo "${GHCR_USER:-${GITHUB_USER:-${GITHUB_ACTOR:-}}}" ;;
    gcr|gar)   echo "${GCP_USER:-_json_key}" ;;
    quay)      echo "${QUAY_USER:-}" ;;
    custom*)   echo "${REGISTRY_USER:-}" ;;
  esac
}

resolve_token() {
  local hub="$1"
  if [[ -n "$FLAG_TOKEN" ]]; then echo "$FLAG_TOKEN"; return; fi
  case "$hub" in
    dockerhub) echo "${DOCKERHUB_TOKEN:-${DOCKER_TOKEN:-${DOCKER_PASSWORD:-}}}" ;;
    ghcr)      echo "${GHCR_TOKEN:-${GITHUB_TOKEN:-}}" ;;
    gcr|gar)
      if [[ -n "${GCP_SA_KEY_FILE:-}" && -f "${GCP_SA_KEY_FILE}" ]]; then
        cat "${GCP_SA_KEY_FILE}"
      else
        echo "${GCP_TOKEN:-${GCP_SA_KEY_JSON:-}}"
      fi
      ;;
    quay)      echo "${QUAY_TOKEN:-${QUAY_PASSWORD:-}}" ;;
    custom*)   echo "${REGISTRY_TOKEN:-${REGISTRY_PASSWORD:-}}" ;;
  esac
}

resolve_namespace() {
  local hub="$1" user="$2"
  if [[ -n "$FLAG_NAMESPACE" ]]; then echo "$FLAG_NAMESPACE"; return; fi
  case "$hub" in
    dockerhub) echo "${DOCKERHUB_NAMESPACE:-$user}" ;;
    ghcr)      echo "${GHCR_NAMESPACE:-${GITHUB_REPOSITORY_OWNER:-$user}}" ;;
    quay)      echo "${QUAY_NAMESPACE:-$user}" ;;
    *)         echo "$user" ;;
  esac
}

image_refs_for_hub() {
  # prints one fully-qualified ref per tag (without still tagging locally)
  local hub="$1"
  local user ns project region repo host
  user="$(resolve_user "$hub")"
  ns="$(resolve_namespace "$hub" "$user")"

  case "$hub" in
    dockerhub)
      [[ -n "$ns" ]] || die "dockerhub: set DOCKERHUB_USER or --username / --namespace"
      for t in "${TAGS[@]}"; do
        echo "docker.io/${ns}/${IMAGE_NAME}:${t}"
      done
      ;;
    ghcr)
      [[ -n "$ns" ]] || die "ghcr: set GHCR_USER or --username / --namespace"
      # GHCR likes lowercase
      ns="$(echo "$ns" | tr '[:upper:]' '[:lower:]')"
      local img
      img="$(echo "$IMAGE_NAME" | tr '[:upper:]' '[:lower:]')"
      for t in "${TAGS[@]}"; do
        echo "ghcr.io/${ns}/${img}:${t}"
      done
      ;;
    gcr)
      project="${GCP_PROJECT_FLAG:-${GCP_PROJECT:-}}"
      [[ -n "$project" ]] || die "gcr: set GCP_PROJECT or --project"
      for t in "${TAGS[@]}"; do
        echo "gcr.io/${project}/${IMAGE_NAME}:${t}"
      done
      ;;
    gar)
      project="${GCP_PROJECT_FLAG:-${GCP_PROJECT:-}}"
      region="${GCP_REGION_FLAG:-${GCP_REGION:-}}"
      repo="${GCP_REPO_FLAG:-${GCP_REPOSITORY:-$IMAGE_NAME}}"
      [[ -n "$project" ]] || die "gar: set GCP_PROJECT or --project"
      [[ -n "$region" ]] || die "gar: set GCP_REGION or --region (e.g. us-central1)"
      for t in "${TAGS[@]}"; do
        echo "${region}-docker.pkg.dev/${project}/${repo}/${IMAGE_NAME}:${t}"
      done
      ;;
    quay)
      [[ -n "$ns" ]] || die "quay: set QUAY_USER or --username / --namespace"
      for t in "${TAGS[@]}"; do
        echo "quay.io/${ns}/${IMAGE_NAME}:${t}"
      done
      ;;
    custom|custom:*)
      host="${CUSTOM_HOST_FLAG:-${REGISTRY_HOST:-}}"
      if [[ "$hub" == custom:* ]]; then
        host="${hub#custom:}"
      fi
      [[ -n "$host" ]] || die "custom: set REGISTRY_HOST or --registry-host"
      # IMAGE_NAME may already include path "org/name"
      for t in "${TAGS[@]}"; do
        echo "${host}/${IMAGE_NAME}:${t}"
      done
      ;;
  esac
}

login_hub() {
  local hub="$1"
  local user token host
  user="$(resolve_user "$hub")"
  token="$(resolve_token "$hub")"

  case "$hub" in
    dockerhub)
      if [[ -z "$user" || -z "$token" ]]; then
        warn "dockerhub: no credentials — assuming already logged in ($CTR login)"
        return 0
      fi
      log "$CTR login docker.io as $user"
      printf '%s' "$token" | ctr_login docker.io "$user"
      ;;
    ghcr)
      if [[ -z "$user" || -z "$token" ]]; then
        warn "ghcr: no credentials — assuming already logged in"
        return 0
      fi
      log "$CTR login ghcr.io as $user"
      printf '%s' "$token" | ctr_login ghcr.io "$user"
      ;;
    gcr)
      host="gcr.io"
      if [[ -n "$token" ]]; then
        log "$CTR login $host as ${user:-_json_key} (JSON key / token)"
        printf '%s' "$token" | ctr_login "$host" "${user:-_json_key}"
      elif have gcloud; then
        log "gcloud auth configure-docker (gcr.io) — also run: $CTR login gcr.io if needed"
        run gcloud auth configure-docker --quiet
        # Podman does not always read Docker's config.json; hint only
        if [[ "$CTR" == "podman" ]]; then
          warn "podman: if push fails auth, use GCP_SA_KEY_FILE or: podman login gcr.io -u _json_key --password-stdin < sa.json"
        fi
      else
        die "gcr: provide GCP_SA_KEY_FILE / GCP_TOKEN or install gcloud and run: gcloud auth configure-docker"
      fi
      ;;
    gar)
      local region
      region="${GCP_REGION_FLAG:-${GCP_REGION:-}}"
      [[ -n "$region" ]] || die "gar login: need GCP_REGION"
      host="${region}-docker.pkg.dev"
      if [[ -n "$token" ]]; then
        log "$CTR login $host as ${user:-_json_key}"
        printf '%s' "$token" | ctr_login "$host" "${user:-_json_key}"
      elif have gcloud; then
        log "gcloud auth configure-docker $host"
        run gcloud auth configure-docker "$host" --quiet
        if [[ "$CTR" == "podman" ]]; then
          warn "podman: if push fails auth, use GCP_SA_KEY_FILE or: podman login $host -u _json_key --password-stdin < sa.json"
        fi
      else
        die "gar: provide GCP_SA_KEY_FILE or: gcloud auth configure-docker ${host}"
      fi
      ;;
    quay)
      if [[ -z "$user" || -z "$token" ]]; then
        warn "quay: no credentials — assuming already logged in"
        return 0
      fi
      log "$CTR login quay.io as $user"
      printf '%s' "$token" | ctr_login quay.io "$user"
      ;;
    custom|custom:*)
      host="${CUSTOM_HOST_FLAG:-${REGISTRY_HOST:-}}"
      if [[ "$hub" == custom:* ]]; then host="${hub#custom:}"; fi
      [[ -n "$host" ]] || die "custom login: need REGISTRY_HOST"
      if [[ -z "$user" || -z "$token" ]]; then
        warn "custom ($host): no credentials — assuming already logged in"
        return 0
      fi
      log "$CTR login $host as $user"
      printf '%s' "$token" | ctr_login "$host" "$user"
      ;;
  esac
}

# ── 3) Build & push ────────────────────────────────────────────

build_with_docker() {
  local platforms="$1"
  shift
  local tag_args=("$@")

  local use_buildx=0
  if docker buildx version >/dev/null 2>&1; then
    use_buildx=1
  fi

  if [[ "$use_buildx" -eq 1 ]]; then
    if ! docker buildx inspect content-sync-builder >/dev/null 2>&1; then
      log "creating buildx builder content-sync-builder"
      run docker buildx create --name content-sync-builder --use 2>/dev/null \
        || run docker buildx use content-sync-builder 2>/dev/null \
        || true
      run docker buildx use content-sync-builder 2>/dev/null || run docker buildx use default || true
    else
      run docker buildx use content-sync-builder 2>/dev/null || true
    fi

    local build_cmd=(docker buildx build)
    build_cmd+=(--platform "$platforms")
    build_cmd+=(--build-arg "APP_VERSION=${PKG_VERSION}")
    build_cmd+=("${tag_args[@]}")
    build_cmd+=(-f "$DOCKER_DIR/Dockerfile")
    build_cmd+=("$DOCKER_DIR")

    if [[ "$NO_PUSH" -eq 1 ]]; then
      if [[ "$LOAD_LOCAL" -eq 1 ]] || [[ "$platforms" != *","* ]]; then
        build_cmd+=(--load)
      else
        warn "multi-platform build cannot --load; using --output=type=image,push=false"
        build_cmd+=(--output "type=image,push=false")
      fi
    else
      build_cmd+=(--push)
    fi

    log "docker buildx build (platforms=$platforms)"
    run "${build_cmd[@]}"
  else
    warn "buildx not available — building host arch only ($HOST_DOCKER_ARCH)"
    if [[ "$DRY_RUN" -eq 0 && ! -f "$STAGE_DIR/${HOST_DOCKER_ARCH}/content-sync" ]]; then
      die "missing staged binary for $HOST_DOCKER_ARCH"
    fi
    log "docker build (host arch)"
    run docker build \
      --build-arg "TARGETARCH=${HOST_DOCKER_ARCH}" \
      --build-arg "APP_VERSION=${PKG_VERSION}" \
      "${tag_args[@]}" \
      -f "$DOCKER_DIR/Dockerfile" \
      "$DOCKER_DIR"

    if [[ "$NO_PUSH" -eq 0 ]]; then
      local t
      for t in "${ALL_TAGS[@]}"; do
        log "docker push $t"
        run docker push "$t"
      done
    fi
  fi
}

build_with_podman() {
  local platforms="$1"
  shift
  local tag_args=("$@")

  # Single platform (or --load / --no-push local): plain podman build
  if [[ "$platforms" != *","* ]] || [[ "$LOAD_LOCAL" -eq 1 ]]; then
    local plat="$platforms"
    if [[ "$LOAD_LOCAL" -eq 1 ]]; then
      plat="linux/${HOST_DOCKER_ARCH}"
    fi
    # For single-arch classic build, pass TARGETARCH (buildkit/podman may set it;
    # still set explicitly for reliability).
    local arch="${plat##*/}"
    log "podman build (platform=$plat)"
    run podman build \
      --platform "$plat" \
      --build-arg "TARGETARCH=${arch}" \
      --build-arg "APP_VERSION=${PKG_VERSION}" \
      "${tag_args[@]}" \
      -f "$DOCKER_DIR/Dockerfile" \
      "$DOCKER_DIR"

    if [[ "$NO_PUSH" -eq 0 ]]; then
      local t
      for t in "${ALL_TAGS[@]}"; do
        log "podman push $t"
        run podman push "$t"
      done
    fi
    return 0
  fi

  # Multi-arch: build into a local manifest list, then push each remote tag.
  # https://docs.podman.io/en/latest/markdown/podman-build.1.html (--manifest)
  local manifest="localhost/${IMAGE_NAME}:manifest-${PKG_VERSION}"
  log "podman multi-arch build → manifest $manifest (platforms=$platforms)"

  # Remove stale manifest if present (ignore errors)
  if [[ "$DRY_RUN" -eq 0 ]]; then
    podman manifest rm "$manifest" >/dev/null 2>&1 || true
    podman rmi "$manifest" >/dev/null 2>&1 || true
  else
    echo "[dry-run] podman manifest rm $manifest  (ignore errors)"
  fi

  run podman build \
    --platform "$platforms" \
    --manifest "$manifest" \
    --build-arg "APP_VERSION=${PKG_VERSION}" \
    -f "$DOCKER_DIR/Dockerfile" \
    "$DOCKER_DIR"

  if [[ "$NO_PUSH" -eq 1 ]]; then
    log "podman: multi-arch manifest kept locally as $manifest (not pushed)"
    # Also tag first local name for convenience
    local first="${ALL_TAGS[0]}"
    run podman tag "$manifest" "$first" 2>/dev/null || true
    return 0
  fi

  local t
  for t in "${ALL_TAGS[@]}"; do
    log "podman manifest push --all $manifest → $t"
    # docker:// prefix is optional on recent podman; use plain ref for hub tags
    run podman manifest push --all "$manifest" "docker://${t}"
  done
}

build_and_push() {
  [[ -n "$CTR" ]] || resolve_engine

  ALL_TAGS=()
  local hub
  if [[ "$NO_PUSH" -eq 0 ]]; then
    for hub in "${TO_REGISTRIES[@]}"; do
      login_hub "$hub"
      while IFS= read -r ref; do
        [[ -n "$ref" ]] && ALL_TAGS+=("$ref")
      done < <(image_refs_for_hub "$hub")
    done
  else
    for t in "${TAGS[@]}"; do
      ALL_TAGS+=("${IMAGE_NAME}:${t}")
    done
  fi

  [[ ${#ALL_TAGS[@]} -gt 0 ]] || die "no image tags to build"

  log "image tags:"
  local t
  for t in "${ALL_TAGS[@]}"; do
    echo "  - $t"
  done

  local tag_args=()
  for t in "${ALL_TAGS[@]}"; do
    tag_args+=(-t "$t")
  done

  local platforms="$PLATFORMS"
  if [[ "$LOAD_LOCAL" -eq 1 ]]; then
    platforms="linux/${HOST_DOCKER_ARCH}"
    log " --load requested: single platform $platforms"
  fi

  case "$CTR" in
    docker) build_with_docker "$platforms" "${tag_args[@]}" ;;
    podman) build_with_podman "$platforms" "${tag_args[@]}" ;;
    *) die "internal: unknown engine $CTR" ;;
  esac
}

# ── main ───────────────────────────────────────────────────────
log "content-sync image pipeline  version=$PKG_VERSION  image=$IMAGE_NAME"
log "tags: ${TAGS[*]}"
log "platforms: $PLATFORMS"
[[ ${#TO_REGISTRIES[@]} -gt 0 ]] && log "registries: ${TO_REGISTRIES[*]}"
[[ "$NO_PUSH" -eq 1 ]] && log "mode: build only (no push)"
[[ "$DRY_RUN" -eq 1 ]] && log "DRY RUN"

resolve_engine

if [[ "$SKIP_BINARY_BUILD" -eq 0 ]]; then
  build_binaries
else
  log "skip binary build (using $DIST_DIR)"
fi

stage_binaries
build_and_push

log "done"
if [[ "$NO_PUSH" -eq 0 ]]; then
  echo
  echo "Pull / run example:"
  for hub in "${TO_REGISTRIES[@]}"; do
    image_refs_for_hub "$hub" | head -1 | while read -r ref; do
      echo "  $CTR pull $ref"
      echo "  $CTR run --rm -p 8080:8080 -v content-sync-data:/data $ref"
    done
  done
else
  echo
  echo "Local run example:"
  echo "  $CTR run --rm -p 8080:8080 -v content-sync-data:/data ${IMAGE_NAME}:${TAGS[0]}"
  echo "  # config persists in volume /data  (= HOME, so ~/.content-sync → /data/.content-sync)"
fi

#!/usr/bin/env bash
# Multi-platform release builds for content-sync (portable / distro-independent where possible).
#
# Prefers (in order, per target):
#   1. host `cargo` for the native triple
#   2. `cargo-zigbuild` (+ zig) when installed
#   3. `cross` with podman or docker
#
# Always runs `cargo fmt` first. Smoke-tests binaries that can run on this host
# (`--help` / `--version`). macOS / other foreign targets: build only if tooling
# allows; runtime test is skipped when the binary cannot execute here.
#
# Usage:
#   scripts/build-release-multi.sh              # default target set
#   scripts/build-release-multi.sh --list       # print targets + selected builders
#   scripts/build-release-multi.sh --only x86_64-unknown-linux-musl,aarch64-unknown-linux-musl
#   scripts/build-release-multi.sh --skip-test
#   OUT_DIR=dist/release scripts/build-release-multi.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN_NAME="content-sync"
PKG_VERSION="$(
  grep -E '^version\s*=' "$ROOT/Cargo.toml" | head -1 \
    | sed -E 's/^version\s*=\s*"([^"]+)".*/\1/'
)"
PKG_VERSION="${PKG_VERSION:-0.0.0}"

OUT_DIR="${OUT_DIR:-$ROOT/dist}"
SKIP_TEST=0
LIST_ONLY=0
ONLY_TARGETS=""

# Default portable-oriented matrix
DEFAULT_TARGETS=(
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-gnu
  aarch64-unknown-linux-musl
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-pc-windows-gnu
  aarch64-pc-windows-gnullvm
)

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \?//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage 0 ;;
    --list) LIST_ONLY=1; shift ;;
    --skip-test) SKIP_TEST=1; shift ;;
    --only)
      ONLY_TARGETS="${2:-}"
      shift 2
      ;;
    --only=*)
      ONLY_TARGETS="${1#--only=}"
      shift
      ;;
    --out-dir)
      OUT_DIR="${2:-}"
      shift 2
      ;;
    --out-dir=*)
      OUT_DIR="${1#--out-dir=}"
      shift
      ;;
    *)
      echo "unknown arg: $1" >&2
      usage 1
      ;;
  esac
done

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'warn: %s\n' "$*" >&2; }
err()  { printf 'error: %s\n' "$*" >&2; }

have() { command -v "$1" >/dev/null 2>&1; }

HOST_TRIPLE="$(rustc -vV | awk -F': ' '/^host:/{print $2}')"
HOST_OS="$(uname -s)"
HOST_ARCH="$(uname -m)"

# Prefer podman for cross when both exist (lighter on many hosts).
if have podman; then
  export CROSS_CONTAINER_ENGINE="${CROSS_CONTAINER_ENGINE:-podman}"
elif have docker; then
  export CROSS_CONTAINER_ENGINE="${CROSS_CONTAINER_ENGINE:-docker}"
fi

HAS_ZIGBUILD=0
if have cargo-zigbuild && have zig; then
  HAS_ZIGBUILD=1
fi

HAS_CROSS=0
if have cross && { have podman || have docker; }; then
  HAS_CROSS=1
fi

# Targets zigbuild commonly handles well
zigbuild_ok() {
  case "$1" in
    *-unknown-linux-gnu|*-unknown-linux-musl|*-apple-darwin|*-pc-windows-gnu|*-windows-gnullvm)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# Targets cross-rs images typically cover
cross_ok() {
  case "$1" in
    x86_64-unknown-linux-gnu|x86_64-unknown-linux-musl|\
    aarch64-unknown-linux-gnu|aarch64-unknown-linux-musl|\
    armv7-unknown-linux-gnueabihf|armv7-unknown-linux-musleabihf|\
    x86_64-pc-windows-gnu|i686-unknown-linux-gnu|i686-unknown-linux-musl)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

ensure_rustup_target() {
  local t="$1"
  if [[ "$t" == "$HOST_TRIPLE" ]]; then
    return 0
  fi
  if ! have rustup; then
    return 0
  fi
  if rustup target list --installed 2>/dev/null | grep -qx "$t"; then
    return 0
  fi
  log "rustup target add $t"
  rustup target add "$t" || warn "rustup target add $t failed (builder may still work)"
}

# Print: cargo | zigbuild | cross | skip:<reason>
select_builder() {
  local t="$1"
  if [[ "$t" == "$HOST_TRIPLE" ]]; then
    echo cargo
    return
  fi
  if [[ "$HAS_ZIGBUILD" -eq 1 ]] && zigbuild_ok "$t"; then
    echo zigbuild
    return
  fi
  if [[ "$HAS_CROSS" -eq 1 ]] && cross_ok "$t"; then
    echo cross
    return
  fi
  if [[ "$HAS_ZIGBUILD" -eq 0 ]] && zigbuild_ok "$t" && [[ "$t" == *apple-darwin* || "$t" == *windows* ]]; then
    echo "skip:need cargo-zigbuild+zig (or macOS/Windows host)"
    return
  fi
  if [[ "$HAS_CROSS" -eq 0 ]] && cross_ok "$t"; then
    echo "skip:need cross + podman/docker"
    return
  fi
  echo "skip:no builder for $t on this host"
}

artifact_name() {
  local t="$1"
  local ext=""
  case "$t" in
    *windows*) ext=".exe" ;;
  esac
  echo "${BIN_NAME}-v${PKG_VERSION}-${t}${ext}"
}

built_binary_path() {
  local t="$1"
  local ext=""
  case "$t" in
    *windows*) ext=".exe" ;;
  esac
  echo "$ROOT/target/${t}/release/${BIN_NAME}${ext}"
}

# Can we exec this target's binary on the current host?
can_smoke_test() {
  local t="$1"
  case "$t" in
    *apple-darwin*|*windows*)
      return 1
      ;;
  esac
  # Same arch Linux only (no qemu required)
  case "$HOST_ARCH-$t" in
    x86_64-x86_64-unknown-linux-*|amd64-x86_64-unknown-linux-*)
      return 0
      ;;
    aarch64-aarch64-unknown-linux-*|arm64-aarch64-unknown-linux-*)
      return 0
      ;;
    *)
      # Optional qemu-user static
      if [[ "$t" == aarch64-unknown-linux-* ]] && have qemu-aarch64; then
        return 0
      fi
      if [[ "$t" == x86_64-unknown-linux-* ]] && have qemu-x86_64; then
        return 0
      fi
      return 1
      ;;
  esac
}

smoke_test() {
  local bin="$1"
  local t="$2"
  if [[ ! -x "$bin" && ! -f "$bin" ]]; then
    err "missing binary for smoke test: $bin"
    return 1
  fi
  chmod +x "$bin" 2>/dev/null || true
  log "smoke test ($t): --help / --version"
  if [[ "$t" == aarch64-unknown-linux-* && "$HOST_ARCH" != aarch64 && "$HOST_ARCH" != arm64 ]]; then
    qemu-aarch64 "$bin" --version
    qemu-aarch64 "$bin" --help >/dev/null
  elif [[ "$t" == x86_64-unknown-linux-* && "$HOST_ARCH" != x86_64 && "$HOST_ARCH" != amd64 ]]; then
    qemu-x86_64 "$bin" --version
    qemu-x86_64 "$bin" --help >/dev/null
  else
    "$bin" --version
    "$bin" --help >/dev/null
  fi
}

build_one() {
  local t="$1"
  local builder
  builder="$(select_builder "$t")"
  if [[ "$builder" == skip:* ]]; then
    warn "[$t] ${builder#skip:}"
    return 2
  fi

  ensure_rustup_target "$t"
  log "[$t] building with $builder"

  case "$builder" in
    cargo)
      cargo build --release --target "$t"
      ;;
    zigbuild)
      cargo zigbuild --release --target "$t"
      ;;
    cross)
      cross build --release --target "$t"
      ;;
    *)
      err "internal: unknown builder $builder"
      return 1
      ;;
  esac

  local src dest
  src="$(built_binary_path "$t")"
  if [[ ! -f "$src" ]]; then
    # native cargo without --target places binary in target/release/
    if [[ "$t" == "$HOST_TRIPLE" && -f "$ROOT/target/release/${BIN_NAME}" ]]; then
      src="$ROOT/target/release/${BIN_NAME}"
    elif [[ "$t" == "$HOST_TRIPLE" && -f "$ROOT/target/release/${BIN_NAME}.exe" ]]; then
      src="$ROOT/target/release/${BIN_NAME}.exe"
    else
      err "[$t] binary not found at $src"
      return 1
    fi
  fi

  mkdir -p "$OUT_DIR"
  dest="$OUT_DIR/$(artifact_name "$t")"
  cp -f "$src" "$dest"
  chmod +x "$dest" 2>/dev/null || true
  log "[$t] → $dest ($(du -h "$dest" | awk '{print $1}'))"

  if [[ "$SKIP_TEST" -eq 0 ]] && can_smoke_test "$t"; then
    smoke_test "$dest" "$t" || {
      err "[$t] smoke test failed"
      return 1
    }
  elif [[ "$SKIP_TEST" -eq 0 ]]; then
    warn "[$t] smoke test skipped (cannot run on $HOST_OS/$HOST_ARCH)"
  fi
  return 0
}

# ── Target list ────────────────────────────────────────────────
TARGETS=()
if [[ -n "$ONLY_TARGETS" ]]; then
  IFS=',' read -r -a TARGETS <<< "$ONLY_TARGETS"
else
  TARGETS=("${DEFAULT_TARGETS[@]}")
fi

# Trim whitespace
for i in "${!TARGETS[@]}"; do
  TARGETS[$i]="$(echo "${TARGETS[$i]}" | xargs)"
done

log "host: $HOST_TRIPLE ($HOST_OS $HOST_ARCH)"
log "version: $PKG_VERSION"
log "out: $OUT_DIR"
log "tools: cargo-zigbuild=$([[ $HAS_ZIGBUILD -eq 1 ]] && echo yes || echo no) cross=$([[ $HAS_CROSS -eq 1 ]] && echo yes || echo no) engine=${CROSS_CONTAINER_ENGINE:-none}"

if [[ "$LIST_ONLY" -eq 1 ]]; then
  printf '\n%-40s %s\n' "TARGET" "BUILDER"
  printf '%-40s %s\n' "----------------------------------------" "--------------------"
  for t in "${TARGETS[@]}"; do
    printf '%-40s %s\n' "$t" "$(select_builder "$t")"
  done
  exit 0
fi

# ── Always fmt first ───────────────────────────────────────────
log "cargo fmt --all"
cargo fmt --all

# Optional quick host check (fail early on compile errors)
log "cargo check (host)"
cargo check --quiet

ok=0
fail=0
skip=0
BUILT=()
FAILED=()
SKIPPED=()

for t in "${TARGETS[@]}"; do
  [[ -z "$t" ]] && continue
  set +e
  build_one "$t"
  rc=$?
  set -e
  if [[ $rc -eq 0 ]]; then
    ok=$((ok + 1))
    BUILT+=("$t")
  elif [[ $rc -eq 2 ]]; then
    skip=$((skip + 1))
    SKIPPED+=("$t")
  else
    fail=$((fail + 1))
    FAILED+=("$t")
  fi
done

echo
log "summary: built=$ok skipped=$skip failed=$fail"
if [[ ${#BUILT[@]} -gt 0 ]]; then
  echo "  built:   ${BUILT[*]}"
fi
if [[ ${#SKIPPED[@]} -gt 0 ]]; then
  echo "  skipped: ${SKIPPED[*]}"
fi
if [[ ${#FAILED[@]} -gt 0 ]]; then
  echo "  failed:  ${FAILED[*]}"
fi

if [[ -d "$OUT_DIR" && "$ok" -gt 0 ]]; then
  echo
  log "artifacts in $OUT_DIR"
  ls -lh "$OUT_DIR" 2>/dev/null | sed 's/^/  /' || true
  # Checksums for release uploads
  if have sha256sum; then
    (
      cd "$OUT_DIR"
      sha256sum content-sync-v${PKG_VERSION}-* 2>/dev/null > "SHA256SUMS.txt" || true
    )
    if [[ -f "$OUT_DIR/SHA256SUMS.txt" ]]; then
      log "wrote $OUT_DIR/SHA256SUMS.txt"
    fi
  fi
fi

if [[ "$fail" -gt 0 ]]; then
  exit 1
fi
if [[ "$ok" -eq 0 ]]; then
  err "no targets were built"
  exit 1
fi

log "done"
exit 0

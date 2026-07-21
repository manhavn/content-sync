#!/usr/bin/env bash
# Format sources, then build a release binary for content-sync.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt"
cargo fmt --all

echo "==> cargo check (deny warnings)"
# Surface any remaining warnings after fmt
if ! cargo check 2>&1 | tee /tmp/content-sync-check.log; then
  exit 1
fi
if grep -q "^warning:" /tmp/content-sync-check.log; then
  echo "error: cargo check produced warnings; fix them before release build" >&2
  exit 1
fi

echo "==> cargo build --release"
cargo build --release

BIN="$ROOT/target/release/content-sync"
echo
echo "OK — release binary:"
ls -lh "$BIN"
echo "Run: $BIN --help"

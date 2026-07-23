#!/usr/bin/env bash
# Format sources, then build a release binary for content-sync.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT/scripts/build-release.sh"
BIN="$ROOT/target/release/content-sync"

$BIN quit
sudo cp $BIN "/usr/local/bin"

content-sync background --no-log --bind 0.0.0.0:18790


#!/usr/bin/env bash
set -euo pipefail

# cd to repo root (parent of this script's directory)
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build --release

PLATFORM="$(node -p 'process.platform')"
ARCH="$(node -p 'process.arch')"
BINARY="target/release/dsh-tui"
if [[ "$PLATFORM" == "win32" ]]; then
  BINARY="${BINARY}.exe"
fi

node scripts/package-native.mjs stage \
  --source "$BINARY" \
  --platform "$PLATFORM" \
  --arch "$ARCH" \
  --vendor-root npm/vendor

mkdir -p dist
(cd npm && npm pack --pack-destination ../dist)

echo "tgz written to: $(ls -1 dist/*.tgz | tail -n 1)"

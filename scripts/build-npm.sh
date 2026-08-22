#!/usr/bin/env bash
set -euo pipefail

# cd to repo root (parent of this script's directory)
cd "$(dirname "${BASH_SOURCE[0]}")/.."

TARGET_DIR="${DSH_TUI_CARGO_TARGET_DIR:-target}"
if [[ "$TARGET_DIR" != /* ]]; then
  TARGET_DIR="$PWD/$TARGET_DIR"
fi

./scripts/cargo-guard.sh build --release --locked
TARGET_DIR="$(CDPATH= cd -- "$TARGET_DIR" && pwd -P)"

PLATFORM="$(node -p 'process.platform')"
ARCH="$(node -p 'process.arch')"
BINARY="$TARGET_DIR/release/martty"
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

#!/usr/bin/env bash
set -euo pipefail

# cd to repo root (parent of this script's directory)
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build --release

PLAT="$(node -p 'process.platform + "-" + process.arch')"

mkdir -p "npm/vendor/$PLAT"
cp target/release/dsh-tui "npm/vendor/$PLAT/"
chmod +x "npm/vendor/$PLAT/dsh-tui"

mkdir -p dist
(cd npm && npm pack --pack-destination ../dist)

echo "tgz written to: $(ls -1 dist/*.tgz | tail -n 1)"

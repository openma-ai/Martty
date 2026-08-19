#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# 1. Build the npm tarball (cargo release build + npm pack -> dist/)
./scripts/build-npm.sh

# 2. Pick the highest-version tarball in dist/. Version sort (sort -V),
#    not mtime: a later-built older version must never regress the install.
shopt -s nullglob
tgz=(dist/openma-deepseek-harness-tui-*.tgz)
if (( ${#tgz[@]} == 0 )); then
  echo "no tarball in dist/ — did build-npm.sh succeed?" >&2
  exit 1
fi
LATEST="$(printf '%s\n' "${tgz[@]}" | sort -V | tail -n 1)"
echo "using tarball: $LATEST"

# 3. Install the freshly built plugin into the tui profile.
#    pnpm re-installs the tarball when its content changes (integrity check);
#    clearing node_modules + lockfile below is a belt-and-braces force step.
#    Deliberately NOT wiping the whole profile dir, so the user's
#    cordis.patch.yml layer and any other installed plugins survive.
rm -rf ~/.dsh/profiles/tui/node_modules ~/.dsh/profiles/tui/pnpm-lock.yaml
dsh plugin --profile tui add "$PWD/$LATEST"

echo "installed $LATEST into profile tui — restart the TUI or run /refresh to pick it up"

#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# 1. Build the npm tarball (cargo release build + npm pack -> dist/)
./scripts/build-npm.sh

# 2. Pick the newest tarball in dist/
LATEST="$(ls -1t dist/openma-deepseek-harness-tui-*.tgz | head -n 1)"
echo "using tarball: $LATEST"

# 3. Install the freshly built plugin into the tui profile.
#    pnpm re-installs the tarball when its content changes (integrity check);
#    clearing node_modules + lockfile below is a belt-and-braces force step.
#    Deliberately NOT wiping the whole profile dir, so the user's
#    cordis.patch.yml layer and any other installed plugins survive.
rm -rf ~/.dsh/profiles/tui/node_modules ~/.dsh/profiles/tui/pnpm-lock.yaml
dsh plugin --profile tui add "$PWD/$LATEST"

echo "installed $LATEST into profile tui"

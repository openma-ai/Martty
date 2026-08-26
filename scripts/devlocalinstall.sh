#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Target profile: first CLI arg, else DSH_TUI_PROFILE, else martty (the
# primary dev profile). Must match the `dsh --profile <name>` you run.
PROFILE="${1:-${DSH_TUI_PROFILE:-martty}}"

# 1. Build the Rust binary and stage native artifacts into npm/
./scripts/build-npm.sh

# 2. Produce the tarball under the name users install:
#    `dsh plugin --profile martty add martty@latest` pulls the aliased
#    `martty` package from npm, so the dev install must be the same shape:
#    dist/martty-*.tgz. Mirrors CI (package-npm.yml):
#    package-alias.mjs npm npm-martty martty && npm pack ./npm-martty
rm -rf npm-martty
node scripts/package-alias.mjs npm npm-martty martty
npm pack ./npm-martty --pack-destination dist

# 3. Pick the highest-version tarball in dist/. Version sort (sort -V),
#    not mtime: a later-built older version must never regress the install.
shopt -s nullglob
tgz=(dist/martty-*.tgz)
if (( ${#tgz[@]} == 0 )); then
  echo "no tarball in dist/ — did the alias+pack step succeed?" >&2
  exit 1
fi
LATEST="$(printf '%s\n' "${tgz[@]}" | sort -V | tail -n 1)"
echo "using tarball: $LATEST"

# 4. Install the freshly built plugin into the target profile.
#    pnpm re-installs the tarball when its content changes (integrity check);
#    clearing node_modules + lockfile below is a belt-and-braces force step.
#    Deliberately NOT wiping the whole profile dir, so the user's
#    cordis.patch.yml layer and any other installed plugins survive.
PROFILE_DIR="$HOME/.dsh/profiles/$PROFILE"
# Note: a missing profile dir is fine — `dsh plugin add` below creates it
# (with the @deepseek-ai/dsh-base bundle), matching the user flow
# `dsh plugin --profile martty add martty@latest` on a fresh machine.
# Drop the legacy-named TUI bundle (@openma/deepseek-harness-tui) if a
# previous install left it in the profile: `dsh plugin add` would otherwise
# append `martty` alongside it and mount the TUI twice.
if grep -q '@openma/deepseek-harness-tui' "$PROFILE_DIR/package.json"; then
  dsh plugin --profile "$PROFILE" remove '@openma/deepseek-harness-tui'
fi
rm -rf "$PROFILE_DIR/node_modules" "$PROFILE_DIR/pnpm-lock.yaml"
dsh plugin --profile "$PROFILE" add "$PWD/$LATEST"

echo "installed $LATEST into profile $PROFILE — restart the TUI or run /refresh to pick it up"

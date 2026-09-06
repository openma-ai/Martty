#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Target profile: first CLI arg, else DSH_TUI_PROFILE, else martty (the
# primary dev profile). Must match the `dsh --profile <name>` you run.
PROFILE="${1:-${DSH_TUI_PROFILE:-martty}}"

# Cargo profile for the local build: devlocal (debug codegen) by default —
# the fastest turnaround for build/test loops; release matches the shipped
# npm bundles when the debug binary is too slow. The names map 1:1 to the
# [profile.*] sections in Cargo.toml.
CARGO_PROFILE="${DSH_TUI_CARGO_PROFILE:-devlocal}"

# 1. Build first so the installed copy always reflects the current source.
# The target dir must match where cargo actually writes (CARGO_TARGET_DIR
# wins over the default) — otherwise an old binary from a previous default
# location could be installed and the change would look like it never
# landed.
TARGET_DIR="${DSH_TUI_CARGO_TARGET_DIR:-${CARGO_TARGET_DIR:-target}}"
if [ "$TARGET_DIR" = "target" ]; then
  TARGET_DIR="$(pwd)/target"
fi
BIN="${TARGET_DIR}/${CARGO_PROFILE}/martty"
echo "building $BIN (cargo build --profile $CARGO_PROFILE --locked)…" >&2
cargo build --profile "$CARGO_PROFILE" --locked

# 2. Resolve exactly the binary `dsh --profile $PROFILE` runs — no hardcoded
#    package names or home dirs:
#      * the profile dir is $DSH_HOME/profiles/<name> (dsh defaults DSH_HOME
#        to ~/.dsh)
#      * the TUI package is the bundle in the profile's package.json
#        `dsh.profile.bundles` whose `bin` maps martty/dsh-tui to a script
#      * bin/martty.js spawns that package's vendor/<platform>-<arch>/martty,
#        unless MARTTY_BIN/DSH_TUI_BIN points at an existing file (it wins)
RESOLVED_FILE="$(mktemp)"
trap 'rm -f "$RESOLVED_FILE"' EXIT
DSH_TUI_INSTALL_PROFILE="$PROFILE" node --input-type=module > "$RESOLVED_FILE" <<'EOF'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const profile = process.env.DSH_TUI_INSTALL_PROFILE
const profileDir = path.join(
  process.env.DSH_HOME || path.join(os.homedir(), '.dsh'),
  'profiles',
  profile,
)
const out = { profileDir, candidates: [] }

// Legacy fallback names for profiles that install the TUI without declaring
// it in dsh.profile.bundles.
const LEGACY = ['martty', '@openma/deepseek-harness-tui']

let bundles = null
try {
  bundles = JSON.parse(
    fs.readFileSync(path.join(profileDir, 'package.json'), 'utf8'),
  ).dsh?.profile?.bundles ?? null
} catch {
  // no profile manifest — fall through to the legacy names
}

const platformKey = `${process.platform}-${process.arch}`
const exe = process.platform === 'win32' ? 'martty.exe' : 'martty'
const names = [...new Set([...(bundles ?? []), ...LEGACY])]

for (const name of names) {
  const pkgDir = path.join(profileDir, 'node_modules', name)
  let pkg
  try {
    pkg = JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf8'))
  } catch {
    continue // bundle not installed
  }
  const bin = pkg.bin
  const binScript = typeof bin === 'string' ? bin : bin?.['martty'] ?? bin?.['dsh-tui']
  if (typeof binScript !== 'string' || !binScript.length) continue
  out.candidates.push({
    name,
    pkgDir,
    binScript: path.join(pkgDir, binScript),
    dest: path.join(pkgDir, 'vendor', platformKey, exe),
  })
}

if (out.candidates.length === 0) {
  out.error =
    `no TUI package (bin martty/dsh-tui) under ${path.join(profileDir, 'node_modules')}; ` +
    `install once first: dsh plugin --profile ${profile} add martty@latest`
} else if (out.candidates.length === 1) {
  out.pick = out.candidates[0]
} else {
  // Several bundles ship a martty bin (e.g. current + legacy alias): prefer
  // the one that already staged a vendor binary for this platform.
  const staged = out.candidates.filter((c) => fs.existsSync(c.dest))
  out.pick = staged.length === 1 ? staged[0] : out.candidates[0]
}

// bin/martty.js precedence: MARTTY_BIN/DSH_TUI_BIN override the packaged copy
// only when set to an existing file.
const envBin = process.env.MARTTY_BIN || process.env.DSH_TUI_BIN
if (typeof envBin === 'string' && envBin.length > 0) {
  out.envBin = envBin
  out.envOverrideActive = fs.existsSync(envBin)
}

process.stdout.write(JSON.stringify(out, null, 2))
EOF

field() { node -p "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'))$1" "$RESOLVED_FILE"; }

ERROR="$(field '.error || ""')"
if [[ -n "$ERROR" ]]; then
  echo "$ERROR" >&2
  exit 1
fi

DEST="$(field '.pick.dest')"
PKG_DIR="$(field '.pick.pkgDir')"
BUNDLE="$(field '.pick.name')"
if [[ "$(field '.envOverrideActive || false')" == "true" ]]; then
  echo "dsh --profile $PROFILE does not use the packaged binary: MARTTY_BIN/DSH_TUI_BIN is set to" >&2
  echo "  $(field '.envBin')" >&2
  echo "unset MARTTY_BIN DSH_TUI_BIN (or point them at $BIN) and rerun." >&2
  exit 1
fi

# 3. Replace the binary. Copy to a temp file and rename: cp onto a running
#    TUI fails with ETXTBSY, while rename(2) just swaps the directory entry.
#    -p keeps the source's build mtime on the installed file.
mkdir -p "$(dirname "$DEST")"
if stat -c '%w' / >/dev/null 2>&1; then
  stat_created() { stat -c '%w' "$1"; }
  stat_modified() { stat -c '%y' "$1"; }
else
  stat_created() { stat -f '%SB' "$1"; }
  stat_modified() { stat -f '%Sm' "$1"; }
fi
# sha256 availability differs between GNU coreutils (sha256sum) and BSD/macOS (shasum).
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
else
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
fi
if [[ -e "$DEST" ]]; then
  OLD_CREATED="$(stat_created "$DEST")"
  OLD_MODIFIED="$(stat_modified "$DEST")"
  OLD_SHA256="$(sha256 "$DEST")"
else
  OLD_CREATED="(absent)"
  OLD_MODIFIED="(absent)"
  OLD_SHA256="(absent)"
fi
TMP="$DEST.tmp.$$"
cp -p "$BIN" "$TMP"
chmod 755 "$TMP"
mv -f "$TMP" "$DEST"
NEW_CREATED="$(stat_created "$DEST")"
NEW_MODIFIED="$(stat_modified "$DEST")"
NEW_SHA256="$(sha256 "$DEST")"

echo "installed $BIN -> $DEST"
echo "profile: $PROFILE ($(field '.profileDir')) · bundle: $BUNDLE"
echo "old martty: sha256 $OLD_SHA256 · created $OLD_CREATED · modified $OLD_MODIFIED"
echo "new martty: sha256 $NEW_SHA256 · created $NEW_CREATED · modified $NEW_MODIFIED"
echo "restart the TUI or run /refresh to pick it up"

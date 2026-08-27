#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Target profile: first CLI arg, else DSH_TUI_PROFILE, else martty (the
# primary dev profile). Must match the `dsh --profile <name>` you run.
PROFILE="${1:-${DSH_TUI_PROFILE:-martty}}"

# 1. The release binary to install; built on demand when missing.
BIN="target/release/martty"
if [[ ! -x "$BIN" ]]; then
  echo "$BIN missing — building (cargo build --release --locked, lto is slow)…" >&2
  cargo build --release --locked
fi

# 2. Locate the TUI package installed in the profile: the aliased `martty`
#    package (current) or the legacy-named @openma/deepseek-harness-tui.
#    `dsh --profile <name>` runs the package's bin/martty.js, which spawns
#    vendor/<platform>-<arch>/martty — that vendored file is what we replace.
PROFILE_DIR="$HOME/.dsh/profiles/$PROFILE"
PKG_DIR=""
for candidate in "$PROFILE_DIR/node_modules/martty" \
                 "$PROFILE_DIR/node_modules/@openma/deepseek-harness-tui"; do
  if [[ -d "$candidate/vendor" ]]; then
    PKG_DIR="$candidate"
    break
  fi
done
if [[ -z "$PKG_DIR" ]]; then
  echo "no TUI package with a vendor/ dir under $PROFILE_DIR/node_modules" >&2
  echo "install once first: dsh plugin --profile $PROFILE add martty@latest" >&2
  exit 1
fi

# 3. Mirror bin/martty.js's platform key: process.platform + '-' + process.arch.
case "$(uname -s)" in
  Linux) platform="linux" ;;
  Darwin) platform="darwin" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64) arch="x64" ;;
  aarch64 | arm64) arch="arm64" ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac
DEST="$PKG_DIR/vendor/$platform-$arch/martty"
mkdir -p "$(dirname "$DEST")"

# stat flag sets differ between GNU and BSD; birth time is what both report
# for "created" (GNU prints '-' when the filesystem doesn't track it).
if stat -c '%w' / >/dev/null 2>&1; then
  stat_created() { stat -c '%w' "$1"; }
  stat_modified() { stat -c '%y' "$1"; }
else
  stat_created() { stat -f '%SB' "$1"; }
  stat_modified() { stat -f '%Sm' "$1"; }
fi

# 4. Replace the binary. Copy to a temp file and rename: cp onto a running
#    TUI fails with ETXTBSY, while rename(2) just swaps the directory entry.
#    -p keeps the source's build mtime on the installed file.
OLD_CREATED="$(stat_created "$DEST")"
OLD_MODIFIED="$(stat_modified "$DEST")"
TMP="$DEST.tmp.$$"
cp -p "$BIN" "$TMP"
chmod 755 "$TMP"
mv -f "$TMP" "$DEST"
NEW_CREATED="$(stat_created "$DEST")"
NEW_MODIFIED="$(stat_modified "$DEST")"

if [[ -n "${MARTTY_BIN:-}${DSH_TUI_BIN:-}" ]]; then
  echo "note: MARTTY_BIN/DSH_TUI_BIN is set — bin/martty.js prefers that override over this copy." >&2
fi
echo "installed $BIN -> $DEST (profile $PROFILE)"
echo "old file: created $OLD_CREATED · modified $OLD_MODIFIED"
echo "new file: created $NEW_CREATED · modified $NEW_MODIFIED"
echo "restart the TUI or run /refresh to pick it up"

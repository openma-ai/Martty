#!/usr/bin/env bash
set -euo pipefail

# Dev-local install: builds a debug painter (fast, no release build) and
# installs the plugin into a freshly wiped `tui` profile. Only cargo and npm
# themselves are used — no external build scripts.
#
# WARNING: the whole ~/.dsh/profiles/tui directory is deleted first, so any
# custom cordis.patch.yml overlays, other installed plugins, and the
# profile's session data are discarded. This is a dev scratch install.

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# 1. Build the native painter as a debug binary (locked to Cargo.lock).
cargo build --locked

# 2. Stage the binary into the npm bundle's vendor dir for this platform,
#    mirroring package-native's layout: npm/vendor/<platform>-<arch>/dsh-tui.
PLATFORM="$(uname -s)"
case "$PLATFORM" in
  Linux) PLATFORM=linux ;;
  Darwin) PLATFORM=darwin ;;
  MINGW*|MSYS*|CYGWIN*) PLATFORM=win32 ;;
  *) echo "unsupported platform: $PLATFORM" >&2; exit 1 ;;
esac
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

BIN="target/debug/dsh-tui"
if [[ "$PLATFORM" == "win32" ]]; then
  BIN="${BIN}.exe"
fi
VENDOR_DIR="npm/vendor/$PLATFORM-$ARCH"
mkdir -p "$VENDOR_DIR"
install -m 755 "$BIN" "$VENDOR_DIR/dsh-tui"
echo "staged debug painter at $VENDOR_DIR/dsh-tui"

# 3. Pack the npm bundle.
mkdir -p dist
TGZ_NAME="$(cd npm && npm pack --pack-destination ../dist 2>/dev/null | tail -n 1)"
TGZ="dist/$TGZ_NAME"
echo "using tarball: $TGZ"

# 4. Wipe the whole profile, then install from scratch.
rm -rf ~/.dsh/profiles/tui
dsh plugin --profile tui add "$PWD/$TGZ"

echo "installed $TGZ into a fresh profile tui — restart the TUI to pick it up"

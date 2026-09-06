#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
project_root=$(CDPATH= cd -- "$script_dir/.." && pwd -P)
target_input=${DSH_TUI_CARGO_TARGET_DIR:-"$project_root/target"}
cargo_bin=${DSH_TUI_CARGO_BIN:-cargo}
max_gib=${DSH_TUI_RUST_CACHE_MAX_GIB:-20}
min_free_gib=${DSH_TUI_RUST_DISK_MIN_GIB:-10}
allow_external=${DSH_TUI_CARGO_ALLOW_EXTERNAL_TARGET:-0}

case "$max_gib" in
  ''|*[!0-9]*)
    echo "DSH_TUI_RUST_CACHE_MAX_GIB must be a non-negative integer" >&2
    exit 2
    ;;
esac
case "$min_free_gib" in
  ''|*[!0-9]*)
    echo "DSH_TUI_RUST_DISK_MIN_GIB must be a non-negative integer" >&2
    exit 2
    ;;
esac

if [ -d "$target_input" ]; then
  target_dir=$(CDPATH= cd -- "$target_input" && pwd -P)
else
  target_parent=$(dirname -- "$target_input")
  target_name=$(basename -- "$target_input")
  if [ ! -d "$target_parent" ]; then
    echo "Cargo target parent does not exist: $target_parent" >&2
    exit 2
  fi
  target_parent=$(CDPATH= cd -- "$target_parent" && pwd -P)
  target_dir="$target_parent/$target_name"
fi

case "$target_dir" in
  /|"$project_root")
    echo "refusing unsafe Cargo target: $target_dir" >&2
    exit 2
    ;;
esac
case "$target_dir" in
  "$project_root"/*) ;;
  *)
    if [ "$allow_external" != 1 ]; then
      echo "refusing Cargo target outside this repository: $target_dir" >&2
      exit 2
    fi
    ;;
esac

size_kib=0
if [ -d "$target_dir" ]; then
  # Fall back to 0 on failure (permissions, racing deletion) so the guard
  # degrades to "skip the size check" instead of aborting the build with
  # an "Illegal number" test error.
  size_kib=$(du -sk "$target_dir" 2>/dev/null | awk '{print $1}')
  case "$size_kib" in
    ''|*[!0-9]*) size_kib=0 ;;
  esac
fi
available_kib=$(df -Pk "$project_root" | awk 'NR == 2 {print $4}')
case "$available_kib" in
  ''|*[!0-9]*) available_kib=0 ;;
esac
max_kib=$((max_gib * 1024 * 1024))
min_free_kib=$((min_free_gib * 1024 * 1024))

show_status() {
  echo "Cargo target: $target_dir"
  echo "Cache size: ${size_kib} KiB (limit ${max_gib} GiB)"
  echo "Disk available: ${available_kib} KiB (floor ${min_free_gib} GiB)"
}

clean_target() {
  echo "cargo-guard: pruning scoped target $target_dir" >&2
  (
    cd "$project_root"
    CARGO_TARGET_DIR="$target_dir" "$cargo_bin" clean
  )
  size_kib=0
}

command=${1:-}
case "$command" in
  status)
    show_status
    exit 0
    ;;
  prune)
    clean_target
    exit 0
    ;;
  ''|-h|--help|help)
    echo "usage: scripts/cargo-guard.sh status|prune|<cargo args...>"
    exit 0
    ;;
esac

if [ "$size_kib" -gt "$max_kib" ] \
  || { [ "$size_kib" -gt 0 ] && [ "$available_kib" -lt "$min_free_kib" ]; }; then
  show_status >&2
  clean_target
fi

cd "$project_root"
export CARGO_TARGET_DIR="$target_dir"
exec "$cargo_bin" "$@"

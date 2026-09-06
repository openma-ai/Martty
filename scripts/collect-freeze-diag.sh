#!/usr/bin/env bash
# Collect diagnostics when the martty TUI looks frozen (issue: freeze after
# tab switches). Run this in ANOTHER terminal while the TUI is still wedged,
# then share the file path the script prints at the end.
#
# mktemp, not a fixed /tmp path: a predictable world-writable location is a
# symlink-attack target on multi-user machines.
set -euo pipefail
out=$(mktemp /tmp/martty-freeze-diag.XXXXXX.txt)
: > "$out"

pids=$(pgrep -f 'vendor/[^ ]*/martty|target/(devlocal|debug|release)/martty' || true)
if [ -z "$pids" ]; then
  pids=$(pgrep -xo martty || true)
fi

{
  echo "== date: $(date -Is)"
  echo "== candidate martty processes:"
  if [ -z "$pids" ]; then
    echo "(none found — is the TUI actually a martty process?)"
  fi
  for pid in $pids; do
    echo
    echo "== pid $pid: $(tr '\0' ' ' < /proc/$pid/cmdline 2>/dev/null)"
    ps -o pid,ppid,stat,%cpu,%mem,etime -p "$pid" 2>/dev/null || true
    echo "-- wchan: $(cat /proc/$pid/wchan 2>/dev/null || true)"
    echo "-- threads: $(ls /proc/$pid/task 2>/dev/null | wc -l)"
    if command -v rust-gdb >/dev/null 2>&1; then
      echo "-- backtraces (rust-gdb):"
      # The process may vanish mid-collection; every step stays best-effort
      # (set -e must not abort the report halfway).
      timeout 30 rust-gdb -batch -p "$pid" \
        -ex 'set pagination off' -ex 'thread apply all bt' 2>/dev/null \
        | grep -v '^\[Thread debugging' | head -160 || true
    else
      echo "-- rust-gdb not found; install gdb for userspace backtraces"
    fi
  done
} >> "$out" 2>&1

echo "diagnostics written to $out"

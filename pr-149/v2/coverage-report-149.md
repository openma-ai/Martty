# Coverage report — PR #149 update (NO_COLOR regression fix + restore-path tests)

Tooling: `cargo llvm-cov` 0.9.1 — line pass on stable Rust (`coverage.lcov`),
branch pass on nightly 1.101 (`--branch`/`BRDA`, `coverage_branch.lcov`).
Base for the diff: PR head `d2119742`. Total tests: **856 passed, 0 failed**
(847 unit + 5 `tests/osc_bg.rs` PTY + 4 other integration). PTY tests spawn
the real instrumented `martty` binary, so their coverage counts toward
`src/main.rs`/`src/ui.rs`.

## Per-file totals (context, not the gate)

| File | Lines (stable lcov) | Branches (nightly `--branch`) |
|---|---|---|
| `src/ui.rs` | 84.80% (2845/3355) | 74.67% (678/908) |
| `src/main.rs` | 45.64% (413/905) | 56.15% (73/130) |

`main.rs` is mostly the interactive run loop — only PTY tests drive it.

## New/changed lines — 100% of executable lines and branches covered

### `src/main.rs` (new lines 397–425, 558–569, 590–596)

- **404–424 `DSH_TUI_DEMO_DOCK` hook** — sends the real
  `_dsh/cordis/tui/slots/update` notification to populate
  `conversation.composer.dock` (the stats row). `if` at 404 runs **both**
  directions (BR `[6,1,4,2]`: demo+dock on/off × set/unset); body executes
  4× (the three dock-enabled PTY tests + tmux test).
- **561–563 `DSH_TUI_TEST_PANIC` hook** — `if` runs both directions
  (BR `[1,5]`); `panic!` line 562 executes once (`panic_restores_terminal_bg`).
- **569 `CanvasBgSync::new(!ansi_color_disabled())` + 593 sync call** —
  execute on every spawned run; enabled and disabled states both observed.
- DA=0 records at 402–403, 422–424, 568, 572, 596 are **non-executable
  lines**: comments, `serde_json::json!`/`let _ = send(..)` macro-boundary
  closers. No counter can land there; not a gap.
- **571 `if frames.ready(..)`** is a *context* line (unchanged by this
  patch). Its not-ready arm shows 0 in these runs — pre-existing condition,
  not a changed line.

### `src/ui.rs` (new lines 4189–4241)

- **4199–4201 `ansi_color_disabled()`** — DA=8: unit test
  (`ansi_color_disabled_matches_crossterm_semantics`, set/unset/restore)
  + spawned binaries.
- **4216–4221 `CanvasBgSync::new`** — DA=7.
- **4226–4236 `CanvasBgSync::sync`** — DA=14/8/6; `if !enabled || last ==
  color` at **4231 runs all four arm combinations** (BR `[4,10,4,6]`):
  disabled → early return (NO_COLOR test), unchanged → early return
  (dedup), enabled+changed → `write_all` (OSC 11 tests). The
  `Color::Reset → OSC 111` arm is exercised by
  `canvas_bg_sync_emits_once_and_recovers_on_change`.
- **4189 `OSC_BG_RESET` const** — use site (`restore_terminal`) covered by
  every PTY shutdown test (SIGTERM, SIGINT, panic, tmux).
- DA=0 records at 4196–97, 4203, 4206, 4209–15, 4222–23 are **doc comments
  and item declarations** (struct fields, `impl`/`fn` signatures) —
  non-executable attribution artifacts, not gaps.

## What the tests prove end-to-end (`tests/osc_bg.rs`, real PTY)

- `osc11_emitted_and_osc111_restores_on_sigterm` — OSC 11 `#0f1115` on
  first frame with the **stats row populated** (`DSH_TUI_DEMO_DOCK`),
  asserts `"2 turns"` bytes render; OSC 111 emitted on SIGTERM shutdown.
- `restores_bg_on_sigint_exit` — same restore on the SIGINT path.
- `panic_restores_terminal_bg` — `DSH_TUI_TEST_PANIC` panics after the
  hook install; binary exits non-zero **and** emits OSC 111 (panic hook
  runs `restore_terminal`).
- `no_color_suppresses_osc_bg_sync` — `NO_COLOR=1` + dock: **no**
  `\x1b]11;` bytes anywhere in the stream; OSC 111 still emitted on exit
  (unconditional, idempotent restore).
- `renders_and_restores_inside_tmux` — runs inside a detached tmux server
  (100×30), stats row renders ("Standard mode" + dock text), SIGTERM
  exits cleanly, pane returns to shell.

Terminals that ignore OSC 11 need no code path — the sequence is ignored
by the emulator and behaviour matches pre-fix; the cell-painted bg is
unchanged either way. NO_COLOR + light theme now keeps the terminal's own
default fg/bg pair (regression fixed).

## Conclusion

Every executable line and every branch arm added or changed by this patch
is covered (100%). The only 0-count records are non-executable lines
(comments, declarations, macro boundaries) and one unchanged context
branch (`frames.ready`).

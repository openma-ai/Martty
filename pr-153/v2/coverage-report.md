# Coverage report — changed code of `devin/composer-drag-resize` (issue #147)

Toolchain: `cargo +nightly llvm-cov --locked --branch --json` (llvm-cov branch coverage,
nightly-2026-10-09 rustc 1.101.0-nightly). Scope: every `+` line of the diff vs main
(`4f85c69`) in the four touched source files; test files excluded (they are the
measurement, not the measured).

| File | changed lines | uncovered lines | branch outcomes in changed ranges | uncovered |
|------|---------------|-----------------|-----------------------------------|-----------|
| src/app.rs | 47 | 0 | 30 | 0 |
| src/input/keymap.rs | 2 | 0 | 4 | 0 |
| src/main.rs | 0 | 0 | 0 | 0 |
| src/ui.rs | 42 | 0 | 18 | 0 |

**Result: 0 uncovered changed lines, 0 uncovered changed branch outcomes — 100% line and branch coverage of the changed code.**

Test suite: `cargo test --locked` — 858 tests total (854 unit + 4 integration), all passing.

## Coverage obtained with these tests (added for the requirement)

- `ui::tests::composer_border_highlight_paints_while_dragging` — mid-drag frame paints the highlighted cap (covers the `composer_resize.is_some()` paint arm).
- `ui::tests::composer_border_redrag_to_the_same_height_is_a_noop` — zero-delta drag exercises the no-change short-circuit.
- `ui::tests::composer_chrome_survives_the_borderless_fallback` — sub-10-row frame takes the `cap_h == 0` branch (no drag handle) and keeps painting safely with a stale hover flag (`composer_box == None` arm).
- `ui::tests::composer_cap_layout_hides_the_jump_glyph_on_tiny_widths` — `layout_jump_btn` early return on caps narrower than glyph+margins.
- `ui::tests::composer_chrome_tolerates_a_missing_or_stray_jump_rect` — paint with absent / out-of-frame ↥ rect.
- `app::session_tabs_tests::composer_border_hit_checks_only_the_cap_row` — hit-test boundaries (left/right of the box, well rows, `None` box).
- `app::session_tabs_tests::composer_height_keys_dispatch_through_the_action_map` — `Action::ComposerGrow/Shrink` through `dispatch`, clamp + floor release.
- `app::session_tabs_tests::composer_resize_hover_short_circuits_under_overlays` — release/move while an elicitation or subagent view is active (the overlay guards' false edges).

## Source simplifications made for coverage honesty
- Drag arm now matches on `if let Some((grab_row, start_h)) = self.composer_resize` — removes the unreachable `let-else` guarded by `is_some()`.
- Cap-cell highlight indexes `b[(x, cap.y)]` directly — the recorded box is already clipped to the frame, so the `frame.contains`/`cell_mut` guards were unreachable.
- The redundant `rect.width >= JUMP_BTN_W && rect.height >= CAP_BTN_H` check dropped — `layout_jump_btn` only ever emits 2x1 rects or `None`.


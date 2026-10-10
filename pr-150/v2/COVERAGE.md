# Coverage report — PR #150 follow-up (composer footer hints)

- Repo: `openma-ai/Martty`, branch `devin/1791617170-composer-footer-no-key-hints`
- Base for this patch: PR head `884855c` ("ui: drop inline key hints from composer footer chips (#148)")
- New commit under test: `2ac5c1e` ("ui: keep only the running esc interrupt hint in composer dock (#148)")
- Toolchain: `cargo-llvm-cov 0.9.1` + `llvm-tools-preview` (rustc `-Cinstrument-coverage`), full suite via `cargo llvm-cov --locked`
- Suite result: **847 tests, 0 failures** (`cargo test --locked` and `cargo check --locked --tests` both clean)

## Scope

PR #150 changes two regions in `src/ui.rs`:

1. `status_title` (commit `884855c`, already in the PR): composer meta-row chips render values only — preset + permission (+ optional approval / plan) with no glued key hints.
2. `context_hints` (this patch, `2ac5c1e`): the dock's right-side contextual hint row is trimmed to a single affordance — `esc interrupt` while a turn is running. All other hints (`⏎ send`, `⏎ queue`, `ctrl+⏎ steer`, `edit queue`, slash/shell/modal hints) are removed. Modal flows also suppress the interrupt label since `esc` then closes the modal.

## Per-file summary — `src/ui.rs`

| Metric | Covered | Total | % |
|---|---|---|---|
| Lines | 2738 | 3191 | 85.8% |
| Functions | 178 | 196 | 90.8% |
| Branches | 0 | 0 | n/a (see note) |

`src/ui.rs` is a ~5000-line render file; the PR touches ~140 lines across the two functions below. Uncovered lines elsewhere in the file predate this PR and are out of scope.

## Changed lines — every one is covered

### `context_hints` (new code, lines 1943–1965) — 100% line coverage

| Line(s) | Content | Hits |
|---|---|---|
| 1943–1945 | `fn context_hints` / `theme` / `queue_delete_confirming()` | 500 |
| 1946 | `|| queue_selecting()` | 498 |
| 1947 | `|| slash_completion_open()` | 494 |
| 1948 | `|| queue_editing()` | 476 |
| 1949 | `if modal_open \|\| matches!(app.state, RunState::Idle)` | 500 |
| 1950 | `return Vec::new();` | 462 |
| 1951–1957 | `vec![` + `"esc"` span (bold) | 38 |
| 1959–1961 | `" interrupt"` label span (incl. zh `中断` via `tr`) | 38 |
| 1963 | `" · "` separator span | 38 |
| 1965 | `}` | 500 |

The `||`-chain operands show distinct hit counts (500 → 498 → 494 → 476), evidencing each short-circuit arm is exercised — delete-confirm short-circuits first, queue-editing last. The early return fires 462× (idle + modal paths); the running `esc interrupt` path fires 38×.

### `status_title` (PR head code, lines 1881–1934) — changed lines all covered

| Line(s) | Content | Hits | Notes |
|---|---|---|---|
| 1881–1886 | `fn` + `session_bound` guard | 469 / 10 | early-return arm hit 10× |
| 1892–1895 | preset chip push | 459 | |
| 1897–1903 | permission resolution (`or_else` sandbox, `unwrap_or_else` fallback) | 459 | all three feed paths exercised |
| 1904–1906 | zh renamed labels (`只读` / `工作区可写` / `完全访问`) | 8 | |
| 1907–1908 | zh `_` fallback → `permission_label` | 2 | **was 0 — gap closed** |
| 1911 | en `permission_label` | 451 | |
| 1913–1918 | permission chip push + warn arm | 457 / 2 | `warn_soft` arm (danger-full-access) hit |
| 1921–1925 | `if let Some(approval)` chip | 459 cond / 3 body | **was 0 — gap closed** |
| 1926–1931 | `if app.modes.plan` chip | 456 cond / 3 body | **was 0 — gap closed** |
| 1932–1934 | `Line::from(spans)` | 456 | |

## Gaps found and closed

First coverage pass found three uncovered regions adjacent to / inside the PR hunks:

- `approval` chip body (1922–1925): `modes.approval` was never `Some` in tests
- `plan` chip body (1928–1931): `modes.plan` never `true`
- zh `_` label fallback (1907–1908): zh tests only used the three renamed ids

All three closed by the new test `status_chips_render_optional_approval_plan_and_fallback_labels` (sets `approval`/`plan`, zh locale with a `custom-perm` id, and a `sandbox`-only mode). Context-hints behavior is covered by `meta_row_hints_only_advertise_the_running_interrupt` (idle drafts `/`,`!`, plain text, empty → empty; queued → empty; running → `esc interrupt`; zh → `esc 中断`) and `meta_row_hints_stay_quiet_while_queue_modals_are_open` (drives Enter → Alt+Up → Enter → Ctrl+d under `RunState::Running`, asserting hints stay empty while `queue_selecting`/`queue_editing`/`queue_delete_confirming` are open).

## Branch coverage note

Stable rustc `instrument-coverage` records line + region counters, not boolean BRDA pairs — the lcov emits `BRF:0`. Branch behavior is therefore evidenced per region: every conditional arm above has a distinct non-zero hit count (each `||` operand, both arms of the `modal_open`/Idle check, both arms of the `session_bound` guard, all three permission feed paths, zh match arms, approval/plan `if` bodies, warn arm). The only zero-hit line near the patch is `1975` — the vim `-- INSERT --` indicator inside `status_right`, outside this patch's diff hunk (diff covers new-side 1933–1967).

## Artifacts

- `coverage/coverage.lcov` — lcov data for the whole workspace (includes `src/ui.rs` record)
- `coverage/html/html/index.html` — browsable per-line report
- Patch: `0001-ui-keep-only-the-running-esc-interrupt-hint-in-compo.patch`

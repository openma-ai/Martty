# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Local `!` commands now share one shell for the lifetime of the TUI session,
  so working-directory changes, exported variables, and other shell state
  persist across invocations. Commands remain client-local and start in the
  configured workspace.
- Linux x64 packages now ship a statically linked musl binary instead of a
  binary tied to the Ubuntu 24.04 glibc version. Release CI verifies the ELF
  artifact has no dynamic program interpreter and runs it on Ubuntu 20.04
  before packaging it.
- Every picker (`/resume`, model, mode, theme, permission, subagent, auth,
  plugin) now scrolls when the rows overflow the terminal: the visible
  window follows the selection instead of clipping the tail out of reach,
  and an inline scrollbar appears in a gutter column inside the popup's
  right edge, leaving the rounded border intact. `page up`/`page down`
  jump a screenful and `home`/`end` pin to the first/last row; `↑`/`↓`
  keep wrapping. The selected row is now highlighted across its full width
  (soft chip background behind marker, label, meta and the row tail — the
  meta text steps up from caption gray on the highlighted row) instead of
  only the label column. The scrolling viewport now comes from
  `tui-widget-list` (the one new crate; it shares ratatui's
  `ratatui-core`/`ratatui-widgets` crates, so there is no duplicate widget
  tree).

### Changed

- `/keys` is no longer a hand-written text wall: the shortcut map is now
  derived from the keymap table (`src/input/keymap.rs`) and rendered as
  markdown through the transcript's markdown pipeline — one GFM
  box-drawing table per group (`send · interrupt`, `scroll · navigate`,
  `edit · readline`, `app shortcuts`, `mouse`) with `[empty]`/`[typing]`
  context tags for dual-use keys and platform-appropriate chord spellings
  (⌘/⌥ vs ctrl). Notices gained a markdown render path (`/keys` is the
  first consumer); tables truncate with an ellipsis on narrow terminals
  instead of breaking mid-line. Anti-drift tests assert every `Action` is
  documented and that every displayed chord resolves back to its action
  through `classify()`; previously undocumented bindings (`ctrl+q`,
  `shift+enter`, `shift+tab`, `ctrl+v`, …) are now listed. `/help` stays
  the command overview; `/keys` is the complete key map.
- `/resume` picker rows now lead with the session's human handle — the
  harness-generated `session/title` (falling back to the first real prompt)
  — instead of the full UUID; the meta column carries the 8-char id prefix,
  relative age and turn count, and the picker title shows how many sessions
  are listed. Label and id columns use fixed display widths (CJK-aware) so
  rows align vertically. ACP `session/list` rows are enriched with the same
  local summaries (turns, age, prompt) when the session log is on disk.
- Markdown rendering now runs on `tui-markdown` (pulldown-cmark) instead of a
  hand-rolled line scanner: proper CommonMark + GFM parsing (nested lists,
  reference links, footnotes, task lists, definition lists, math delimiters)
  with the DeepSeek palette mapped through a custom `StyleSheet`. The CJK /
  Latin two-tone body coloring, hanging list/quote indents, full-width rules,
  and box-drawing tables are kept, now with a `├─┼─┤` junction between every
  pair of table body rows; tables wider than the viewport now
  truncate with an ellipsis instead of re-wrapping cells, fenced code blocks
  render as framed boxes with a language label on the top edge, all code
  (blocks and inline spans) uses upright light gray on a lighter panel
  background, and image syntax renders as `[img] alt (url)`.

### Added

- Added an internal Creator Host overlay to the TUI package. It overlays
  `tui-plugin-development` onto the shipped `cordis` preset's standing scope
  without copying the preset, publishing another package, or using ACP for
  skill loading.

### Documentation

- Reworked the repository and npm README mastheads around the centered
  DeepSeek logo, product name, concise positioning, and a consistent badge row.

## [0.2.1] - 2026-08-14

### Fixed

- `dsh-tui --help` now names `ctrl+o` as the expand-output shortcut instead of
  the retired `ctrl+e` binding.

### Documentation

- Expanded the repository and npm READMEs to cover the current host-skill,
  multi-image, Markdown, input, run-state, and tool-view features.

[0.2.1]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.2.1

## [0.2.0] - 2026-08-14

### Added

- Host skills in the slash menu (plugin mode): the runner's new `tui/skills`
  lists user-invocable skills through the same lens as the Web gateway's
  `skill.list` (preset-scoped registry, `isUserInvocable` filter); entries
  merge after the builtins (builtins shadow a colliding name), picking one
  lands the literal `/name ` and enter ships the line as an ordinary prompt
  — dsh-tool-skill's pre-step boundary injects the body host-side. The
  catalog refreshes on `/new` and `/mode`.
- Multi-image staging (grok-style): paste/`/image`/`/clip` stage up to 8
  images as literal `[image n]` tokens living *inline in the draft text* —
  rendered as chips (no icon), edited like text: backspace/delete on a chip
  cuts the whole token and un-stages that image, kills/history-recall
  reconcile the tray to surviving tokens. Hovering a chip (or parking the
  cursor on one) pops a grok-style preview: kitty thumbnail plus
  name/dimensions/size/type. Send order = token order; tokens strip out of
  the caption. The slash-command popup is capped at 12 rows with a
  selection-following scroll window and a position indicator.
- macOS physical-modifier rescue (grok-build's CoreGraphics side channel):
  bare arrows/backspace arriving while ⌘/⌥ is held get their modifier
  restored, so `⌘←`-style chords work in every terminal, kitty protocol or
  not.
- `DSH_TUI_KEYDEBUG=1` echoes each delivered key event in the tip row.

### Changed

- Input handling decoupled into `src/input/` (CG probe · normalizer · pure
  `keymap` table · line editor) and `src/attachments.rs` (tray model + strip
  geometry), all unit-tested; `App::handle_key` is now a thin dispatcher.
- OS-aware bindings: `⌘←/⌘→/⌘↑/⌘↓/⌘⌫`, `⌥←/⌥→/⌥⌫` (macOS),
  `ctrl+←/→/⌫` (linux/windows), full readline set (`^a/^e/^b/^f/^k/^u/^w`),
  expand-all moved `^e`→`^o`, `^u/^d` scroll only on an empty draft, unbound
  chords no longer type their base letter.
- Run state (`● idle` / spinner + phase + elapsed + queue depth) moved out
  of the composer to the transcript's always-last line, hugging the newest
  message (hidden on the landing banner); the composer meta row keeps only
  the mode chips, which now render cached workspace facts immediately.
- Measured accent vocabulary over the neutral base (“minimal, not
  monotone”): green for success/liveness (idle dot, finished tools, clean
  shell exits), amber for attention (queued, warnings, danger permission),
  gray-blue `hint` for informational text — the tip banner body and skill
  entries in the slash menu. Red stays errors-only; DeepSeek blue stays
  brand/actions.
- `!` local-shell blocks render as terminal cards: status glyph + a
  `$ cmd` chip header, output on full-width `code_bg` rows behind a
  gray-blue gutter, `exit N` in the error accent. Stale “ctrl+e” expand
  hints now say ctrl+o.
- The model chip now shows a fresh `/model` pick immediately: precedence is
  explicit pick → last streamed model → configured default (the pick defers
  back to the stream once a turn realizes it; `/resume` trusts the replayed
  stream).
- Mode facts (agent preset · permission · approval · effort) cache per
  workspace in `<session_root>/dsh-tui-modes.json`: chips and pickers show
  the last-known values on launch, `/new` inherits them, live host events
  overwrite and re-save. `plan` never carries over.
- Composer reorganized: the draft sits on top and a single bottom meta row
  carries run state · agent preset · permission (with its `⇧⇥` cycle hint)
  · approval/plan · contextual shortcuts · model · reasoning effort. The
  static shortcut-hints row is gone; the contextual hints are a state
  machine over (run state × draft): `⏎ send`/`⏎ command`/`⏎ shell` when
  typing idle, `esc interrupt` while running, `⏎ queue · ^x send now · esc
  interrupt` while running with a draft, and `^. keys` (new ctrl+.
  binding → `/keys`) when idle and empty.
- The tip banner fuses its rounded cap with the text row (`╭ Tip · … ─╮`,
  title-in-border): rounded corners kept, one row total.
- Image prompts now send every staged image as content blocks of one
  prompt (`tui/attach-image` per image, then a single `session/prompt`).

[0.2.0]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.2.0

## [0.1.3] - 2026-08-14

### Fixed

- `dsh plugin add` no longer warns about missing `@deepseek-ai/*` peers. The
  runner imports host packages from the running `dsh` (`$DSH_HOME/profiles/node_modules`)
  instead of reinstalling them into the profile, and ships its own JSON-RPC
  transport because `dsh-sdk-protocol` is not on that fallback graph.

[0.1.3]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.3

## [0.1.2] - 2026-08-14

### Fixed

- Closing the TUI no longer crashes the host Node process with unhandled
  `write EPIPE` on the JSON-RPC extra fds. The runner now treats those pipe
  errors as non-fatal, matching the stock SDK client, and skips flushing a
  pipe the native binary already closed.

[0.1.2]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.2

## [0.1.1] - 2026-08-14

### Fixed

- The dsh profile runner is now ESM. A CommonJS runner that `require()`d the
  host's ESM packages raced Cordis's parallel `import()` and crashed Node 24
  with `ERR_REQUIRE_ESM_RACE_CONDITION` on `dsh --profile tui`.

[0.1.1]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.1

## [0.1.0] - 2026-08-14

### Added

- Terminal-native DeepSeek Harness client built with Rust and ratatui.
- dsh profile bundle and standalone SDK JSON-RPC runtime modes.
- Streaming reasoning, tool calls, subagent activity, usage, and durable session
  resume.
- Host model, agent preset, permission preset, and plan-mode controls.
- Dark and light themes, mouse selection, routed clipboard support, and the
  optional `/liang` pixel companion.
- Native npm binaries for macOS arm64, macOS x64, Linux x64, and Windows x64.
- Tag-driven npm Trusted Publishing with provenance and GitHub Releases.

### Changed

- Installation now uses the unqualified npm package name and resolves through
  the `latest` dist-tag.
- Project and npm documentation now lead with quick start, supported platforms,
  runtime-mode differences, and troubleshooting.

[0.1.0]: https://github.com/openma-ai/deepseek-harness-tui/releases/tag/v0.1.0

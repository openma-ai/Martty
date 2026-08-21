# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- New gallery palette pack `catppuccin` (dark from Catppuccin Mocha, the
  second token map from Catppuccin Frappe), sourced from
  terminalcolors.com/themes/catppuccin. It registers at Client boot as a
  sibling insert row and is switchable via `/theme`.
- New gallery palette pack `ayu` (dark from the Ayu dark variant, the second
  token map from Ayu Mirage), sourced from terminalcolors.com/themes/ayu.
  It registers at Client boot as a sibling insert row and is switchable via
  `/theme`.
- New gallery palette packs `everforest`, `gruvbox`, `iceberg`, `night-owl`,
  `one-half`, `seoul256` and `solarized` (each with complete dark and light
  variant maps), sourced from terminalcolors.com/themes. They register at
  Client boot as sibling insert rows and are switchable via `/theme`.
- New gallery palette pack `one` (dark from One Dark, light from One Light),
  sourced from terminalcolors.com/themes/one. It registers at Client boot
  as a sibling insert row and is switchable via `/theme`.
- UI Presets now compose multiple UI Plugin contributions behind one
  persistent `/ui` choice. Builtin `default` (Martty) and `deepseek` presets
  each fill independent `welcome.hero` and `welcome.info` slots; dynamic
  Client Plugins can register additional compositions through `tuiPresets`.
- Bare `/ui` now opens a native preset selector. The composer also reserves
  the empty stats footer row until session usage arrives, avoiding a layout
  jump after the first response.
- The composer is now one rounded box: the cap row (`Tip` / plugin input
  dock / `·` workspace title) and the entire input surface (input well +
  meta row) share a single rounded rectangle, so the input text is fully
  enclosed by the frame. The cap is exactly one row — the plugin input
  dock (PLAN summary) wins the row over the tip line whenever both could
  appear, so PLAN + TIP never stack. The meta row (agent / permission /
  model chips) rides the box's bottom border (`╰⚙ … ╯`) instead of an
  inner row, so the box costs no extra rows and the input well is taller.
  The running-state glow now replaces the box's left border. Short
  terminals (below the cap threshold) keep the previous borderless
  composer.
- The composer stats dock is back: `conversation.composer.dock` (the
  `Cache hit` / token / TTFT readout) rides below the composer box on
  terminals tall enough, fed by the restored built-in `stats-view`
  Client Plugin; the `/liang` pet command and its pixel companion (kitty
  sprites + half-block fallback) are back too, perched inside the box's
  bottom-right corner. The `/logo` command stays removed — the banner is
  the Martty lockup.

### Fixed

- Switching `/ui` presets no longer returns an active conversation to the
  Welcome screen; preset slot updates now change only the Welcome content.
- `/theme` completion now matches its Theme Plugin picker, including stopped
  plugins that can be restored. Dark/light mode remains on `Ctrl+T`, which now
  also works while the Theme picker is open.

### Changed

- `/plan-view` 的审阅界面重做：`items` 型 plan 现在以单条 markdown 任务
  列表节点渲染（`## Plan · n/m` 标题、`- [x]`/`- [ ]` 状态框、进行中
  加粗、取消/失败删除线、priority 后缀），走完整 markdown 管线；审阅
  面板在宽终端上可扩到 2/3 屏宽（原 84 列封顶），滚动与关闭语义不变。

- The composer meta row now leads each chip with a small dot (`· Standard
  · Workspace Write …`) instead of emoji glyphs (`⚙ ⛨ ⚖ ⌁`); color still
  carries the meaning (full access turns warn).
- `/help`, `/session`, and `/keys` now share one output style: each
  renders through the transcript's markdown pipeline as `## name` with
  single-column markdown lists: one `- label · value` item per fact or
  binding, grouped by `###` for the keys map.
- `/session` shows the full turn profile folded from the same ACP facts
  as `acpSessionStats`: turns, steps, LLM vs tool time, TTFT average,
  and output throughput, with token counts compacted to K/M.
- New `/status` command: a compact single-list readout of the run state
  plus ACP facts (agent connection, authenticate state, session binding,
  server banner) and the key session facts (model, effort, agent,
  permission, plan, tokens, turns/steps, LLM/tool time, TTFT average,
  throughput). Live runs render it through the built-in `status-view`
  Client Plugin overlay: run-state facts come from the new
  `acpSessionStatus` Client service, and every token/turn/step/timing
  figure comes from `acpSessionStats.current()` — the same snapshot the
  composer stats dock renders, so the two readouts can never drift apart.
  The Rust painter no longer reads its own transcript accumulators for
  `/status` (a lean fallback keeps demo/standalone runs working with
  run-state facts only); `/session` keeps the full runtime detail, and
  both show the reasoning effort when set.

### Fixed

- The bundled ACP runtime is now `0.4.17-beta.0`, where ACP profiles own
  their complete `agent-presets` and `cordis-host-runner` service closure.
  Permission switching and command execution therefore stay compatible
  across current dsh hosts without a TUI-side host monkeypatch.
- The plan review window (the ACP elicitation form the agent opens with
  `exit_plan_mode`) now renders the plan markdown through the full markdown
  pipeline instead of truncating raw source lines: headings lose their `#`,
  task-list checkboxes become `✓`/`○` status glyphs, tables and code blocks
  keep their layout. The detail pane scrolls with PageUp/PageDown/End and
  the mouse wheel, the stored offset is normalized to the visible content so
  scrolling back up works after `End` (previously it stayed pinned at the
  bottom), and the Approve / Keep planning answers stay pinned below the
  pane; the title bar hints the scroll keys when the plan overflows it.
- The plan review (`/plan-view`) task list no longer reads as raw markdown:
  `- [x]` / `- [ ]` checkboxes render as status glyphs (`✓` in the ok color
  for completed items, `○` in the caption color for open ones) through the
  full markdown pipeline — the same applies to task lists in agent replies.
  Continuation lines keep hanging under the shorter glyph prefix.
- The plugin view overlay scrolls with the mouse wheel (previously wheel
  silently scrolled the chat underneath), and arrow keys scroll even when
  the terminal reports them with modifier bits (kitty keyboard protocol).
  `End` jumps to the bottom and overscroll clamps to the last content row
  instead of showing blank space; the title bar now hints
  `↑↓/wheel scroll · esc close`.
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
  only the label column. The picker viewport now comes from ratatui's own
  `Table`/`TableState` (selection column, full-row highlight, and
  follow-window scrolling), replacing the separate `tui-widget-list`
  dependency.

### Changed

- The startup banner now shows the **Martty** lockup (figlet `ansi_shadow`
  block logo on wide terminals, degrading to the small figlet variant and
  then plain text) tinted with the brand belly tone, and the hero line
  shows the project URL `https://martty.sh` instead of the old whale +
  slogan. The TUI's block-ASCII whale art and its generator
  (`src/logo_data.rs`, `scripts/gen_logo.py`) were removed; the website's
  own whale visualization keeps its data.

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

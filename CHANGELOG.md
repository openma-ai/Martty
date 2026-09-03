# Changelog

All notable changes to this project are documented here. The project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `/agent`, `/effort`, `/model`, `/theme` and `/permission` pickers now
  open with the row that is actually in effect preselected and ✓-marked
  (issue #102): the effort picker follows the host-echoed effort (falling
  back to the model's advertised default), and the model picker follows
  the running model (explicit `/model` pick → last streamed model →
  configured default) instead of the config default. The inline
  argument-option lists typed next to `/model` and `/effort` behave the
  same way: they open on the effective model/effort, ✓-mark it, and list
  the effective model even when it is not in the host catalog.

- `⌕` user-prompt jump button on the composer cap row, between the project
  path and the `⛶` expand glyph (issue #103): hovering highlights it and
  clicking scrolls the chat to the newest user prompt; each further click
  walks one prompt back, and the oldest prompt wraps to the newest again.
  The last-jumped position is remembered in memory only (never persisted),
  so the walk resumes where the previous click stopped; the cap row tip
  shows the current position (`user prompt k/n · newest first`). Text and
  image prompts both count as jump targets. Right after a jump the jumped
  prompt's rows are highlighted for 5 seconds like a picker's selected row
  (chip background wash with the text in the brand tone) and then restore
  to the ordinary bubble look. Hovering either cap-row glyph pops a small
  tooltip card above it explaining the click (⛶ expand/collapse, ⌕ jump).


- `/resume [n|id]` — a bare number lists the `n` most recent durable
  sessions in the resume picker (`/resume 10` → latest 10). Without a
  number the default is 50 (`/resume` ≡ `/resume 50`); anything that is
  not a number stays an id prefix for a direct resume. The cap applies to
  both the ACP `session/list` results and the local JSONL listing, and
  survives the `session/list`-unavailable fallback.

- Chinese coverage for every client-owned user-visible string: status-bar
  tips, transcript notices (turn end, session/plan/policy/approval facts,
  subagent labels, tool preview chrome), elicitation form validation
  errors, auth notices (sign-in hints, terminal-auth failures), the
  write-outside permission ask, and the composer/subagent chrome. ACP and
  plugin payloads stay authored by their owner. `Locale::trf` fills `{}` 
  holes left-to-right for parameterized messages.

- `/close` closes the current session tab (issue #94): everything bound to
  the tab — transcript, composer draft, staged images, prompt queue, ACP
  asks, subagents — is discarded, and the ACP controller forgets the
  session's turn state so no queued follow-up is sent into the void.
  Local only: ACP has no session/close, so the server-side session
  survives and a running turn keeps settling (its result drops at the
  router); the session can be re-entered later with `/resume`. The last
  remaining tab cannot close. Closing a tab that is still awaiting its
  `session/new`·`resume` bind keeps the FIFO slot but marks it dead, so
  the late bind is forgotten instead of rebinding whatever tab is on
  screen.

- Everything the frame paints between the session tab strip and the
  composer stats dock is now bound to the viewed tab, not shared: the
  composer draft and its staged `[image n]` chips (plus the `@file`
  browser and the pinned/expanded composer height) park with their
  session like the queue already did, chat scroll position, the `/model`
  pick, the welcome banner, and the running-state meta row (elapsed
  timer + state note) survive a tab switch exactly as left, and a `!`
  shell command started on one session lands its result in that session's
  transcript even if the user switched tabs while it ran.

- `/session view` shows the session + runtime info popup, and
  `/session prev` / `/session next` switch to the previous/next session
  tab (issue #94) — the tab strip needs no mouse. Every route (click and
  command) shares one path that dismisses transient interactions and
  releases compositor overlays with their cancel events before parking
  the session. Overflowing tab bars show an overflow counter (`+N`).

### Fixed

- Code-review sweep (2026-09-03):

  - **Terminal output integrity** — `terminal/output` no longer corrupts
    multi-byte characters split across 4096-byte reads: partial UTF-8
    sequences are carried across chunks instead of being decoded per read
    into U+FFFD. Unknown terminal ids now return protocol errors for
    `output`/`wait`/`kill`/`release` instead of a fake success.
  - **Terminal Auth keystrokes** — the crossterm input reader parks during
    Terminal Auth (and stays parked until the auth subprocess exits), so
    the login child no longer loses keystrokes to the UI reader.
  - **Background tab errors** — a failed image prompt (no image support,
    spill failure, empty payload) now reports through the requesting
    session, so a parked tab's running badge can no longer stick forever
    while the error lands on the viewed tab. `PromptQueued` carries the
    session id and only settles that session.
  - **Persistent shell** — `!` commands run with stdin from `/dev/null` and
    a 120-second silence deadline: a command that used to eat the control
    stream (`!cat`) or hang silently now kills the shell and the next `!`
    restarts it, instead of wedging the single shell worker forever.
  - **Cross-tab bind state** — same-session `/resume` keeps a FIFO bind
    entry (a concurrent `/new` can no longer steal the bind and cross-wire
    tabs), and both resume paths release plugin overlays with their cancel
    events like a tab click does, instead of leaking them onto the new
    tab.
  - **Modal keys** — vim mode only intercepts keys when no modal is up:
    one Esc cancels a permission ask / picker / overlay while vim Insert
    is active, and normal-mode letters no longer land in a hidden composer
    under a modal.
  - **ACP turn lifecycle** — in-flight prompts carry a generation tag so a
    stale finish after `/close` + `/resume` can never clear the new turn's
    occupancy marker (no more double `session/prompt` per session), and a
    `session/cancel` racing an already-settling turn trusts the agent's
    stop reason instead of reporting a successful turn as interrupted.
  - **ACP robustness** — every request the command loop awaits gets a
    120s deadline (a hung agent can no longer freeze Interrupt/Shutdown);
    the queue/agents snapshots stay silent for agents that never
    negotiated `_dsh/cordis`; second and later sessions now update the
    mode catalog from their own `configOptions`; `file://` URIs are
    percent-encoded (spaces, `#`, `?`, CJK paths); and `terminal/create`
    asks through the permission overlay before running an agent-supplied
    command.
  - **Runtime writer deadlock** — the legacy transport's stdin writer is
    taken out of the shared slot while a write blocks, so a wedged child
    can no longer deadlock `kill()` or starve the frame reader; the
    reader's best-effort replies never block on the write path.
  - **Editing** — vim `O` lands the cursor on the new blank line above;
    explicit range kills and chip deletion ignore an active shift
    selection (an image chip can no longer survive as literal
    `[image n]` text with its attachment removed); the form editor
    deletes whole grapheme clusters and gives combining marks zero width;
    plain ctrl+c (not ctrl+shift+c) cancels elicitation forms.
  - **Transcript rendering** — `/clear` invalidates stale cell-index
    handles (shell results and steer echoes can no longer leak into a
    fresh transcript); 4+ backtick fences with literal ```` ``` ````
    content render as one frame; the composer input dock measures at its
    real width (pet inset included) so PLAN summaries are not truncated;
    collapsed shell output keeps the tail like tool output; the
    navigation rail trims its last section instead of overlapping the
    trailing actions; user-node and injected-context wrap budgets use
    display width; zero-width characters are consistent across the
    markdown paths; and the chat pane's selection snapshot is
    viewport-sized instead of cloning the whole scrollback every frame
    (long streaming sessions no longer allocate O(history) text per
    frame — hit-testing translates through the viewport anchor, and the
    render window borrows the assembled lines instead of copying them).
  - **npm packaging** — a corrupt `settings.json` no longer blocks boot
    (it is moved aside and settings restart fresh) and theme preferences
    are written atomically; `package-alias.mjs` skips `node_modules` and
    the lockfile, asserts source/alias version parity, and tells you how
    to replace a stale alias; `release.mjs` warns when a stale local
    `npm-martty` checkout would publish an old version; the painter exit
    path maps signals to 130/143/1 consistently, `MARTTY_BIN` falling
    back is announced, spawn failures print a diagnostic; shell scripts
    honor `CARGO_TARGET_DIR` in the local installer, tolerate `du`/`df`
    failures, clean stale `dist/*.tgz`, and use `mktemp` for diagnostics;
    the old-ACP install-matrix case skips when the registry is
    unreachable.

- The session tab strip now scrolls instead of cutting off: clicking the
  leftmost or rightmost visible tab switches to it **and** nudges the
  strip by one, so the adjacent tab appears — repeated edge clicks walk
  the mouse through every session tab in either direction. The window
  always keeps the live tab on screen: opening or switching to a session
  outside the window re-anchors the strip with it at the right edge, and
  once the tabs all fit again the strip snaps back to the head.

- The mouse wheel now scrolls an open picker dialog (the `/resume` session
  list, `/model`, `/mode`, …) instead of falling through to the chat
  behind it: one notch moves the selection exactly like one ↑/↓ press,
  clamping at the list ends. Permission asks floating above a picker take
  the wheel first, matching their keyboard priority.

- Picker dialogs own a stable viewport window now: ↑/↓ and the wheel
  sweep the highlight through the visible rows, and the window only
  scrolls once the selection leaves it — previously the window was
  re-pinned to the selection every frame, so keys after a wheel-scroll
  slid the whole list instead of moving the highlight, and the highlight
  stuck to a window edge instead of tracking the wheel. The scrollbar
  thumb also tracks the real scroll offset (rescaled onto the track), so
  it reaches the bottom at the deepest scroll instead of stalling a
  third of the way down.

- Background session subagents now receive their transcript updates and
  streamed deltas cleanly, rather than having non-root session facts dropped
  at the UI event router while parked.

- Permission and elicitation asks from subagents are routed to their owning
  session tab (badging the tab when parked) instead of defaulting to the
  live tab and cancelling any foreground ask already on screen.

- Closing a session (`/close`) now purges any auth-stalled prompts parked
  for that session, and prompt settlement always reports session idle state
  so a stall on one session never freezes queue dispatch on others.

- Local shell commands (`!cmd`) started on an unbound tab update their
  pending tracker when `SessionBound` arrives, ensuring command completion
  never hangs indefinitely.

- `list_sessions` now pre-sorts files by modified time before loading and
  parsing JSONL logs, bounding decompression work to the top 50 sessions.

- `/plan` (and host skill config actions that map onto ACP
  `session/set_config_option`) now addresses the **viewed** session: the
  command carries the session id, so toggling plan mode on an older tab
  no longer silently flips the most recently bound session's plan.

- Failures are now routed to the session they concern instead of always
  printing on the viewed tab: prompt/steer/config rejections and errors
  arrive as session-scoped events and land in the owning tab's own
  transcript (including parked tabs), while a parked session's
  `Send Now` settlement (`SteerSettled`) requeues into that session's
  FIFO even if the user switched away meanwhile.

- Auth stalls no longer misroute retries or lose payloads: a prompt that
  hits an auth error is parked **with its owning session** (a queue, so
  concurrent stalls on several sessions cannot overwrite each other), its
  tab settles like a finished turn, and the next successful sign-in (or
  any succeeding prompt) retries every stalled prompt into its own
  session — never into the fallback session. A stalled prompt whose tab
  was closed meanwhile is dropped instead of leaking into another
  conversation.

- Failed or auth-stalled `session/new`·`resume` requests no longer poison
  the bind FIFO: they emit a bind-failure event that drops the awaiting
  entry and tells its tab (which stays open), so the next successful bind
  lands on the tab that actually asked — including the startup race where
  the client's own `/new` overtakes the unrequested startup bind (the
  startup session now finds its original parked tab instead of hijacking
  the viewed one).

- An unbound tab's queued prompts are never burned: queue dispatch is
  gated on the session bind, so a rejection error cannot cascade-pop the
  whole FIFO through repeated unknown-session errors.

- `/resume` over ACP (`session/load`) no longer leaves the welcome banner
  covering the replayed transcript: the banner was only dismissed by the
  first prompt send (and by the local JSONL resume path), so resuming
  before ever sending a prompt hid every loaded message until the user
  typed and sent something. `resume_acp_session` (session/resume and the
  legacy session/load fallback) now dismisses the banner before the
  transcript streams in, like `resume_session` already did.

- Painter info popups (`/help`, `/keys`, `/session`, and the painter
  `/status` fallback) and the `/plugins` / `/cordis-plugins` inventory tree
  now follow their session across tabs like the ACP asks do: opening one,
  clicking another tab, and returning restores the popup exactly as left
  (scroll and tree selection included), instead of floating over the newly
  viewed session. Compositor-owned plugin overlays (the live `/status`
  view, plan views, select/slider modals) cannot ride along — they belong
  to the client Plugin runtime's single-overlay slot — so a tab switch
  cancels them with the same event Esc would send (the plugin releases its
  slot; shipped plugins have no cancel side effects). A plugin's later
  "overlay closed" ack no longer eats a painter popup restored by that
  switch.

- Session-bound ACP asks (permission and elicitation popups) now follow
  their session across tabs instead of floating over whatever session is on
  screen: an ask that arrives for a background session waits in that
  session's tab (marked with a `?` in the tab strip) and only shows when
  its own tab is viewed; switching away never cancels or answers it, and
  answering or Esc-ing it works from its own tab only. Two asks on
  different sessions no longer clobber each other (the displaced ask used
  to auto-cancel, silently denying the agent's tool call).

- The UI no longer freezes for minutes when an agent floods the connection
  with session updates — reproduced with dsh-acp's `session/load` replay of
  a long session, which re-emits every historical delta (a 10k-event log
  arrived as ~258k notifications / 1.4 GB and saturated the event loop.
  Two defenses: drained bursts of `session/update` notifications are now
  coalesced before application (adjacent text deltas of one message are
  concatenated; superseded bare `tool_call_update`s keep only the latest
  state — lossless for the final transcript), and the per-event
  queue/agents projection comparisons now use cheap fingerprints instead of
  full snapshot clones. The composer stays responsive while such a storm
  drains in the background.


- `/resume` now uses ACP `session/resume` when the agent advertises it, so long
  sessions can continue without replaying their full transcript into the TUI.
  Agents that only support the legacy `session/load` path keep working, and a
  rejected resume request falls back to load when that capability is available.

- `acpSessionStatus` now treats each standard ACP `session/prompt` response
  (success or error) as that request's terminal turn signal, returning to
  `idle` after every pending prompt and concurrent steer has settled. This
  keeps generic ACP V1 agents from leaving `/status` stuck on `starting` or
  `running` when they do not emit Martty's optional `session.status`
  extension; cancellation still waits for the original prompt response
  required by ACP V1 (issue #25).
- Prompt history now keeps browsing older/newer entries on repeated `↑`/`↓`
  presses after the first recall. In a non-empty draft, `↑` still moves by
  visual line until the cursor reaches the top, then stashes the draft and
  enters history; `↓` restores that draft after the newest history entry.
- macOS native-package staging now atomically replaces an existing executable,
  so repeated source-checkout builds do not leave a linked `martty` process
  killed by a stale Mach-O vnode signature cache.
- Slash-menu and `@file` black blocks (issue #83): scrolling the command
  candidate list (or the file browser) moved wide (CJK) glyphs and could
  orphan a trailing half-cell on the real screen while both ratatui
  buffers agreed the cell was unchanged, so the frame diff skipped it and
  the leftover half-glyph stayed as a black rectangle. Both popups now
  mark every cell for a full rewrite each frame, the same guard the
  transcript and review overlay already use (issue #38 class).
- `/agents` is now an on/off switch (issue #80): bare `/agents` toggles
  the Agent panel, `on`/`off` set it explicitly, and an agent id still
  selects that transcript. The panel also auto-closes once every subagent
  task has ended — a failed task keeps it visible, `/agents on` holds it
  open until the next batch starts, and the native no-Client-tree rail
  follows the same rule. Finishing the last task also clears an inline
  selection left open with ↓, which previously kept the expanded panel
  visible forever.
- Chinese command descriptions (issue #80): `vim`, `ui`,
  `cordis-plugins` and the built-in Client Plugin commands `agents`,
  `status`, `plan-view` now render in Chinese under `/lang zh`, and the
  `/plugins` description is corrected everywhere (it only shows Host
  plugin status — it cannot disable or re-enable plugins).
- Composer dock context gauge (issue #77): the percentage now uses the
  harness's `usage_update` readout (`used` = final prompt size, `size` =
  the real model window) instead of accumulating per-prompt usage over a
  model-name guess, which over-counted every step's full context and
  pegged at 100% early. Without an authoritative readout the gauge is
  hidden rather than guessed, and `stats-view` no longer depends on
  `acpSessionStatus`.
- Chinese locale gaps (issue #77): composer `ctrl+shift+a`/`shift+tab`
  hint labels (`steer`, `shell`), the run-state notes (starting runtime,
  cancelling, sending images/followups, queue paused) and the native
  agent rail (Agents/History/expand/close hints) now render in Chinese;
  the composer key hints also drop the brand accent and match the light
  tertiary tone of the stats dock readout.
- User-visible product strings say `martty` instead of the legacy
  `dsh-tui` name (quit/keys descriptions, welcome version row, auth
  hint); filesystem paths, settings file names, env vars and the
  `dsh-tui` compatibility alias are unchanged (issue #77).

- `/status` on runs without a Client tree (demo, standalone painter) now
  opens the same popup as `/keys`, `/help` and `/session` instead of pushing
  into the scrollback: directly after startup the transcript was hidden
  behind the welcome banner, so the command appeared to do nothing until a
  conversation dismissed the banner (issue #53).
- `ctrl+shift+k` (and vim `dd`) on the last line of the draft now
  removes the whole line — including its newline, so the line above
  becomes the last one — and lifts the cursor onto it; repeated presses
  keep deleting upward. Killing a middle line still merges the next
  line up (issue #63).
- Pasting multi-line text into the composer keeps its line structure
  instead of flattening every newline into a space; CRLF and CR-only
  line endings (iTerm2 et al.) are normalized to plain rows too
  (issue #54).
- IME candidate/composition popups (e.g. the first Chinese character typed
  into an empty composer) now anchor at the input caret again: the hidden
  hardware cursor is repositioned onto the painted caret cell after every
  frame instead of being left wherever the last diff write happened
  (issue #66).
- Table frames no longer show gaps in the vertical borders when a palette
  aliases `border` onto a body gray (Ayu: border == fg_secondary): the
  two-tone body pass no longer repaints box-drawing characters, so the
  hand-drawn `├─┼─┤` junction rows between table rows match the frame
  (issue #68).
- Scrolling the `/keys` popup or the chat transcript no longer leaves
  irregular black blocks on screen: orphan halves of wide (CJK) characters
  survived because the frame diff considered those cells unchanged; both
  scroll surfaces are now marked `CellDiffOption::AlwaysUpdate` so every
  drawn frame rewrites them (issue #38).
- Esc no longer freezes the TUI when the agent runtime closes its stdout but
  keeps running: the frame reader no longer holds the child lock across a
  blocking wait, so the hard interrupt can always SIGKILL it.
- A failed agent spawn (e.g. a missing `--agent` binary) no longer crashes
  the client process with an uncaught exception; the transport sees EOF and
  pending requests fail instead of hanging, and a later apply retries the
  spawn instead of reusing the dead child.
- Pressing space on an agent elicitation multi-select field with zero
  options no longer panics the TUI.
- Backspace/Delete with an active composer selection (shift+arrows, vim
  `x`/`X`) now delete the whole selection instead of a single character.
- `terminal/wait_for_exit` no longer hangs forever when the exit
  notification fires between the status check and the await (lost wakeup).
- Esc pressed with no in-flight turn no longer poisons the next prompt
  (whose result would be swallowed and misreported as interrupted).
- `--dump-frame` no longer swallows the following CLI flag when its
  optional `[WxH]` argument is omitted.
- Scrolling in the same event burst as jump-to-top no longer teleports the
  viewport from the top of the scrollback to just above the tail.
- Sending an image prompt on a transport without image support now reports
  an error and returns to idle instead of wedging in the Starting state
  with a queue that never drains.
- User message bubbles no longer overflow the chat pane by two cells, which
  clipped the last visible character of every wrapped CJK line.
- Review/info popups no longer repeat their title (issue #85): `/plan-view`
  moves the `Plan n/m` counter into the popup window title, and the
  in-body `## …` headings were dropped from `/plan-view`, `/keys`, `/help`,
  `/session` and `/status` — the popup border already names the window.


- The composer input now supports the mouse: left-click places the caret at
  the clicked character (blank space past the text snaps to the end), and
  left-drag selects a text span that is copied to the clipboard on release,
  with the highlight persisting until the next click or Esc. CJK/emoji
  double-width characters are handled cell-accurately: the caret splits a
  wide char at its midpoint and a drag covers it whole. Mouse actions never
  trigger submit, history recall or slash-completion selection.
- `Ctrl+U`, `Ctrl+K`, and the corresponding macOS line-kill shortcut now
  delete only to the current rendered line boundary instead of truncating the
  entire multiline composer draft.
- The composer caret no longer flickers while the transcript scrolls (agent
  streaming auto-follow, or scrolling yourself). The caret is now painted as
  a buffer cell — a steady reversed block that rides the frame diff — and
  the terminal's hardware cursor stays hidden for the whole session, instead
  of being hidden for the entire frame write (long during scroll redraws)
  and re-shown at frame end. Elicitation form fields use the same cell
  caret; the old hardware-cursor position and the redundant `▏` insert mark
  are gone. Frames still apply as one synchronized update (DECSET 2026).
- Image thumbnails and the pixel pet no longer re-send their whole PNG to
  the terminal on every move: each kitty image is transmitted once under its
  image id, and viewport scrolls (thumbnails) or composer-height changes and
  idle↔working toggles (pet) re-place it with a small `a=p` placement
  command, dropping the previous placement only after the new one exists so
  nothing blinks out mid-scroll. The pet's ~43KB sprite retransmit on every
  long-draft wrap boundary is gone. All kitty writers now share one set of
  protocol primitives, and the pet's screen anchor is recorded by the frame
  draw itself instead of being recomputed in main.

### Added

- Multi-session parallel management (issue #94): one ACP connection now
  drives several sessions concurrently. Once a second session exists, a
  native tab strip appears at the top — one tab per session with a running
  indicator (spinner on the viewed tab, `●` in the background) and a `✓`
  completion badge for sessions that finished while in the background;
  left-click a tab to switch. `/new` and `/resume` now park the current
  session instead of discarding it: switching back restores its transcript,
  prompt queue, modes, and subagent panels exactly as left, while background
  sessions keep folding their `session/update` events into their own
  transcripts (the event router also no longer lets foreign-session events
  leak into the viewed transcript). Prompts, steers, interrupts, and
  per-session queues are addressed by session id, so turns on different
  sessions run truly concurrently.
- `scripts/acp-multi-session.smoke.mjs`: opt-in smoke check that the
  spawned dsh-acp agent drives concurrent sessions over one ACP connection
  (consumes a small amount of model tokens; not part of `npm test`).

- Fast local build profile `devlocal` (debug codegen, incremental) for
  `scripts/devlocalinstall.sh`: the local install/build loop no longer
  pays the fat-LTO release build (`DSH_TUI_CARGO_PROFILE=release`
  restores the shipped config). The npm bundles keep the fat-LTO release
  config untouched.

- Standalone harness registry and discovery: `martty harness list` shows saved
  entries, the bundled DSH runtime, and executable `*-acp` / `*_acp` commands
  on `PATH`; `harness add` saves a named command and repeated arguments, while
  `harness use` persists the active entry in `$MARTTY_HOME/settings.json`.
  The built-in Client Plugin adds `/harness` and `/harness <id>` as the third
  entry point, using the native TUI single-select overlay and the same registry.
  Standalone startup now resolves `--agent` → `DSH_TUI_AGENT` → the saved
  `activeHarness` → the bundled default. Selection applies on the next launch,
  which creates a fresh ACP session; sessions never carry across Harnesses and
  profile-owned Host runtimes and sessions are unchanged.
- The `@file` mention browser now follows the typed query across the
  tree: the listing is live-filtered to matching names (exact > prefix >
  contains > subsequence, `../` always kept so a filtered view can back
  out), and when the filter leaves nothing in the current directory a
  bounded search (max 3 levels, skipping `.git`/`node_modules`/`target`-
  style trees) hops the browser to the closest entry and selects it —
  typing `@abc` jumps to `docs/abc_ref.md` without a path prefix, and
  continuing to type keeps the browser there. An empty match leaves the
  frame up with a no-match hint instead of hiding the menu.
- A mouse-only expand affordance now sits on the composer card's
  top-right frame border (issue #92): an always-visible `⛶` glyph that
  highlights on hover and pins the input to the amplified height (about
  5/8 of the main area) on click; clicking again restores the
  draft-following auto height. The glyph lives on the cap row, outside
  the text well, so a full draft never hides it. It has no key binding
  and never moves the caret.
- New gallery palette pack `tomorrow` (dark from Tomorrow Night
  Bright, light from Tomorrow), sourced from
  terminalcolors.com/themes/tomorrow. It registers at Client boot as a
  sibling `inject = ['tuiTheme']` row and `/theme tomorrow` covers it.
  Both terminalcolors variants export bright black as `#000000`
  (equal to the dark background, darker than the light foreground), so
  secondary text/border take the Tomorrow family's canonical comment
  tones (`#969896` dark / `#8e908c` light) and panel/chip take the
  selection background (`#424242` / `#d6d6d6`), following the
  solarized/everforest readability precedent.
- New gallery palette pack `one` (dark from One Dark, light from One
  Light), sourced from terminalcolors.com/themes/one. It registers at
  Client boot as a sibling `inject = ['tuiTheme']` row and `/theme one`
  covers it; dark/light follows the existing packs. Three adaptations
  in light mode, all sourced from the One family's canonical syntax
  tones because One Light's terminal export lacks them: secondary
  text/border take the comment gray (`#5c6370`) instead of collapsing
  onto `panel`'s `#bbbbbb` (which made picker dialog text unreadable),
  `warn` takes the orange constant tone (`#c18401`) instead of the
  export's pale tan (invisible as the approval dialog border on the
  panel), and `hint` takes the aqua tone (`#0184bc`) because the export
  maps cyan and green to the same swatch, colliding with `ok`. One
  Dark's inverted selection also makes the user bubble light-on-dark in
  dark mode and dark-on-light in light mode, straight from the source
  selection swatches.
- `@file` mentions in the composer (issue #62): typing `@` at a word
  boundary opens a workspace file browser (ratatui-explorer) above the
  input. The query's directory prefix descends as you type (`@src/m` opens
  `src/` and jumps to the first `ma*` entry), `↑/↓` move, `Tab`/`→` drill
  into a directory (rewriting the token to `@dir/`), `←` goes up, `Enter`
  picks (`@path`, `@"path with spaces"` quoted automatically, directories
  keep their trailing `/`), `Esc` dismisses the token until its text
  changes, and `ctrl+h` toggles hidden files and directories. Emails and
  URL hosts never trigger; the `@"…"` form allows
  spaces and keeps the quote open while drilling. The browser colors from
  the active theme tokens (panel/border/fg/brand/chip_bg), so palette
  packs and both UI presets apply.


- The light/dark theme mode (`ctrl+t`, `/theme toggle`) now persists to
  `settings.json` (`themeMode`) and is restored on the next launch; an
  explicit `--theme <dark|light>` flag still overrides it for that run
  (issue #63).
- A gallery palette saved via `/theme <id>` (ayu, iceberg, …) is
  re-activated on the next launch: the static gallery packs register with
  no owner, so the boot restore pass now activates the saved pick after
  they are all registered (dynamic packs were already restored by
  `tui-local-plugins`) (issue #63).
- Martty prints the live session id on stderr (`session <id>`) when the
  TUI exits, so a later `--session-id <id>` can resume the same
  conversation (issue #63).
- The dock bar no longer advertises `/keys` in the shortcut hints — the
  keymap lives in the tip banner and the `/keys` modal itself, and the
  empty state now stays quiet (issue #63).
- `/plugins` now renders the static Host plugin inventory as a two-level
  tree grouped by provider (npm scope) → plugin name, using the
  tui-tree-widget crate: every provider branch opens expanded by default,
  ↑/↓ move the selection, ←/→ or enter fold/unfold a branch, pgup/pgdn,
  home/end and the mouse wheel scroll, esc closes. Leaves keep the
  enabled/fiber-phase meta of the old flat picker (issue #49).

### Changed

- Transcript rendering now caches each cell's body lines and re-renders only
  the cells that actually changed (streaming deltas, tool results, width,
  theme or expansion drift). Spinner ticks and unchanged frames no longer
  re-parse the whole scrollback: measured ~12.6x faster idle frames and
  ~4.3x faster streaming frames on a seven-message markdown transcript,
  with the gap growing as the conversation grows.
- `/help` and `/session` now open the same popup dialog as `/keys` instead
  of pushing markdown into the scrollback; the transcript stays untouched
  (issue #49).
- Popup dialog bodies (keys/plan-view/status and friends) are indented one
  level from the border, and markdown wraps one level narrower so
  continuation lines stay aligned (issue #49).

## [0.2.24] - 2026-08-24

### Fixed

- TUI Client process now converts SIGTERM/SIGINT/SIGHUP into an orderly
  `process.exit`, so the Rust painter is always torn down (it is killed from
  the Client's `exit` hook). Previously the Host runner's SIGTERM terminated
  the Client without running `exit` listeners, orphaning the painter holding
  the TTY — shutdown appeared to hang until the terminal was closed.


### Added

- New gallery palette pack `kanagawa` (dark from Kanagawa Wave, light from
  Kanagawa Lotus), sourced from terminalcolors.com/themes/kanagawa. It
  registers at Client boot as a sibling insert row and is switchable via
  `/theme`.
- New gallery palette pack `catppuccin` (dark from Catppuccin Mocha, light
  from Catppuccin Latte), sourced from terminalcolors.com/themes/catppuccin.
  It registers at Client boot as a sibling insert row and is switchable via
  `/theme`.
- New gallery palette pack `ayu` (dark from Ayu, light from Ayu Light),
  sourced from terminalcolors.com/themes/ayu. It registers at Client boot
  as a sibling insert row and is switchable via `/theme`.
- New gallery palette packs `everforest`, `iceberg` and `solarized` (each with complete dark and light
  variant maps), sourced from terminalcolors.com/themes. They register at
  Client boot as sibling insert rows and are switchable via `/theme`.
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

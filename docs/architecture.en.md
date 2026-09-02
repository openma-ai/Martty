# Architecture

The ACP client is a Cordis plugin: it provides `ctx.acpClient`, speaks ACP, and **does not bundle or import any harness**. The TUI is a shell on this **client** tree (Rust owns the TTY). TUI plugins such as palettes `inject` client-side services, not the Martty package name and not dsh-acp.

The primary `dsh --profile martty` path starts two processes. The Host process mounts the complete ACP plugin on its Base Cordis tree, then the Host runner starts a separate TUI Client process. They speak ACP only through the TUI Client's standard stdin/stdout; no Cordis services, plugin ids, `inject`, or fibers cross the process boundary. The Client process receives the user TTY on fd 3/4 and maps it to the Rust painter's stdin/stdout. Host↔Client ACP never uses fd 3/4.

The standalone `martty` entry remains a general ACP client: its Client tree may spawn `dsh-acp`, `dsh --profile acp`, or another ACP agent, or attach a caller-owned stream.

## ACP client: root plugin or attached plugin

One `apply`, two composition seats:

| | Standalone root | Attached to an existing tree |
|---|---|---|
| Seat | First row of the client composition; process root | One `insert` in someone else's client tree |
| Provides | `ctx.acpClient` (and client services it boots) | The same `ctx.acpClient` |
| Agent | `config.agent.command` spawn (default may be `dsh-acp`) or `config.stream` on standard pipes | The profile runner attaches `config.stream` to cross-process stdio; standalone may spawn |
| Must not | `ctx.plugin` dsh-base | Share a process with the ACP Host plugin or replace ACP with an in-process stream |

Attaching to dsh is agent config, e.g. `{ command: "dsh-acp" }` or `{ command: "dsh", args: ["--profile", "acp"] }`. Another ACP agent is a different command.

## Negotiated DSH Cordis ACP extensions

The Server tree (Base + ACP plugin in the profile path, or standalone dsh-acp) and the TUI Client tree **live in different processes and do not share Cordis**. They coordinate with ACP only:

| Direction | Over ACP | Not over the wire |
|---|---|---|
| Agent → UI | `session/update`, `available_commands_update`, `config_option_update`, permission; negotiated `_dsh/cordis/inspect/*`, `run/*`, and `plugins/*` notifications | The whole plugin graph, `inject` lists, fibers, `ctx.*` |
| UI → Agent | `session/prompt`, `cancel`, `set_session_config_option`, `authenticate`; negotiated `_dsh/cordis/inspect/*`, `run/*`, `plugin/*`, and `plugins/*` requests/notifications | A client plugin `register`ing a server service |

A server plugin that any client should see is projected into standard ACP (commands, config, updates). Client plugins (skins, slots) stay on this tree. The TUI is a Cordis **Client**: it serializes the discoverable `Theme.listTokens` and `Slots.list` directories over `_dsh/cordis/*`, the same kind of projection performed by the web Client. Both sides must negotiate protocol 0 at `initialize` `_meta.dsh.cordis.protocol`; without it, the TUI neither sends nor consumes these extensions.

The follow-up FIFO and Agent/session navigation are Client interaction state,
not timeline content. The Rust painter projects local snapshots into the Client
compositor. Builtin `tuiQueue`/`queue-view` render Queue state in
`conversation.input.dock`, with every queued item visible above the input;
empty Enter steers the FIFO head immediately, and a deferred steer restores it
to the front. Otherwise the head advances as the next prompt when the turn idles.
Builtin `tuiAgents`/`agents-view` render session
navigation in `conversation.navigation.dock`. The navigation row sits inside
the composer between its input and metadata row. It defaults to a collapsed
`· Agents · completed/total` summary; `↓` expands every item in place. `←/→` moves,
Enter opens, and Escape collapses. No popup or mouse action is created. Native
Queue and Agent views retain
the same semantics as fallbacks when no Client tree is present.

A standard ACP `tool_call` carrying subagent `_meta` still enters the parent
transcript as the original tool event; the parser only appends a
`SubagentStarted` projection. A terminal `tool_call_update` likewise produces
the original `ToolResult` before appending `SubagentFinished`, so subagent
lifecycle never replaces the visible tool request.

```text
Host process: Base Cordis + ACP plugin
        │ TUI Client child stdin/stdout (ACP)
Client process: independent Cordis root
  client plugins  sibling insert + inject ['tuiTheme' / 'tuiPresets' / 'tuiSlots']
  Client runner   injects Client services and runs dsh-tool-cordis code.client
  TUI shell       injects acpClient plus Client services; owns only the TTY
  plan-view       injects acpSessionPlan + tuiSlots + tuiCommands + tuiOverlay
  tui-queue       receives local FIFO snapshots and provides tuiQueue
  queue-view      tuiQueue → conversation.input.dock
  tui-agents      receives local Agent snapshots and provides tuiAgents
  agents-view     tuiAgents → conversation.navigation.dock + /agents
  stats-view      injects acpSessionStats + tuiSlots
  status-view     injects acpSessionStatus + acpSessionStats +
                  tuiCommands + tuiOverlay; registers the /status command
  harness-view    injects tuiCommands + tuiOverlay + acpClient + acpSessionStatus;
                  standalone startup/switch initializes and binds an empty session;
                  /harness switches it immediately before the first prompt,
                  otherwise confirms before initializing the next Harness;
                  product defaultHarness wins persisted activeHarness at startup,
                  while /harness only updates activeHarness;
                  profile runtime stays Host-owned
  tui-presets     provides tuiPresets; owns persistent /ui composition selection
  martty-preset   default UI Plugin → welcome.hero + welcome.info
  deepseek-logo   injects tuiPresets + tuiSlots; registers the deepseek UI Plugin
  acp-session-status  provides acpSessionStatus (connection/server/auth/
                  session/model/effort/permission/plan/agent run-state facts)
  tui-slots       provides tuiSlots; declares welcome.hero / welcome.info / chrome.right /
                  conversation.input.dock / conversation.navigation.dock /
                  conversation.composer.dock
  tui-theme       provides tuiTheme
  acp-client      attaches Host stdio, provides acpClient
        │ Client fd 3/4 carries only the inherited TTY
  Rust painter    stdin/stdout owns the TTY; its own fd 3/4 is compositor mux

standalone: the Client process may instead spawn any ACP agent on standard stdio
```

A profile only needs `@openma/deepseek-harness-tui`. dsh supplies Base through
the normal profile composition; the TUI bundle mounts the ACP plugin and Creator
Host overlay from its runtime dependencies, plus the Host runner. The runner
does not spawn `dsh-acp`: it starts the independent TUI Client process and binds
standard ACP stdio. That process constructs the Client root; Host bundle rows
are not Client fibers.

## Creator skill overlay

Creator skills belong to the agent Host, not the TUI Client tree. The TUI
package's internal `creator-overlay` entry is layered onto the Host Base tree.
It injects `agentPresets` and
`skills`, obtains the
upstream Creator standing scope key through `standingKeyFor('cordis')`, then
mounts its own fiber with that key and registers `tui-plugin-development`.
Only the `cordis` scope layer receives the skill; other presets and the global
catalog do not see it, and unloading removes only this contribution.

This is not a Creator fork and does not edit its `agent.cordis.yml`. Restarting
the profile after an upstream upgrade mounts the overlay on the new standing
composition. Skill discovery and registration do not use ACP; TUI Client
capability queries and dynamic `code.client` execution still use the negotiated
Client inspect/run channel.

## Layers

```text
Third-party plugins   sibling insert + inject ['tuiTheme'] → palette register
                      sibling insert + inject ['tuiPresets'] → UI composition register
                      sibling insert + inject ['tuiSlots'] → slot register
tui-theme             palette → Theme registry
tui-presets           several UI contributions → persistent /ui composition
tui-slots             TuiNode trees → welcome.hero / welcome.info / chrome.right / input / navigation / telemetry dock registry
plan-view             standard ACP Plan projection → input dock + /plan-view overlay
tui-queue             local Client FIFO snapshots → tuiQueue service
queue-view            tuiQueue → input dock summary and in-composer picker
tui-agents            local Agent/session snapshots → tuiAgents service
agents-view           tuiAgents → navigation dock + inline session selection
stats-view            standard ACP usage/timing projection → composer dock stats line
status-view           acpSessionStatus + acpSessionStats → /status markdown overlay
harness-view          Harness registry + started state → /harness confirmation + new process
martty-preset         default UI Plugin → Martty Hero + native dynamic information
deepseek-logo         deepseek UI Plugin → DeepSeek Hero + native dynamic information
acp-session-status    standard ACP run-state projection: connection/server/auth/
                      session/model/effort/permission/plan/agent — no token or
                      timing accumulation (acpSessionStats owns that)
Client runner         Client inspect + code.client mount/stop
TUI shell             consumes Theme, slot, and generic overlay snapshot trees
acp-client            session/new · authenticate · prompt · cancel · config · commands
                      provides acpSessionConfig / acpSessionPlan / acpSessionStats
                      plus a generic effect-scoped ACP observer registry
                      `_dsh/cordis/*` is negotiated at initialize
                      `_dsh/cordis/tui/*` is the mux-isolated painter child domain
Rust painter          input, keymap, existing widgets reading Theme, kitty, clipboard
```

| Layer | Owns | Does not own |
|---|---|---|
| Palette plugin | dark/light values for closed token names | Layout, token names, the TTY, a particular harness |
| acp-client | ACP session control; root or insert | Composing dsh, the UI model |
| tui-theme / tui-slots | The `tuiTheme` / `tuiSlots` registries | The TTY, any agent dependency |
| Client runner | Publishes TUI Client capabilities to `dsh-tool-cordis` and mounts `code.client` | Owning the TTY, running Web React plugins |
| TUI shell | TTY, input, and compositor; consumes structured slot/overlay snapshots | Owning Plan/stats domain projections or interpreting harness `SessionEvent` |
| Rust | Painting Theme through existing widgets | Interpreting colors in JS |

The `tui-theme` service starts with builtin `default`. A dynamic Theme Plugin
declares its palette with `tuiTheme.register`; `/theme` is a special single-select
Plugin switch that starts the target and stops the current Theme Plugin. Its
palette, commands, overlays, slots, and RPC therefore share one Fiber lifetime.
`martty --demo` stays on `default`; `--demo-skin` remains the static gallery path.

## Control plane vs paint

Control uses existing ACP methods (including `authenticate`, matching Backchat's probe / sign-in panel: forms go through `_meta`, Terminal Auth occupies the TTY). Client `initialize` advertises `fs.readTextFile` / `fs.writeTextFile` and `terminal` (`createTerminal`, distinct from `auth.terminal`); mid-session `auth_required` opens client `/auth` — do not tell the user to type `/login`. The transcript is painted from `session/update`. Palettes are `tuiTheme.register` on the **client** tree; `_dsh/cordis/tui/theme/update` reaches the Rust painter as a notification and is not a third-party plugin API. `_meta.dsh.cordis` negotiates capability only; chrome data belongs in extension notifications.

## Theme

Token **names** are closed; see [plugins.md](plugins.en.md). Built-in `default` keeps the cold bluish values. A palette pack may (and must) supply `#RRGGBB` for every token. When conversation nodes open later, nodes still name tokens; they do not carry RGB.

`/theme toggle` or `ctrl+t` toggles dark/light inside the current theme. `/theme <id>` switches the whole
Theme Plugin. Kitty pet sprites are RGBA and do not recolor. In the startup
lockup, `MAR` reads the ocean gradient while `TTY` uses terminal foreground
(white in dark mode, black in light mode). A UI Plugin can compose several UI
contributions and is not a Theme alias. Both builtin `default` (Martty) and `deepseek`
fill `welcome.hero` and `welcome.info`: the first is the centered `logo + hint`
brand region; the second is the lower-left version/model/workspace/session/
credential/access/help region. Both currently reuse the native dynamic info
renderer, while the slot remains independently replaceable. The Rust painter
reuses the original responsive whale primitive and owns whole-welcome vertical
centering. `/ui deepseek` and `/ui default` switch the saved composition.

## Out of scope

- Running Web `dsh-client-ui-*` React components in the terminal.
- An open ACP method table for third parties.
- Putting acp-client and acp-bridge in one process that fights over stdout.
- The TUI npm package depending directly on `@deepseek-ai/dsh-*`. `@deepseek-ai/cordis` and ACP are the explicit runtime dependencies; ACP and the internal Creator overlay stay out of Client bundle rows.
- Plugins emitting kitty, entering raw mode, or reading full-screen size for absolute layout.
- Treating an extra timeline message as proof the TUI is pluggable.

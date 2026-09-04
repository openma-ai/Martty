<p align="center">
  <img src="assets/martty-lockup.svg" width="650" alt="Martty terminal lockup" />
</p>

<h1 align="center">Martty</h1>

<p align="center">
  DeepSeek Harness TUI is now <strong>Martty</strong>.
</p>

<p align="center">
  DeepSeek Harness in your terminal: streamed reasoning, tools, skills, multi-image prompts, and durable sessions.
</p>

<p align="center">
  <a href="README.md">中文</a> · <a href="README.en.md">English</a>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/martty"><img src="https://img.shields.io/npm/v/martty?logo=npm&color=cb3837" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/martty"><img src="https://img.shields.io/npm/dm/martty" alt="npm downloads" /></a>
  <a href="https://github.com/openma-ai/Martty/actions/workflows/package-npm.yml"><img src="https://github.com/openma-ai/Martty/actions/workflows/package-npm.yml/badge.svg" alt="CI" /></a>
  <img src="https://img.shields.io/node/v/martty" alt="Node.js 18+" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT" /></a>
</p>

---

## Martty: Agent TUI

- A high-performance interface written in Rust/ratatui.
- **DSH-first:** built on the same Cordis composition and plugin model as DeepSeek Harness, while also supporting other compatible ACP agents.

`martty` is the primary command. It is a terminal-native ACP client and an extensible terminal on a
Cordis **client** tree. It presents streamed reasoning, tool calls, subagents,
token usage, durable sessions, and other Agent UI elements in a Rust/ratatui interface. The recommended
profile path mounts the ACP plugin on the dsh Base Host tree and starts an
independent TUI Client process; standalone may spawn or attach any ACP agent.
Its long-term direction is not to
hard-wire more features into the TUI, but to compose themes, views, commands,
and interactions as plugins until Creator can inspect, create, run, diagnose,
and iterate on its own terminal capabilities.

> **Rename note:** the project and recommended npm package have moved from
> DeepSeek Harness TUI / `@openma/deepseek-harness-tui` to **Martty** / `martty`.
> This is a brand and package-name migration, not a runtime rewrite: `dsh-tui`,
> configuration, and session data remain compatible. The production
> profile is now named `martty`; the legacy `tui` profile remains compatible.

## Quick start

> When installing via `npm install`, [Node.js](https://nodejs.org/) must be installed on your machine, and [pnpm](https://pnpm.io/) may be required.
>
> Agent-oriented installation steps: [agent install guide](docs/agent-install.md).

### Recommended: dsh TUI surface plugin

Requires Node.js 18+. Install the official
[DeepSeek Harness](https://github.com/deepseek-ai/dsh), then add the TUI
directly to the `martty` profile:

```sh
npm install -g @deepseek-ai/dsh
dsh plugin --profile martty add martty@latest
dsh --profile martty
```

`dsh plugin ... add` is the recommended installation and upgrade entry point.
It creates the profile and installs the TUI plus its ACP dependency; no global
`martty` or separate pnpm installation is required.
The legacy `@openma/deepseek-harness-tui` name continues to receive the same
versions, so existing installs do not need to migrate immediately.

### Migrating from DeepSeek Harness TUI / the legacy package name

The project is now named **Martty**, and `martty` is the recommended package
name. Starting with `0.2.13`, `martty`
and `@openma/deepseek-harness-tui` are published by the same CI run with the
same version and artifacts; the scoped name remains as a compatibility alias.
To migrate both the legacy package name and profile:

```sh
dsh plugin --profile tui remove @openma/deepseek-harness-tui
dsh plugin --profile martty add martty@latest
```

Migration changes the recommended profile from `tui` to `martty`, but requires
no config or session-data migration. `dsh-tui` remains a compatibility
commands, and the legacy package and profile continue to work.

An install guide for AI agents: [docs/agent-setup.md](docs/agent-setup.md)

TUI declares ACP as its own runtime dependency. If the target profile already
contains another version through the standard ACP bundle, the package manager
may retain both dependency copies, but the TUI bundle disables that surface's
old provider/transport rows and mounts only the ACP plugin resolved from TUI's
dependency graph. Supported profile compositions therefore do not start two
ACP surfaces, and users do not have to normalize an existing ACP profile first.

### Global: an Agent TUI out of the box

One global install provides the Martty Agent TUI and its built-in ACP connection layer:

```sh
npm install --global martty
martty
```

Martty directly depends on our `@openma/deepseek-harness-acp` implementation.
The default launch resolves this built-in ACP from Martty's dependency graph,
connects DSH, and mounts the Creator overlay; no separate ACP installation is
required. `martty` is therefore the complete DSH-first Agent TUI entry.

Run the same entry point ephemerally when you do not want a global install:

```sh
npx --yes martty
npx --yes martty --demo
```

The built-in ACP can also connect another ACP server. This changes its
connection configuration; it does not replace Martty's ACP client. For a
standalone launch, configure `DSH_TUI_AGENT`:

```sh
DSH_TUI_AGENT="<acp-command> [args...]" martty
```

Standalone launches can also keep named ACP harnesses in Martty settings and
select which one the next launch uses:

```sh
martty harness list
martty harness add codex
martty harness add local --label "Local ACP" --command local-acp --arg --stdio
martty harness use local
martty --check-runtime
```

All three entry points share the same registry: edit `settings.json`, use the
`martty harness` CLI above, or enter `/harness` in the running TUI for the
native single-select form. `/harness <id>` saves directly. TUI selection also
replaces the standalone ACP child immediately. If the current session has
already received a prompt or was restored through `/session`, Martty confirms
first and keeps that session available for navigation.

`harness list` includes saved entries, the bundled DSH runtime, and executable
`*-acp` / `*_acp` entrypoints found on `PATH`; `*` marks the active entry.
`add` and `use` preserve the other fields in `$MARTTY_HOME/settings.json` while
writing `harnesses` and `activeHarness`. Selection applies to the **next
standalone launch**, which initializes the selected Harness and binds an empty
session. This makes the Agent's model and effort config available before the
first prompt without starting a model turn. Sessions are never carried across Harnesses, and
`dsh --profile martty` continues to use its Host-owned runtime.

`martty harness add codex` is a shortcut that saves an `npx`
`@agentclientprotocol/codex-acp` recipe, so no Codex CLI flags need to be guessed.

Standalone precedence is explicit `--agent`, `DSH_TUI_AGENT`, the product's
internal initialization `defaultHarness` (empty by default), the persisted
`activeHarness`, then the bundled fallback. `defaultHarness` is not a CLI or
user startup argument. `/harness` updates only `activeHarness`; it never
changes the product default.
A one-run `--agent` override never changes the saved choice.

Cordis embedding uses `config.agent: { command, args }` to spawn a server, or
`config.stream` to attach caller-owned standard pipes. `martty --agent <cmd>`
plus repeated `--agent-arg` flags are standalone command-line overrides for
the same configuration.

For an existing DSH environment with a durable `martty` profile and standard
plugin management, the recommended path remains
`dsh plugin --profile martty add martty@latest`.

From this checkout (need `cargo build --release` or `scripts/build-npm.sh`, binary in `npm/vendor/<platform>/`, or `MARTTY_BIN`):

```sh
MARTTY_BIN=$(pwd)/target/release/martty martty --agent dsh-acp
```

For local iterating, `scripts/devlocalinstall.sh` builds with the fast
`devlocal` Cargo profile (debug codegen, incremental — the quickest
turnaround) and stages the binary into the running `dsh --profile <name>`
setup; `DSH_TUI_CARGO_PROFILE=release` restores the shipped config.

Third-party capabilities are ordinary Cordis plugins on the client tree: they
declare the services they need, register contributions in `apply`, and retract
them with the owning fiber. Themes, the root right rail, local commands,
slider/single-select/view overlays, UI Presets, current ACP Session config
transactions, and package-private
Host/Client RPC are open today. See the [plugin API](docs/plugins.en.md) and
[Fully pluggable and self-evolving](#fully-pluggable-and-self-evolving) for the
complete direction. `--demo-skin` merely mounts the gallery pack `ember`; it
does not put theme-specific behavior in core.

### Try the demo first

The demo needs neither a runtime nor an API key:

```sh
npm install --global martty
martty --demo
```

You can also run `npx --yes martty --demo`. `martty` is the primary command;
`dsh-tui` remains a compatibility alias.

## Highlights

- **A complete agent timeline:** stream reasoning, replies, tool arguments and
  results, plugin context, subagent lifecycles, and token/cache metrics. A live
  status line keeps the current phase, elapsed time, and queue depth visible.
- **Native ACP capabilities:** read the agent's models, compositions,
  permissions, authentication methods, and available commands. Skills join
  built-in commands in one searchable, scrolling slash menu.
- **Multi-image prompts:** stage up to eight images from files, the clipboard,
  or paste. Editable `[image n]` chips live inline with the draft and expose a
  preview with name, dimensions, size, and media type.
- **Terminal-aware Markdown:** render headings, lists, quotes, fenced and inline
  code, emphasis, strikethrough, links, and image markers while preserving
  styling across CJK/Latin text and soft wraps.
- **Dense tool views:** tool calls expose running, success, and failure states;
  results collapse to the last four lines and expand inline while the wheel
  always scrolls the conversation.
- **Long-turn control:** queue follow-ups or steer the active turn immediately;
  manage durable JSONL sessions with `/new`, `/resume`, and `--session-id`, with
  workspace mode facts cached across launches.
- **Cross-platform editing:** readline commands, contextual shortcuts, physical
  ⌘/⌥ modifier rescue on macOS, and ctrl conventions on Linux and Windows keep
  movement and deletion consistent across terminals.
- **A terminal-native surface:** dark/light themes, narrow layouts, mouse
  selection and tool interaction, native/tmux/OSC 52 clipboard routing, kitty
  image previews, and the optional `/liang` pixel companion.

<p align="center">
  <img src="assets/screenshots/agent-turn.png" width="720"
       alt="Markdown output, tool views, and live ACP run state" />
</p>

## Fully pluggable and self-evolving

The goal is a small TUI kernel plus composable plugins, not a terminal app that
accumulates product-specific branches. Core owns ACP sessions, the TTY, input
dispatch, layout constraints, and semantic-node rendering. Product capabilities
enter the client tree through Cordis services, slots, and plugin lifecycles.

- **One lifecycle:** static packages and Creator-generated dynamic packages use
  the same Cordis Loader, fibers, `inject`, and disposers. Mounting takes effect
  immediately, while stopping or switching retracts every contribution; there
  is no second "dynamic plugin" runtime.
- **One plugin, several surfaces:** a package may contribute a theme, slots,
  commands, and overlays together, sharing state or coordinating through
  package-private Host/Client RPC. Core adds no Liang-, effort-, or plugin-specific
  branches.
- **Semantic capabilities only:** plugins submit `TuiNode` trees and general
  interaction semantics such as sliders and forms; the Rust renderer adapts them
  to the terminal. Plugins never receive the TTY, raw mode, Ratatui, kitty escape
  sequences, or absolute coordinates. Replacing the renderer must not change the
  plugin ABI.
- **Dynamic preview and durable composition stay separate:** `define/run` drives
  immediate preview, while `stop/update/rollback` owns the live lifecycle; an
  approved Package can then be persisted. `AgentPreset` continues to compose
  agent-side capabilities; an independent `UI Preset` composes Client/UI
  contributions such as themes, welcome slots, and pets. They are stored and
  switched separately.
- **The Creator loop:** Creator inspects the real Host/Client services, slots,
  tokens, and schemas before generating `code.host`, `code.client`, or both. It
  then observes activation and render failures and can repair, update, roll back,
  or save the Package. That loop is "self-evolution"; it is not direct model
  access to terminal internals.

### Current boundary

The ACP client split is in place, together with these primitives on one dynamic
Package lifecycle:

- `tuiTheme` and the single-select `/theme` Plugin seat;
- `tuiSlots`, `welcome.hero`, `welcome.info`, the input, navigation, and
  telemetry docks,
  `chrome.right`, and schema-validated `TuiNode` trees;
- lifecycle-owned local slash commands with refreshable argument candidates,
  plus native slider, single-select, and view overlays;
- persistent UI Presets, including the built-in Martty and DeepSeek welcomes;
- a current-Session config directory and transactions projected from standard
  ACP `configOptions`;
- Client inspect/run, Package stop/start/retract, and package-private
  Host/Client RPC;
- a TUI development skill visible only in the Creator preset and registered
  independently of ACP.

Static plugins and dynamic `code.client` packages use the same primitives;
none is a demo-specific side channel. More shell and conversation slots,
additional form field types, and complete runtime diagnostics are still being
migrated. "Fully pluggable" remains the target architecture.
See the [migration plan](docs/migration.en.md) for phase-by-phase status.

### Compared with the Web plugin platform

| Dimension | Web today | TUI foundation and direction |
|---|---|---|
| Client runtime | Mature React Cordis tree | Node Cordis client tree is in place; Rust remains a semantic renderer, not a third tree |
| UI extension | Typed slot tree across conversations, settings, tool cards, and many other surfaces | `chrome.right` is open today; `tuiSlots` will cover shell and conversation surfaces without exposing terminal coordinates |
| Theme | `ThemeRuntime` registers themes, stacks token overrides, switches at runtime, and persists built-in preferences | `/theme` is a single-select Plugin switch that loads/replaces a Theme Plugin and all of its contributions together |
| Interaction components | Plugins contribute React components | Schema-validated `TuiNode`, slash argument candidates, and slider/single-select/view overlays are open; more form fields continue as general terminal semantics |
| Dynamic plugins | Two-half `code.host` + `code.client` Packages share Loader/fiber and support run, stop, update, and rollback | Inspect/run, themes, right rail, commands, overlays, config transactions, and package-private RPC share one DSH Cordis ACP extension; diagnostics and durable composition continue |
| Diagnosis and repair | Client activation and React render failures return to Creator for another revision | The target is the same loop for activation, schema, and paint failures, with update and rollback |
| Presets | `AgentPreset` composes agents; Client plugins persist separately | Keep the AgentPreset boundary; an independent `UI Preset` now composes and persistently switches terminal UI contributions |

The Web plugin surface is broader and more mature today. TUI aligns with its
Cordis composition, lifecycle, and Creator loop; it does not move React or the
browser DOM into the terminal.

## Runtime architecture

The primary `dsh --profile martty` path mounts the ACP plugin on the Host Base
Cordis tree, then starts a Node Cordis Client tree in a separate process:
`tui-theme` provides the palette
registry, `tui-cordis-client-runner` serves Client inspect/run requests from
`dsh-tool-cordis`, `acp-client` attaches the Host's standard stdin/stdout, and
`dsh-tui-shell` starts the Rust painter and multiplexes messages. The agent (default
Harness) stays on the Host tree; the two Cordis trees only speak ACP. Standalone
`dsh-tui` instead spawns or attaches the configured ACP agent. The Rust painter
is not a third Cordis tree: it owns the TTY, input, and rendering only.

The two Cordis trees do not synchronize plugin ids, `inject`, or fibers.
Standard ACP continues to carry sessions, prompts, authentication,
configuration, and `session/update`. Client capability discovery, dynamic
Package execution, and package-private RPC for self-evolution use a negotiated
ACP extension instead. The target extension is unified under `_dsh/cordis/*`
and advertised through `initialize` `_meta.dsh.cordis`; an ACP agent without
that extension remains fully usable as a regular agent.

Creator guidance remains a separate Host overlay exported by the TUI package;
ACP is its runtime dependency. Users install only the TUI; its bundle mounts
the ACP plugin and Creator overlay on the Base Host tree, while the runner
only starts the TUI Client process. Creator adds `tui-plugin-development` to
the upstream `cordis` preset's standing
scope. It neither copies the preset nor edits upstream files, and skill
discovery and registration do not depend on ACP. Web and TUI therefore use the
same Creator preset.
ACP and the Creator overlay never enter the Client tree, so the full Harness boots only once
in the Host process.

Host↔TUI Client ACP uses the Client child's standard stdin/stdout. Client fd 3/4
only inherits the user TTY and becomes the Rust painter's stdin/stdout; the
Rust child's own fd 3/4 is the Node↔painter compositor channel.

Node and Rust use fds 3/4 on Unix and a token-authenticated loopback TCP socket
on Windows. This private compositor channel only projects semantic paint state,
including themes and `TuiNode` trees. It is neither a plugin API nor an agent
business channel. Generic Cordis inspect/run/lifecycle uses `_dsh/cordis/*`;
painter capabilities such as themes, slots, commands, and overlays use the
`_dsh/cordis/tui/*` child domain. These are underscore-prefixed ACP Extension
Requests/Notifications and never enter prompts or conversation history.

Interaction details, the complete keymap, and developer integration notes
live in the [composer input docs](docs/composer-input.md).
Agent-oriented, fully verifiable installation steps live in the
[agent install guide](docs/agent-install.md).

## Essential interactions

| Key / command | Behavior |
|---|---|
| `enter` | Send; with an empty composer and a non-empty Queue, send its head immediately |
| `ctrl+enter` | Steer the active turn immediately without cancelling it (macOS `⌘⏎`) |
| `alt+↑` | Select any Queue item; `↑/↓` moves, Enter edits, and `ctrl+d` deletes |
| `↓` · `←/→` · Enter | From an empty prompt, expand Agent navigation, move, and open; Escape collapses it |
| `esc` | Interrupt the current turn (draft survives); clears the draft when idle |
| `ctrl+c` | Clear the draft first; press twice when idle, five times while a turn runs, to quit; never interrupts the current turn |
| `/` | Open the upward command menu and filter by prefix; after `/command ` the same menu shows argument candidates. Enter selects and runs; Tab only completes. Agent-advertised skills still ship as `/name ` prompts |
| `/ui` · `/ui ` | Enter directly for the ordinary UI Preset single-select form; add a trailing space for upward Martty / DeepSeek candidates |
| `/model` · `/agent` | Pick an agent-advertised model or agent preset; `ctrl+shift+a` cycles agents directly without a picker |
| `/harness [id]` | Switch the standalone Harness and start a fresh empty session |
| `/auth` | ACP sign-in (method picker when several methods; otherwise Terminal Auth or `authenticate` `_meta`); mid-session `auth_required` opens the same surface; the agent's `/login` stays a prompt |
| `/permission` · `shift+tab` | Pick or cycle agent-advertised permission modes |
| `/vim` | Toggle vim modal editing (off by default; normal mode h/j/k/l move, dd kill line, i insert) |
| `/effort` · `/plan` | Set reasoning effort or pass plan mode to the host |
| `/image <path> [text]` | Send a local image (png/jpeg/webp/gif); ACP Image blocks when the agent advertises `promptCapabilities.image`, otherwise `resource_link` |
| `/clip [text]` · `ctrl+v` | Stage the clipboard image (repeatable; up to 8 ride one prompt); macOS/Linux |
| Image chips | Live inline in the draft as `[image n]` tokens (no icon); backspace cuts the whole chip, hover (or park the cursor on) one for a preview popup — kitty thumbnail + dimensions/size/type |
| `ctrl+o` · `ctrl+t` | Expand output · toggle the theme |
| `/keys` | Open the complete keyboard-shortcut modal |
| `pgup/pgdn` · `ctrl+u/d` (empty prompt) | Scroll; `end` follows the live tail |
| Draft editing | `←/→` move by char · `↑/↓` move by screen line · `home/end` visual line start/end · `shift+enter` newline · `ctrl+d` delete forward |
| Readline editing | `home/ctrl+e` line ends · while typing, `ctrl+k/u` kill to end/start · `ctrl+shift+k` kill the line · `ctrl+w` word back · `ctrl+z` undo · `ctrl+shift+z` redo (macOS `⌘z` / `⌘⇧z`) · `ctrl+y` paste the last kill |
| macOS | `⌘←/→` line ends · `⌥←/→` word hops · `⌘⌫` kill to start · `⌥⌫` word back (physical key state read natively — works in every terminal) |
| Linux/Windows | `ctrl+←/→` word hops · `ctrl+⌫` word back |
| Click tool · wheel | Click a tool to expand/collapse it; wheel always scrolls the conversation |
| Mouse drag | Copy on release; double-click a word; `shift+drag` uses native selection |
| `!cmd` | Run a command in the client's session-local shell, outside the agent; the shell starts in the workspace and keeps `cd`, environment variables, and other state for later `!` commands until the TUI exits |

### Composer text editing

| Key | Behavior |
|---|---|
| `←` / `→` | Move by char (`ctrl+b` / `ctrl+f`) |
| `↑` / `↓` | Move by screen line (multiline drafts) |
| `home` / `end` | Line start / end (`ctrl+a` / `ctrl+e`; macOS `⌘←/→`) |
| `alt+←` / `alt+→` | Move by word (Linux/Win `ctrl+←/→`) |
| `⌫` / `delete` | Delete before / after the cursor |
| `ctrl+w` | Delete the word before the cursor |
| `ctrl+k` / `ctrl+u` | Kill to line end / start (macOS `⌘⌫` kills to start) |
| `ctrl+shift+k` | Kill the whole line |
| `ctrl+z` / `ctrl+shift+z` | Undo / redo (macOS `⌘z` / `⌘⇧z`) |
| `ctrl+y` | Paste the last killed text (yank) |
| `shift+←/→/↑/↓` · `shift+home/end` | Extend the selection |
| `ctrl+shift+c` / `ctrl+x` | Copy / cut the selection (keyboard and mouse drags) |
| `shift+enter` / `ctrl+j` | Newline in the draft |

Use `/help` for commands and `/keys` for the complete shortcut list.
The full editing reference — keys, mouse click/drag behavior, and developer
integration notes — lives in the [composer input docs](docs/composer-input.md).

The welcome surface is a composable UI Preset. Built-in `default` (Martty) and
`deepseek` each mount a centered `welcome.hero` (`logo + hint`) and a separate
dynamic `welcome.info` slot. `/ui` opens the native select form, `/ui ` reuses
the upward slash menu for candidates, and the choice persists in
`$MARTTY_HOME/settings.json`.

### UI Preset vs Agent Preset

Both apply the same idea—turn several plugin contributions into one selectable
composition—but they live on different Cordis trees and remain separate:

| | Agent Preset | UI Preset |
|---|---|---|
| Composes | Host/Agent system prompt, tools, skills, and runtime capabilities | Client UI slots, pets, chrome, and other structural contributions |
| Switch with | `/agent` | `/ui` |
| Built-ins | Agent compositions such as standard, code, minimal, and cordis | `default` (Martty) and `deepseek` |
| Persistence | Session/Agent selection | UI selection in `$MARTTY_HOME/settings.json` |

Creator mode can inspect the real `UiPresets`, `Theme`, `Slots`, `Commands`,
and `Overlay` contracts, generate a dynamic Package with `code.client`, and
call `tuiPresets.register({ id, label }, mount)` to create a new UI Preset.
The mount callback composes that Preset's welcome slots, pet, or other structural
UI contributions. Once the Package is saved and mounted, the new Preset appears in
both the `/ui` form and `/ui ` candidates and shares the same switching,
persistence, and unload lifecycle. Creator produces an ordinary Client Plugin
composition; it does not receive the TTY, absolute coordinates, or a way around
the slot/overlay protocols.

#### Return to the classic DeepSeek view

After `dsh --profile martty` starts, run `/ui deepseek` directly, or enter `/ui`
and choose DeepSeek in the ordinary single-select form. Typing one trailing
space after `/ui` (that is, `/ui `) opens the same candidates above the
composer.

The classic view restores the responsive whale, `DEEPSEEK HARNESS` wordmark,
and hint text while keeping Martty's pluggable shell, ACP session, and dynamic
info slot. The choice persists across launches. Run `/ui default` (or choose
Martty in the `/ui` form) to return to the blue-white `MARTTY` view. The old
experimental `/deepseeklogo` command has been replaced by this UI Preset.

<p align="center">
  <img src="assets/screenshots/skills-menu.png" width="720"
       alt="Built-in commands and host skills in one slash menu" />
</p>

<p align="center">
  <img src="assets/screenshots/image-preview.png" width="720"
       alt="An inline image chip with its metadata preview" />
</p>

<details>
<summary><strong>Composer pet: /liang 🤫</strong></summary>

`/liang` places a small pixel-art companion beside the composer: quiet while
idle, typing at a tiny terminal during a turn. Terminals with kitty graphics
protocol support, including Ghostty, Kitty, and WezTerm, get RGBA sprites;
others fall back to a half-block whale. The pet hides below 60 columns.

Use `/liang on` or `/liang off` to control it explicitly. He is off by
default; run `/liang on` to summon him.

<p align="center">
  <img src="assets/screenshots/liang.png" width="640"
       alt="The optional Liang pixel companion beside the composer" />
</p>

</details>

## Build from source

Requires Rust stable and Node.js 18+:

```sh
make rust-test
node --test scripts/package-native.test.mjs
bash scripts/build-npm.sh
```

`make rust-build` / `make rust-test` / `bash scripts/build-npm.sh` go through
`scripts/cargo-guard.sh`. It runs a scoped `cargo clean` first when this
repository's `target` exceeds 20 GiB or free disk falls below 10 GiB.
`make rust-cache-status` is read-only and `make rust-cache-prune` cleans
explicitly; override the thresholds with `RUST_CACHE_MAX_GIB` /
`RUST_DISK_MIN_GIB`.

Use `make tui-test` for the real development profile. It rebuilds
`target/debug/martty` first, then launches `tui-test` with `MARTTY_BIN`, so
Node HMR cannot leave the Rust painter on an older process image.

The local script builds only the current platform and writes a tarball to
`dist/`. The `Package and publish npm` GitHub Actions workflow builds and then
assembles these paths into one npm package:

```text
npm/vendor/darwin-arm64/martty
npm/vendor/darwin-x64/martty
npm/vendor/linux-x64/martty
npm/vendor/win32-x64/martty.exe
```

Pushing a tag that matches both `npm/package.json` and `Cargo.toml` (for example,
`v0.1.0`) publishes to `latest` through npm Trusted Publishing (OIDC), then
creates a GitHub Release with the tarball. A version mismatch fails before
publishing.

## Troubleshooting

- **`no native binary for ...`**: the installed package does not contain your
  platform. Confirm that you installed the latest version and check the support
  matrix.
- **Built-in ACP cannot start:** reinstall the latest `martty`; a source
  checkout must have its `npm/` dependencies installed. Configure
  `DSH_TUI_AGENT`, `config.agent`, or `config.stream` for another ACP server.
- **No pixel pet**: the terminal may not support kitty graphics protocol; the
  rest of the UI is unaffected.

## Repository map

- `src/`: TUI state, rendering, protocol, runtime lifecycle, and session catalog.
- `npm/`: Cordis client boot, ACP/compositor mux, CLI shim, and native binaries.
- `scripts/`: local builds, cross-platform package checks, protocol integration
  tests, and asset generation.
- `assets/`: screenshots, visual assets, and optional pet sprites.
- `docs/`: architecture, plugin API, `TuiNode` schema, the migration plan, and
  the AI-agent install guide ([index](docs/README.md)).

Agent communication is ACP over stdio; Node and Rust use a separate private
compositor channel. Start with [`src/acp.rs`](src/acp.rs),
[`npm/lib/boot.js`](npm/lib/boot.js), and [`npm/lib/mux.js`](npm/lib/mux.js).
Plugins must not depend on those transports; extension points are in
[docs/plugins.en.md](docs/plugins.en.md).

## License

[MIT](LICENSE). Not affiliated with DeepSeek or xAI;
[grok-build](https://github.com/xai-org/grok-build) is an interaction design
reference, and [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
is the runtime substrate.

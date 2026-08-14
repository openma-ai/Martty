# dsh-tui

[中文](README.md) | [English](README.en.md)

[![npm](https://img.shields.io/npm/v/%40openma%2Fdeepseek-harness-tui)](https://www.npmjs.com/package/@openma/deepseek-harness-tui)
[![Package and publish npm](https://github.com/openma-ai/deepseek-harness-tui/actions/workflows/package-npm.yml/badge.svg)](https://github.com/openma-ai/deepseek-harness-tui/actions/workflows/package-npm.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A terminal-native agent UI for DeepSeek Harness. Follow streamed reasoning,
tool calls, subagents, token usage, and durable sessions in one Rust/ratatui
interface. Run it as an official `dsh` profile plugin or connect it directly to
the SDK JSON-RPC runtime.

> **v0.2.x** · The official package supports macOS on Apple Silicon and Intel,
> Linux x64, and Windows x64. The current integration baseline is
> `dsh 0.1.0-rc.6`.

![The DeepSeek Harness home screen in dsh-tui 0.2](assets/screenshots/banner-v020.png)

## Quick start

### Recommended: run as a dsh profile plugin

Requires a configured `dsh` installation, Node.js 18+, and pnpm 10+.

```sh
dsh plugin --profile tui add @openma/deepseek-harness-tui
dsh --profile tui
```

The install command does not need `-w`. Confirm that the bundle is mounted as
`tui-runner` with:

```sh
dsh --profile tui --dump-config
```

### Try the demo first

The demo needs neither a runtime nor an API key:

```sh
npm install --global @openma/deepseek-harness-tui
dsh-tui --demo
```

`dsh-tui` is the primary command; `dsb` remains as a compatibility alias.

## Highlights

- **A complete agent timeline:** stream reasoning, replies, tool arguments and
  results, plugin context, subagent lifecycles, and token/cache metrics. A live
  status line keeps the current phase, elapsed time, and queue depth visible.
- **Native host capabilities:** plugin mode reads dsh models, agent presets,
  permissions, providers, credentials, and invocable skills. Host skills join
  built-in commands in one searchable, scrolling slash menu.
- **Multi-image prompts:** stage up to eight images from files, the clipboard,
  or paste. Editable `[image n]` chips live inline with the draft and expose a
  preview with name, dimensions, size, and media type.
- **Terminal-aware Markdown:** render headings, lists, quotes, fenced and inline
  code, emphasis, strikethrough, links, and image markers while preserving
  styling across CJK/Latin text and soft wraps.
- **Dense tool views:** tool calls expose running, success, and failure states;
  results collapse cleanly and long output gets its own scrollable viewport
  instead of taking over the conversation.
- **Long-turn control:** queue follow-ups or interrupt and send immediately;
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
       alt="Markdown output, tool views, and live run state in plugin mode" />
</p>

## Two runtime modes

| | dsh plugin (recommended) | Standalone |
|---|---|---|
| Agent, tools, providers | Supplied by the dsh profile | Supplied by a separate SDK runtime |
| Models and agent presets | Uses the live host catalog and switches from the TUI | Uses launch flags or runtime configuration |
| Session storage | `~/.dsh/sessions` | `~/.dsh-tui/sessions`, configurable with `--session-root` |
| Mid-turn interrupt | The host owns the turn; no hard interrupt | `esc` stops the runtime while preserving the session log |
| Runtime installation | The bundle includes its compatibility layer | Requires `dsh-jsonrpc-agent` |

The plugin runner launches the native binary on the host TTY and serves an SDK
server-compatible JSON-RPC interface over fds 3/4. It does not mount
`@deepseek-ai/dsh-sdk-jsonrpc-server` directly; the surrounding dsh profile
still supplies agents, tools, providers, and persistence.

### Standalone runtime

The global npm install provides the TUI binary. Standalone mode also needs the
DeepSeek Harness SDK in a nearby `.venv`, or an explicitly configured runtime:

```sh
python -m venv .venv
.venv/bin/pip install deepseek-harness-sdk
dsh-tui --workspace .
```

Alternatively, set `DSH_RUNTIME_BIN` or pass `--runtime-bin <path>`. Credentials
come from `--api-key`, `DEEPSEEK_API_KEY`, or the local `~/.dsh` configuration,
in that order.

## Essential interactions

| Key / command | Behavior |
|---|---|
| `enter` | Send; queue a follow-up while a turn is running |
| `ctrl+x` | Interrupt the current turn and send next (plugin forwards to host; standalone kills runtime) |
| `esc` | Interrupt the current turn (draft survives); clears the draft when idle |
| `ctrl+c` | Clear the draft, then interrupt; press twice to quit |
| `/` | Open the command menu and filter by prefix; host skills join it in plugin mode (picking one lands `/name `, enter ships it as a prompt the host expands) |
| `/model` · `/mode` | Pick a model or agent preset; the full host catalog needs plugin mode |
| `/permission` · `shift+tab` | Pick or cycle permission presets; plugin mode only |
| `/effort` · `/plan` | Set reasoning effort or pass plan mode to the host |
| `/image <path> [text]` | Send a local image (png/jpeg/webp/gif); plugin mode only |
| `/clip [text]` · `ctrl+v` | Stage the clipboard image (repeatable; up to 8 ride one prompt); macOS/Linux |
| Image chips | Live inline in the draft as `[image n]` tokens (no icon); backspace cuts the whole chip, hover (or park the cursor on) one for a preview popup — kitty thumbnail + dimensions/size/type |
| `ctrl+o` · `ctrl+t` | Expand output · toggle the theme |
| `pgup/pgdn` · `ctrl+u/d` (empty prompt) | Scroll; `end` follows the live tail |
| Readline editing | `ctrl+a/e` line ends · `ctrl+k/u` kill to end/start · `ctrl+w` word back |
| macOS | `⌘←/→` line ends · `⌥←/→` word hops · `⌘⌫` kill to start · `⌥⌫` word back (physical key state read natively — works in every terminal) |
| Linux/Windows | `ctrl+←/→` word hops · `ctrl+⌫` word back |
| Click tool · wheel over it | Click a tool to expand/collapse it; wheel scrolls its inner window |
| Mouse drag | Copy on release; double-click a word; `shift+drag` uses native selection |
| `!cmd` | Run a local shell command on the client, outside the agent |

Use `/help` for commands and `/keys` for the complete shortcut list.

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

Use `/liang on` or `/liang off` to control it explicitly.

<p align="center">
  <img src="assets/screenshots/liang.png" width="640"
       alt="The optional Liang pixel companion beside the composer" />
</p>

</details>

## Build from source

Requires Rust stable and Node.js 18+:

```sh
cargo test --locked
node --test scripts/package-native.test.mjs
bash scripts/build-npm.sh
```

The local script builds only the current platform and writes a tarball to
`dist/`. The `Package and publish npm` GitHub Actions workflow builds and then
assembles these paths into one npm package:

```text
npm/vendor/darwin-arm64/dsh-tui
npm/vendor/darwin-x64/dsh-tui
npm/vendor/linux-x64/dsh-tui
npm/vendor/win32-x64/dsh-tui.exe
```

Pushing a tag that matches both `npm/package.json` and `Cargo.toml` (for example,
`v0.1.0`) publishes to `latest` through npm Trusted Publishing (OIDC), then
creates a GitHub Release with the tarball. A version mismatch fails before
publishing.

## Troubleshooting

- **`no native binary for ...`**: the installed package does not contain your
  platform. Confirm that you installed the latest version and check the support
  matrix.
- **`cannot find ... dsh-jsonrpc-agent`**: the standalone runtime is missing.
  Install the SDK, set `DSH_RUNTIME_BIN`, or use dsh plugin mode.
- **pnpm workspace root error**: upgrade to pnpm 10+ and rerun the install
  command without `-w`.
- **`ERR_REQUIRE_ESM_RACE_CONDITION`**: 0.1.0 and earlier shipped a CommonJS
  runner that raced Cordis's parallel ESM `import()`. Upgrade to `0.1.1` or
  install this repo's `npm/` directory.
- **No pixel pet**: the terminal may not support kitty graphics protocol; the
  rest of the UI is unaffected.

## Repository map

- `src/`: TUI state, rendering, protocol, runtime lifecycle, and session catalog.
- `npm/`: dsh bundle runner, CLI shim, manifest, and native binaries.
- `scripts/`: local builds, cross-platform package checks, protocol integration
  tests, and asset generation.
- `assets/`: screenshots, visual assets, and optional pet sprites.

The wire protocol is NDJSON JSON-RPC 2.0 over stdio. Start with
[`src/proto.rs`](src/proto.rs), [`src/controller.rs`](src/controller.rs), and
[`npm/lib/index.js`](npm/lib/index.js) for implementation details.

## License

[MIT](LICENSE). Not affiliated with DeepSeek or xAI;
[grok-build](https://github.com/xai-org/grok-build) is an interaction design
reference, and [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
is the runtime substrate.

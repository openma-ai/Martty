# dsh-tui

[中文](README.md) | [English](README.en.md)

[![npm](https://img.shields.io/npm/v/%40openma%2Fdeepseek-harness-tui)](https://www.npmjs.com/package/@openma/deepseek-harness-tui)
[![Package and publish npm](https://github.com/openma-ai/deepseek-harness-tui/actions/workflows/package-npm.yml/badge.svg)](https://github.com/openma-ai/deepseek-harness-tui/actions/workflows/package-npm.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A terminal-native agent UI for DeepSeek Harness. Follow streamed reasoning,
tool calls, subagents, token usage, and durable sessions in one Rust/ratatui
interface. Run it as an official `dsh` profile plugin or connect it directly to
the SDK JSON-RPC runtime.

> **v0.1.1** · The official package supports macOS on Apple Silicon and Intel,
> Linux x64, and Windows x64. The current integration baseline is
> `dsh 0.1.0-rc.6`.

![A DeepSeek Harness session in dsh-tui](assets/screenshots/banner.jpg)

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

- Streams reasoning, assistant text, tool arguments and results, plugin context,
  and subagent lifecycles.
- Queues follow-ups during a turn; standalone mode can also interrupt and send
  the next message immediately.
- Keeps durable JSONL sessions managed through `/new`, `/resume`, and
  `--session-id`.
- Uses the host dsh model catalog, agent presets, permission presets, providers,
  and credentials.
- Includes dark and light DeepSeek Web UI-inspired themes, narrow-terminal
  layouts, and mouse interaction.
- Copies through native clipboard tools, the tmux buffer, or OSC 52 for local,
  tmux, and SSH sessions.

![Plugin context, tool calls, and a queued follow-up](assets/screenshots/plugin-turn.jpg)

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
| `alt+enter` | Standalone: interrupt and send next; plugin: queue |
| `esc` | Standalone: interrupt and preserve the draft; press twice while idle to clear |
| `ctrl+c` | Clear the draft, then interrupt; press twice to quit |
| `/` | Open the command menu and filter by prefix |
| `/model` · `/mode` | Pick a model or agent preset; the full host catalog needs plugin mode |
| `/permission` · `shift+tab` | Pick or cycle permission presets; plugin mode only |
| `/effort` · `/plan` | Set reasoning effort or pass plan mode to the host |
| `ctrl+e` · `ctrl+t` | Expand output · toggle the theme |
| `pgup/pgdn` · `ctrl+u/d` | Scroll; `end` follows the live tail |
| Mouse drag | Copy on release; double-click a word; `shift+drag` uses native selection |
| `!cmd` | Run a local shell command on the client, outside the agent |

Use `/help` for commands and `/keys` for the complete shortcut list.

<p align="center">
  <img src="assets/screenshots/commands.jpg" width="536"
       alt="The dsh-tui slash command menu" />
</p>

<details>
<summary><strong>Composer pet: /liang 🤫</strong></summary>

`/liang` places a small pixel-art companion beside the composer: quiet while
idle, typing at a tiny terminal during a turn. Terminals with kitty graphics
protocol support, including Ghostty, Kitty, and WezTerm, get RGBA sprites;
others fall back to a half-block whale. The pet hides below 60 columns.

Use `/liang on` or `/liang off` to control it explicitly.

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

# dsh-tui — DeepSeek Build

A [grok-build](https://github.com/xai-org/grok-build)-style terminal agent UI
for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness),
written in Rust (ratatui). It speaks the harness **SDK JSON-RPC stdio
protocol directly** — no Python SDK in the loop — and skins the whole thing
in the exact DeepSeek Web UI palette, whale included.

```
        ▄▄████████████████████▄        ▄███▄▄         ▄█
      ▄████████████████████████▄       ██████▄ ▄▄▄▄████
     ████████████████████████████▄    ▀████████████▀
    ▄███▀▀▀▀▀▀████████████████████▄▄ █████▀
    ████         ▀▀███████████▀ ▀▀████████████        ██████╗ ███████╗ ···
    ████            ▀▀█████████▀█  ▀██████████        DEEPSEEK · B U I L D
    ▀████               ▀█████████▄▄▄████████
      ▀█████▄       ▄▄▄     ▀████████████▀            深度求索 · 探索未至之境
        ▀███████▄▄▄▄██████▄▄  ▀████████▄▄             everything is a plugin
           ▀▀████████████████▄▄▄█▀██████▀
```

The whale is rasterized by `scripts/gen_logo.py` from the actual favicon SVG
shipped by the harness Web UI; the theme tokens in `src/theme.rs` are a 1:1
mapping of `--dsw-*` variables from the Web UI's `design-platform.css`
(dark and light, `/theme` toggles).

## What it shows

Everything the Web UI surfaces for a session, live from the event stream:
streamed reasoning (collapsible `✻ thought · 3.2s · 14 lines`), streamed
assistant text, every tool call with arguments and results (`bash`,
`str_replace_editor`, …), injected plugin context, subagent lifecycles,
token usage incl. cache hits, turn end reasons, and runtime state.

## Interactions (grok-build homage)

| Key | Behavior |
|---|---|
| `enter` | send · **queues a follow-up while a turn runs** (protocol-native inbox) |
| `alt+enter` | send-now: interrupt the turn, run this next |
| `esc` | interrupt (draft survives) · idle: `esc esc` clears the draft |
| `ctrl+c` | clear draft first · interrupt · `ctrl+c ctrl+c` quits |
| `↑` | prompt history (on an empty prompt) |
| `!cmd` | run a local shell command (client-side, not the agent) |
| `/` | slash menu with prefix filtering (`/help /new /model /mode /theme …`) |
| `/mode` | agent mode picker — standard · code · minimal · creator (the host `agentPresets` roster in plugin mode; composed on a session's first prompt) |
| `ctrl+m` | model picker · `ctrl+e` expand all thoughts/tools · `ctrl+t` theme |
| `pgup/pgdn` `ctrl+u/d` | scroll · mouse wheel works · `end` follows the tail |

## Install & run

Needs the harness runtime binary (`dsh-jsonrpc-agent`) — easiest source is
the Python wheel:

```sh
uv venv .venv && uv pip install --python .venv/bin/python deepseek-harness-runtime-bin
export DEEPSEEK_API_KEY=sk-...
cargo run --release            # or: target/release/dsh-tui
```

Runtime discovery order: `--runtime-bin` → `$DSH_RUNTIME_BIN` → a
`.venv`/`venv` wheel near the workspace/exe → `$DSB_HOME` →
`~/.deepseek-harness-tui` → `PATH`. No key? Try the full UI with scripted
turns: `dsh-tui --demo`.

```sh
dsh-tui --check-runtime        # raw protocol handshake, no TUI
dsh-tui --dump-frame 100x40    # render one demo frame as plain text
```

### npm (local package)

```sh
bash scripts/build-npm.sh
npm i -g ./dist/deepseek-ai-harness-tui-0.1.0.tgz   # installs dsh-tui + dsb
```

### As a dsh plugin (profile bundle)

The same npm package doubles as a Cordis bundle for the official `dsh` CLI:

```sh
dsh plugin --profile tui add @deepseek-ai-harness/tui
dsh --profile tui
```

`npm/lib/index.js` spawns the native binary on the host TTY and mounts the
official `@deepseek-ai/dsh-sdk-jsonrpc-server` with its transport redirected
over pipe fds 3/4 (`dsh-tui --attach-fds`), so the TUI speaks the identical
protocol in both modes while agent, tools, providers, and persistence come
from the dsh profile. Wire-tested by `scripts/test-attach.mjs`. In plugin
mode, mid-turn interrupt is not available (the host owns the turn).

## Protocol

NDJSON JSON-RPC 2.0 over stdio (`src/proto.rs`):
requests `initialize` / `session/prompt` / `shutdown`; notifications
`session.event` (persistence-catalog events: `assistant/chunk`, `tool/call`,
`tool/result`, `turn/*`, `user/message`, …), `session.status`,
`subagent.started/finished`. Esc-interrupt in standalone mode kills the
runtime process — the durable JSONL session survives and the next prompt
respawns it with the same session id.

## Layout

```
src/proto.rs       NDJSON JSON-RPC client (spawn + attach transports)
src/runtime.rs     runtime binary / cordis config discovery
src/controller.rs  lifecycle thread: spawn, initialize, prompt, interrupt
src/events.rs      notifications → UiEvents (unit-tested)
src/transcript.rs  scrollback cells + wrapping + styling
src/app.rs         state machine & keys        src/ui.rs  drawing
src/theme.rs       Web UI design tokens        src/logo.rs + logo_data.rs
src/demo.rs        scripted wire-identical turns for --demo
npm/               dual-mode package (CLI shim + dsh bundle plugin)
scripts/           gen_logo.py · build-npm.sh · test-attach.mjs
```

MIT. Not affiliated with DeepSeek or xAI; grok-build is the interaction
reference, deepseek-harness is the substrate.

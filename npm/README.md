<p align="center">
  <img src="https://raw.githubusercontent.com/openma-ai/deepseek-harness-tui/main/assets/tui-lockup.svg" width="520" alt="DeepSeek Harness TUI" />
</p>

<p align="center">
  Terminal-native ACP client UI with a Cordis client plugin tree.
</p>

---

Native binaries are packaged for macOS arm64, macOS x64, Linux x64, and Windows
x64. Requires Node.js 18+.

## Recommended

```sh
npm install --global @openma/deepseek-harness-tui
dsh plugin --profile tui add @openma/deepseek-harness-tui
dsh --profile tui
```

The profile Host mounts the ACP plugin on Base, then starts a separate TUI
Client process over standard ACP stdin/stdout. For standalone use, run
`dsh-tui` and use `--agent <cmd>` plus repeated `--agent-arg <arg>` for another
ACP server. The Node Client process owns a Cordis tree and starts the Rust painter. A sibling
`tui-cordis-client-runner` publishes TUI Client capabilities and evaluates
approved `code.client` packages from `dsh-tool-cordis` against that client tree.

The package carries ACP as a runtime dependency and exports its Creator Host
overlay internally. The profile bundle mounts both on the Host Base tree;
neither enters the Client tree. Creator adds TUI plugin guidance to the
existing `cordis` preset and does not use ACP for skill registration.

Node and Rust use inherited pipes on Unix and an authenticated loopback TCP
socket on Windows. This compositor channel carries theme/render data only;
agent traffic remains ACP.

## Demo

```sh
dsh-tui --demo
dsh-tui --demo-skin
```

`dsh-tui` is the primary command. `dsb` is a compatibility alias.

## Highlights

- Streamed reasoning, replies, tool activity, subagents, run state, and
  token/cache metrics in one terminal timeline.
- Agent-advertised models, compositions, permissions, authentication methods,
  and invocable skills in one searchable slash menu.
- Up to eight editable image chips per prompt, with clipboard/file staging and
  metadata previews on kitty-capable terminals.
- Terminal-aware Markdown for headings, lists, quotes, code, emphasis, links,
  and mixed CJK/Latin text.
- Durable sessions, queued follow-ups, immediate steering, readline editing,
  platform-native modifier bindings, mouse selection, and inline-expanded tools.
- Dark/light themes, clipboard routing for local, tmux, and SSH sessions, plus
  the optional `/liang` pixel companion.
- A root `chrome.right` plugin rail for validated TuiNode trees, with live
  update/unload and Client inspect support for Creator-authored plugins.

The demo needs no API key or agent. Run `dsh-tui --help` for agent, model,
credential, session, theme, and demo options.

## Supported platforms

| Node platform key | Binary |
|---|---|
| `darwin-arm64` | `vendor/darwin-arm64/dsh-tui` |
| `darwin-x64` | `vendor/darwin-x64/dsh-tui` |
| `linux-x64` | `vendor/linux-x64/dsh-tui` |
| `win32-x64` | `vendor/win32-x64/dsh-tui.exe` |

If installation succeeds but launch reports `no native binary for ...`, confirm
that you installed the latest version and that your platform appears above.

## Uninstall

```sh
npm uninstall --global @openma/deepseek-harness-tui
```

Source, screenshots, development commands, and architecture notes live in the
[GitHub repository](https://github.com/openma-ai/deepseek-harness-tui).

[MIT](https://github.com/openma-ai/deepseek-harness-tui/blob/main/LICENSE).

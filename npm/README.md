# @openma/deepseek-harness-tui

Terminal-native UI for DeepSeek Harness, distributed as both a `dsh` profile
bundle and a standalone `dsh-tui` command.

Native binaries are packaged for macOS arm64, macOS x64, Linux x64, and Windows
x64. Requires Node.js 18+; plugin installation requires pnpm 10+ and is tested
with `dsh 0.1.0-rc.6`.

## Recommended: dsh profile plugin

```sh
dsh plugin --profile tui add @openma/deepseek-harness-tui
dsh --profile tui
```

The install command does not need `-w`. The profile supplies agents, tools,
providers, credentials, and durable sessions; the package supplies the native
TUI and its JSON-RPC bridge. Harness packages are resolved from the running
`dsh`, not reinstalled into the profile.

## Demo and standalone CLI

```sh
npm install --global @openma/deepseek-harness-tui
dsh-tui --demo
```

`dsh-tui` is the primary command. `dsb` is a compatibility alias.

The demo needs no API key or runtime. Normal standalone sessions additionally
need `dsh-jsonrpc-agent`, discovered from a nearby SDK virtual environment,
`DSH_RUNTIME_BIN`, `--runtime-bin`, or `PATH`:

```sh
python -m venv .venv
.venv/bin/pip install deepseek-harness-sdk
dsh-tui --workspace .
```

Run `dsh-tui --help` for runtime, model, credential, session, theme, and demo
options.

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

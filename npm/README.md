<p align="center">
  <img src="https://raw.githubusercontent.com/openma-ai/Martty/main/assets/martty-lockup.svg" width="650" alt="Martty terminal lockup" />
</p>

<h1 align="center">Martty</h1>

<p align="center">
  A DSH-first terminal with the same Cordis plugin model, plus support for other ACP agents.
</p>

---

Native binaries are packaged for macOS arm64, macOS x64, Linux x64, and Windows
x64. Requires Node.js 18+.

> **Rename note:** the project and recommended npm package have moved from
> DeepSeek Harness TUI / `@openma/deepseek-harness-tui` to **Martty** / `martty`.
> The `dsh-tui` command, configuration, and session data remain compatible.
> The production profile is now `martty`; legacy `tui` profiles
> continue to work.

## Global Agent TUI

```sh
npm install --global martty
martty
```

Martty directly depends on our `@openma/deepseek-harness-acp` implementation.
It resolves this built-in ACP from its own dependency graph, connects DSH by
default, and requires no separate ACP installation. For another ACP server,
configure `DSH_TUI_AGENT`; Cordis embedding can instead provide `config.agent`
or `config.stream`. `--agent` is only the standalone CLI override for the same
connection configuration.

## Recommended

```sh
npm install --global @deepseek-ai/dsh
dsh plugin --profile martty add martty@latest
dsh --profile martty
```

The profile command is the recommended install and upgrade path. It creates a
missing profile and installs the TUI plus its ACP dependency; no global
`martty` installation is required.

## Migrating from DeepSeek Harness TUI / the scoped package

`martty` is the recommended package name. Starting with `0.2.13`, it and the
legacy `@openma/deepseek-harness-tui` package are published by the same CI run
with identical versions and artifacts. Existing scoped-package installs remain
supported. To switch package names:

```sh
dsh plugin --profile tui remove @openma/deepseek-harness-tui
dsh plugin --profile martty add martty@latest
```

The recommended profile changes from `tui` to `martty`; runtime behavior,
config, and session data remain unchanged. `dsh-tui` and legacy `tui`
profiles remain compatible.

The profile Host mounts the ACP plugin on Base, then starts a separate TUI
Client process over standard ACP stdin/stdout. For standalone use, run
`martty` and use `--agent <cmd>` plus repeated `--agent-arg <arg>` for another
ACP server. Named standalone harnesses can also be discovered, saved, and selected:

```sh
martty harness list
martty harness find
martty harness add local --command local-acp --arg --stdio
martty harness use local
```

`harness find` fetches the official ACP Registry from
`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` and supplements it
with local `*-acp` / `*_acp` PATH discovery; it is not an npm package search or an immediate
switch. `npx` / `uvx` entries become launch recipes without implicit `--yes` or
`--prefer-offline` flags. Selecting a binary distribution downloads it into
`$MARTTY_HOME/bin/<id>/<version>/<platform>`, verifies its declared SHA-256, and configures
the resulting executable without modifying the system PATH. The same flow is available in
the TUI with `/harness find`; manual `--command` remains available for unregistered agents.
Run `harness find` without a query to browse the full catalog; users do not need to know an
agent name in advance. Query text is only an optional filter, and every unconfigured CLI result
prints the corresponding `martty harness add <id>` command.

The same registry is available through three entry points: edit
`$MARTTY_HOME/settings.json`, use `martty harness`, or run `/harness` (or
`/harness <id>`) inside the TUI. In a standalone TUI, `/harness` immediately
switches before the current session has started. After the first prompt has
started a session, or an existing session was loaded, it first confirms that
the selected Harness will start another session; the current session remains
available in `/session`. Confirming stops the current ACP child and initializes
the selected Harness without closing Martty, then immediately sends `session/new`
to bind an empty session. This does not start a model turn. It never carries one session
across Harnesses. The settings and CLI entry points select the Harness for the
next standalone launch; a profile-owned Host cannot be replaced by its Client.

Every selection is stored in `$MARTTY_HOME/settings.json` as `defaultHarness`. CLI/settings
selection takes effect on the next standalone launch; TUI selection also
updates the running standalone process immediately. Standalone startup only
initializes the selected Harness and binds an empty ACP session before showing Ready.
The product's internal `forcedHarness` initialization value is empty by
default and is not a CLI/user startup argument. When the product supplies it,
startup selects it before consulting the persisted `defaultHarness`; when
empty, startup uses that saved default and otherwise the bundled fallback.
`/harness` updates the saved `defaultHarness`, never the product forced value.
`--agent` and `DSH_TUI_AGENT` remain higher startup priorities.
None of these replace the Host-owned runtime or session of
`dsh --profile martty`.

The Node Client process owns a Cordis tree and starts the Rust painter. A sibling
`tui-cordis-client-runner` publishes TUI Client capabilities and evaluates
approved `code.client` packages from `dsh-tool-cordis` against that client tree.

ACP is a runtime dependency of this package. On a profile that already carries
a different version through the standard ACP bundle, the package manager may
retain both copies, but the TUI bundle replaces that surface's rows and mounts
only the plugin resolved from TUI's own dependency graph. The supported profile
shape never runs two ACP surfaces for one TUI.

The package carries ACP as a runtime dependency and exports its Creator Host
overlay internally. The profile bundle mounts both on the Host Base tree;
neither enters the Client tree. Creator adds TUI plugin guidance to the
existing `cordis` preset and does not use ACP for skill registration.
Creator-authored UI Presets and Theme Plugins preview as process-local dynamic
Packages, then persist explicitly under `$MARTTY_HOME/plugins` through
`tui_plugin_save`; `tui_plugin_read` resumes authoring after restart.

`MARTTY_HOME` resolves explicitly first, then to `$DSH_HOME/.martty`, then to
`~/.martty`. UI choices live in `$MARTTY_HOME/settings.json`. On first use,
legacy Creator artifacts and settings are copied forward without deleting or
overwriting the old files.

Third-party TUI plugins remain ordinary installed packages. Their Host-side
registrar contributes an absolute Client module entry to `tuiClientPlugins`;
the Host runner serializes only that directory into the separate Client
process. Package entries and Creator artifacts share one Client lifecycle
manager, with installed packages winning same-id conflicts.

Node and Rust use inherited pipes on Unix and an authenticated loopback TCP
socket on Windows. This compositor channel carries theme/render data only;
agent traffic remains ACP.

## Demo

```sh
martty --demo
martty --demo-skin
```

Without a global installation:

```sh
npx --yes martty --demo
```

The profile path remains recommended for an existing DSH installation that
wants standard plugin management and a durable `martty` profile. `martty` is the
primary command; `dsh-tui` remains a compatibility alias.

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
- Persistent UI Presets selected with the native `/ui` picker (or `/ui <id>`): builtin Martty and the classic
  DeepSeek Harness composition, both assembled from independent welcome Hero
  and information slots without writing to the transcript.
- A root `chrome.right` plugin rail for validated TuiNode trees, with live
  update/unload and Client inspect support for Creator-authored plugins.
- Lifecycle-owned local commands and native slider overlays, plus transactions
  over the current Session's standard ACP `configOptions`.
- Dynamic `code.host` + `code.client` Packages with inspect/run lifecycle and
  package-private Host/Client RPC over a negotiated `_dsh/cordis/*` ACP
  extension. Ordinary ACP agents remain usable when they do not advertise it.

## Plugin surface

TUI extensions are ordinary Cordis plugins on the Node Client tree. A single
plugin may register a theme, `chrome.right` nodes, slash commands, overlays,
timers, Session config transactions, and Host/Client RPC; all contributions
leave together when its fiber stops or `/theme` replaces the selected Theme
Plugin. Plugins submit semantic data and never receive the TTY, Ratatui,
absolute coordinates, compositor fds, or private transport methods.

The versioned contract and examples live in the repository's
[plugin API](https://github.com/openma-ai/Martty/blob/main/docs/plugins.en.md).

The demo needs no API key or agent. Run `martty --help` for agent, model,
credential, session, theme, and demo options.

## Supported platforms

| Node platform key | Binary |
|---|---|
| `darwin-arm64` | `vendor/darwin-arm64/martty` |
| `darwin-x64` | `vendor/darwin-x64/martty` |
| `linux-x64` | `vendor/linux-x64/martty` |
| `linux-arm64` | `vendor/linux-arm64/martty` |
| `win32-x64` | `vendor/win32-x64/martty.exe` |

If installation succeeds but launch reports `no native binary for ...`, confirm
that you installed the latest version and that your platform appears above.

## Uninstall

```sh
npm uninstall --global martty
```

Use `npm uninstall --global @openma/deepseek-harness-tui` instead for a legacy
global installation.

Source, screenshots, development commands, and architecture notes live in the
[GitHub repository](https://github.com/openma-ai/Martty).

[MIT](https://github.com/openma-ai/Martty/blob/main/LICENSE).

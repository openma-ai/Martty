# Manage ACP harnesses

Martty is a terminal client for DeepSeek Harness and other ACP coding agents. A harness is the agent program it connects to, not a model name. The ACP Registry lets you browse available programs before choosing a launch command, without guessing package names or Registry IDs.

Installation prepares files, configuration saves a launch recipe, and switching connects the agent and creates a session. A configured harness is not necessarily authenticated or ready to make model requests: external dependencies and account access may still be required.

## Find an agent in the ACP Registry

After installing Martty, run this in your system terminal:

```sh
martty harness find
```

The catalog combines the official ACP Registry, local programs, and saved configurations. Martty starts with its local cache or bundled snapshot. Run `martty harness find --refresh` for a fresh catalog; a failed refresh leaves the offline catalog available. Downloading a new package still requires network access.

Inside Martty, enter `/harness` and choose **+ Add Harness…** to browse or filter by name. Installed or configured entries are grouped separately from downloadable entries. The current harness is marked `(current)`, placed first, and cannot be selected again. Names such as Codex, Claude Agent, and Google Antigravity identify catalog entries; CLI operations use the ID printed in the results.

## Configure the next launch from the CLI

Copy the add command from your chosen `find` result. Replace `<id>` below with that result's ID; do not type the angle brackets or substitute its display name.

```sh
martty harness add <id>
martty harness use <id>
martty
```

`add` prepares and saves configuration. `use` sets the default for the next standalone launch. Neither changes an already running Martty nor starts authentication. The final `martty` command starts the agent, initializes ACP, and creates an empty session without sending a model prompt. Explicit launch overrides or a host-enforced configuration may take precedence over the saved default.

Run `martty harness list` to inspect configuration. If your shell cannot find `martty`, use `npx --yes martty harness find`. From the source repository root, `node npm/bin/martty.js harness find` also works. Use `martty harness --help` for available options.

## Local programs, npx, uvx, and binary packages

Martty can reuse a detected local executable without changing your system PATH. Registry npx/uvx entries use the supplied launch recipe. Saving that recipe does not mean the package is already downloaded; a CLI-configured package may download on its first launch.

Binary distributions are selected for the operating system and architecture, verified with SHA-256, and installed in a Martty-owned directory such as `~/.martty/bin/<id>/<version>/<platform>` on macOS. Windows uses its corresponding user directory and platform package; adding the installation to the system PATH is not required.

If npx or uvx is missing, prepare Node.js/npm or uv. A compatible binary distribution can be used when the entry provides one. Some Registry entries are adapters that still need another CLI. An `executable not found` error means that dependency must be resolved; installing the adapter alone does not prove the agent is ready.

## Switch from the TUI after downloading

In `/harness`, select another configured entry and press Enter to switch the running harness. An empty session switches directly. If you have sent a prompt or restored history, Martty first asks to start a new session. Switching runs ACP `initialize` and `session/new`, without feeding the old conversation to the new harness.

When an added entry needs a download, its progress stays in a panel. Esc hides the panel while the download continues for as long as Martty remains open. Completion or failure produces a notice. Installation saves configuration but does not switch in the background. **Enter switch** on the completion panel uses the normal switching flow; **Esc close** only dismisses the panel.

The saved default changes only after ACP is ready. A failed connection leaves the previous default intact. Switching harnesses differs from navigating tabs within one connection; see [sessions and message queues](sessions.en.md).

## Authenticate and inspect connection errors

When the agent requires sign-in, run `/auth` inside Martty and follow its advertised browser, form, or terminal method. The agent's own `/login` command remains an agent command. Browser completion is not the final result: Martty follows the ACP `authenticate` response and subsequent session result. Use `/status` to inspect the connection.

Failure panels display the agent's reason, structured error data, and captured stderr when available. Resolve the reported missing executable, dependency, account restriction, or server rejection before pressing Enter to retry. Esc closes the panel so you can choose another harness. A connection failure is not a download failure, and repeated sign-in cannot fix every cause.

## Remove configuration and private installation files

Check the ID first and exit other Martty instances still using that harness. Preview the cleanup scope before deleting files:

```sh
martty harness remove <id> --cleanup --dry-run
```

`martty harness remove <id>` asks for confirmation, removes configuration, and clears a default pointing to it. Add `--cleanup` to also remove its exclusively owned private binary installation. Deleted installation files must be downloaded again. Global programs, shared npx/uvx caches, history, and credentials are retained; unsafe or shared cleanup targets are rejected.

In the TUI, select a saved, non-current entry and press Delete. Choose configuration-only removal or private-installation cleanup, then confirm the full paths. Esc returns one level within the removal flow without deleting anything.

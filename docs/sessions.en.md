# Sessions, history, and message queues

Martty can open multiple session tabs within one ACP connection. Each tab keeps its own draft, staged images, queue, and scroll position. Restoring history depends on the agent's ACP capabilities and available session records. Enter the slash commands below in Martty's composer, not in your system shell.

## Create a session and navigate tabs

Use `/new` to create a session. With two or more tabs open, use the tab strip or these keyboard commands:

```text
/session prev
/session next
/session view
```

The first two commands navigate adjacent tabs; `/session view` shows the current session and runtime information. Unsent text, images, and queued messages stay with their tab. A background shell command returns its output to the session that started it, even if you navigate elsewhere.

Tab navigation does not replace the agent program. To change harnesses, use `/harness`, which follows a new ACP connection and empty-session flow. See [harness installation and switching](harness-management.en.md).

## Resume recent sessions

`/resume` lists the 50 most recent durable sessions by default. Supply a number to limit the list, or a non-numeric session ID or prefix to select a target:

```text
/resume 10
/resume <id>
```

Replace `<id>` with an actual session ID. A number is a list limit, not a row selection. Martty uses the agent's session listing when available and falls back to local JSONL records when needed.

If the agent advertises `session/resume`, long sessions can continue without replaying the entire transcript. Older agents using `session/load` remain supported. Without either capability, replaying a local transcript is not proof that the remote agent has restored the same context. Model, permission, and history-restoration support depend on the agent and its responses.

## Fork the current session

`/fork` sends standard ACP `session/fork` for the current session. The parameters match `session/load`: the current `cwd` and `mcpServers` (an empty array when Martty has no client-side MCP server list), plus the same extra directories when the agent advertises `additionalDirectories`. The fork covers the whole session. The request carries no message id and no `_meta`.

The only gate is `agentCapabilities.sessionCapabilities.fork` from `initialize`. The standard form is `{}`. Omitted, `null`, and any non-object do not count. Martty does not consult the harness name or version. When the capability is present, `/fork` in the `/` menu runs. When it is absent, the row stays visible and disabled, with the reason in its description.

On success Martty opens the new session with the existing multi-tab flow and switches to it. The original session stays on its tab. An agent error is shown on the new tab; the original session is left as it was. `/fork` takes no session name. `/new`'s optional argument is a local placeholder id, and `/resume`'s argument is a list limit or a session id.

## Queue follow-ups or steer immediately

During a running turn, Enter queues a follow-up for the current session. Use **Ctrl+Enter** to steer the active agent immediately; **⌘⏎** also works on macOS. **Ctrl+X** cuts a selection and is not the steer shortcut.

Press **Alt+↑** to select a queue entry, use ↑ / ↓ to navigate, Enter to edit, and Ctrl+D to delete it. With an empty composer and a nonempty queue, Enter can send the first queued message immediately. Esc interrupts the current turn while preserving the composer draft; it does not close the session.

## Close a tab without deleting the remote session

`/close` closes the current tab and discards its local draft, staged images, and prompt queue. The last remaining tab cannot close; use `/new` to open another first.

ACP has no corresponding `session/close` request. Closing a tab therefore does not cancel a remote task or delete durable history, and a running remote turn may still finish. Use Esc first if you intend to interrupt the task. Available durable sessions can be reopened with `/resume`, but this does not recover unsent drafts or queues discarded when the tab closed.

## Additional workspace directories

Repeat `--add-dir <absolute-path>` at launch to give the session roots besides the workspace. Each path must be an existing directory. Filesystem roots and `$HOME` are rejected, and an entry equal to the workspace is dropped.

Martty includes those directories on `session/new`, `session/load`, `session/resume`, and `session/fork` only when the agent advertises `sessionCapabilities.additionalDirectories` during `initialize`. Without that capability the field is omitted and the UI says so. With no `--add-dir`, the field is omitted as well. The session tab, `/status`, and `/session` show the directories that were sent. The `/resume` list marks `additionalDirectories` when the agent reports them.

## Check the active state

Use `/status` for connection, session, and turn state, and `/keys` for the full shortcut reference. Models and authentication methods come from the active agent. Do not infer the current state from a previous tab's model or another harness's sign-in method.

## Slash names that collide

Builtin commands keep their bare names. A command delivered by `available_commands_update` with the same name is no longer dropped. The menu shows it with the agent name as a prefix (for example `pi-acp /model`). Choosing that row sends the original `/model …` line to the agent as a prompt. Enter on the builtin row still runs Martty's command. This applies to every builtin, including `/model` and `/session`, which already collide with pi-acp, and `/fork`. Client plugin commands never become prompts, so a colliding client command still yields the bare name to the builtin.

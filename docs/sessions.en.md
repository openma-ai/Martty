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

## Queue follow-ups or steer immediately

During a running turn, Enter queues a follow-up for the current session. Use **Ctrl+Enter** to steer the active agent immediately; **⌘⏎** also works on macOS. **Ctrl+X** cuts a selection and is not the steer shortcut.

Press **Alt+↑** to select a queue entry, use ↑ / ↓ to navigate, Enter to edit, and Ctrl+D to delete it. With an empty composer and a nonempty queue, Enter can send the first queued message immediately. Esc interrupts the current turn while preserving the composer draft; it does not close the session.

## Close a tab without deleting the remote session

`/close` closes the current tab and discards its local draft, staged images, and prompt queue. The last remaining tab cannot close; use `/new` to open another first.

ACP has no corresponding `session/close` request. Closing a tab therefore does not cancel a remote task or delete durable history, and a running remote turn may still finish. Use Esc first if you intend to interrupt the task. Available durable sessions can be reopened with `/resume`, but this does not recover unsent drafts or queues discarded when the tab closed.

## Check the active state

Use `/status` for connection, session, and turn state, and `/keys` for the full shortcut reference. Models and authentication methods come from the active agent. Do not infer the current state from a previous tab's model or another harness's sign-in method.

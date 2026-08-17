# Migration plan

Goal: preserve the current TUI experience while moving extension capability
onto an independent Cordis client tree. ACP owns the agent control plane; the
private compositor channel owns Node-to-Rust render data.

## Complete — ACP client control plane

- The profile path mounts the ACP plugin on the Host Base tree and connects a
  separate TUI Client process over standard stdin/stdout; standalone
  `acp-client` still spawns or attaches any ACP agent.
- Rust uses the official `agent-client-protocol` crate for initialize, auth,
  sessions, prompts, permission, config options, and updates.
- The Agent and TUI Client Cordis trees live in different processes and communicate only by ACP.
- Both sides negotiate `initialize` `_meta.dsh.cordis.protocol = 0`;
  inspect/run/lifecycle uses `_dsh/cordis/*`, while painter capabilities use
  `_dsh/cordis/tui/*`.
- Every custom wire method is an underscore-prefixed ACP Extension
  Request/Notification; the Node mux intercepts theme/slots/commands/overlay
  so they never enter the agent stream.
- `tui-cordis-client-runner` is an independent client-tree sibling; the shell
  only binds and forwards its transport.

## Complete — Theme plugin POC

- `tui-theme` is a real Cordis service plugin providing `ctx.tuiTheme`.
- Palette packs are sibling plugins that inject `tuiTheme` and call `register`.
- The 18 token names are closed; each dark/light pack is complete `#RRGGBB` data.
- Node sends `_dsh/cordis/tui/theme/update` notifications to Rust; plugins
  cannot access the transport method.
- `--demo` keeps builtin `default`; `--demo-skin` mounts and activates gallery `ember`.

## Complete — Creator skill overlay

- The TUI package's internal Host overlay registers `tui-plugin-development` in the upstream
  `cordis` preset's standing scope without copying or editing its composition.
- The skill is visible only to Creator agents and disappears when the bundle unloads.
- Skill loading does not depend on ACP; Client inspect/run remains the runtime path for TUI capabilities and dynamic packages.

## Complete — root right-rail slot POC

- `tui-slots` is a real Cordis service declaring root/list `chrome.right`.
- Static and dynamic plugins use `inject` / `register` for any valid `TuiNode`
  tree; `panel.update` replaces it live and fiber unload retracts it.
- The registry aggregates contributions, namespaces node ids, and emits a full
  monotonic-revision snapshot over `_dsh/cordis/tui/slots/update`.
- Rust validates and paints a separate right rail. Removing the last
  contribution restores full width; narrow terminals only hide it temporarily.
- Client inspect `Slots.list` teaches Creator-authored `code.client`; the
  `@openma/deepseek-harness-tui/right-demo` gallery is not mounted by default.

Verification:

```sh
make rust-test
cd npm && npm test
cd ../host && npm test
dsh-tui --demo
dsh-tui --demo-skin
```

## Next — conversation slots

Reuse the existing `tuiSlots` service and [tui-node.v0.schema.json](tui-node.v0.schema.json):

1. The shell declares `conversation.chat`.
2. Independent plugins call `tuiSlots.inject(slot, () => tuiSlots.register(...))`.
3. The registry aggregates contributions into a full snapshot with monotonic `rev`.
4. Node sends the snapshot to Rust over `_dsh/cordis/tui/slots/update`.
5. Rust validates it and preserves expand, selection, and scroll state by stable node id.

The first version may append conversation nodes only. It cannot replace the
composer or impersonate durable ACP session events. ACP `session/update`
remains the transcript source of truth.

## Later

- `/refresh`: rebuild Client plugin fibers without killing the agent or draft.
- Shell slots such as `chrome.status`: reuse the same registry/snapshot protocol.
- Optional JS painter: replace only the renderer, not plugin ABI, slot names, or tokens.

Published token names, node kinds, and slot names are append-only; changing
field meaning requires a protocol bump.

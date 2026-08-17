# Plugin API

Third parties extend the TUI only through the APIs on this page. Objects, fds, and methods not listed here are not provided. Make them unreachable in the host; do not document a honor-system ban.

**Open now:** palette packs and the root `chrome.right` rail. The rail accepts structured `TuiNode` trees and never becomes session history.

## Open: `tuiTheme`

`palette` must match [tui-palette.v0.schema.json](tui-palette.v0.schema.json).

| Field | Rule |
|---|---|
| `id` | Stable string; colliding with an existing id fails `register` |
| `label` | Short name for `/theme` |
| `dark` / `light` | Each is a **complete** token → `#RRGGBB` map. Missing any token, or any unknown name, fails |

Closed token names:

`bg` `surface` `panel` `fg` `fg_secondary` `fg_tertiary` `caption` `brand` `brand_soft` `bubble_bg` `bubble_fg` `border` `code_bg` `ok` `warn` `err` `hint` `chip_bg`

Builtin `default` remains the cold bluish skin and cannot be replaced or removed.
`ctrl+t` only toggles dark/light inside the current theme. `/theme` is a special
**single-select Theme Plugin switch**: selecting an id starts its owning Client
Plugin and stops the previously selected Theme Plugin; selecting `default` stops
the current dynamic Theme Plugin.

A Theme Plugin may register a palette, commands, overlays, slots, timers,
transactions, and Package-private RPC together. None uses a theme condition or
subscribes to the active theme; they are ordinary contributions of one Cordis
Fiber. `/theme` replacement, stop, update, or unload retracts that whole Fiber.
Stopped Theme Plugin ids remain in the `/theme` catalog so they can be started
again later.

`register(palette, { activate: true })` is the immediate-preview path for a new
dynamic `cordis_run`. The Client runner places that run in the same `/theme` seat
and replaces the previous Theme Plugin; if the target fails, the previous Plugin
stays intact. The dynamic Client facade exposes `register` only, not `activate()`,
`active()`, or theme `subscribe()`, so code cannot bypass the Plugin switch.

This changes the TUI: composer, status, borders, bubbles, code blocks, tool cards, and the hint row all recolor. Layout, box-drawing characters, font size, and kitty pet sprites do not.

A Theme Plugin remains an ordinary Cordis Plugin: one `apply` registers independent
contributions through their services and one Fiber owns all disposers. `/theme`
only provides special single-select switching between those Plugins.

Cordis mode discovers this API through Client inspect: `Theme` on platform
`client` is the directory published by the TUI over ACP, not `tuiTheme` on the
Agent tree. `code.client` injects `tuiTheme` and registers its palette; the same
Plugin injects other services for its other contributions. Do not use Web
`theme.overrideTokens`, CSS variables, or theme conditions.

## Minimal plugin

```js
export const inject = ["tuiTheme"];

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register({
    id: "ember",
    label: "Ember",
    dark: { /* 18 tokens, see fixtures/demo-skin.v0.json */ },
    light: { /* 18 tokens */ },
  }));
}
```

For immediate preview, change the registration call to:

```js
ctx.tuiTheme.register(palette, { activate: true })
```

Do not subscribe to theme state or hand-write a return path; `/theme` replaces the
whole Plugin.

## Open: `tuiSlots` / `chrome.right`

The shell declares the root list slot `chrome.right`. A plugin waits with
`inject`, then `register`s any composition of the node kinds in
[tui-node.v0.schema.json](tui-node.v0.schema.json).

```js
export const inject = ['tuiSlots']
export function apply(ctx) {
  ctx.tuiSlots.inject('chrome.right', () => {
    const panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'build-monitor', order: 20 },
      [{ id: 'title', kind: 'markdown', text: '# Build monitor' }],
    )
    return panel.dispose
  })
}
```

`panel.update(nextNodes)` replaces the contribution immediately. Unload removes
it; the rail collapses when the last contribution disappears. Narrow terminals
temporarily hide the rail without deleting state. Dynamic Creator plugins query
`Slots.list` and inject `tuiSlots`, not Web React `ctx.slots`. The optional
`@openma/deepseek-harness-tui/right-demo` export is a rich gallery plugin.

### Host data and polling

A dynamic Package's Host half registers Package-private JSON methods with
`harness.handle(method, handler)`. Its Client half calls them with
`host.call(method, args)`. Arguments and results must be lossless JSON; omitting
`args` delivers `null` to the Host.

For periodic refresh, also inject `timer` and use the Plugin-owned
`ctx.interval`; never use a bare Node `setInterval`:

```js
return {
  inject: ['tuiSlots', 'timer'],
  apply(ctx) {
    const panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'monitor' },
      [{ id: 'loading', kind: 'notice', level: 'info', text: 'Loading…' }],
    )
    let refreshing = false
    const refresh = async () => {
      if (refreshing) return
      refreshing = true
      try {
        const state = await host.call('read-state')
        panel.update([{ id: 'state', kind: 'generic', title: 'State', body: state.text }])
      } catch (error) {
        panel.update([{ id: 'error', kind: 'notice', level: 'error', text: String(error) }])
      } finally {
        refreshing = false
      }
    }
    void refresh()
    ctx.interval(() => void refresh(), 1000)
  },
}
```

`host.call` uses the negotiated ACP `_dsh/cordis/plugin/invoke` Extension Request. Timers and Slots remain in the
Client tree and never become `session/update` or conversation history.

```yaml
- insert:
    - id: tui-right-demo
      name: '@openma/deepseek-harness-tui/right-demo'
```

## Not provided

Plugin modules do not receive: the TTY, stdin, raw mode, kitty escapes, ratatui, the JSON-RPC method table, fd 3/4, the `_dsh/cordis/tui/*` transport, full-screen rows/cols, a frame loop, or coordinates such as “bottom N rows”.

Nodes must not contain RGB; color only by token name. RGB appears only in a palette pack's `dark`/`light` tables.

## Load

Cordis has **no** parent pointer from one plugin instance onto another.
`tui-theme`, palette packs, `tui-cordis-client-runner`, and `tui-runner` are
**sibling inserts** in the profile.

The protocol that waits on the TUI capability is the service name:

1. `tui-theme` and `tui-slots` provide `ctx.tuiTheme` and `ctx.tuiSlots`.
2. `tui-cordis-client-runner` injects both, publishes inspect, and evaluates dynamic `code.client`.
3. Third-party plugins inject the service they use and stay PENDING until it exists.
4. Cordis owns static disposers; the Client runner owns dynamic contributions.

Do not stack third parties with `ctx.plugin(ember)` inside the runner. The
composition mounts third-party packages as siblings. Web
Web `ctx.slots` and terminal `ctx.tuiSlots` are different services.

Local packages use `dsh plugin add ./path` and load **package exports**. A newly inserted row needs a TUI restart or later `/refresh`. Hot-swap of an already-mounted palette ships by phase.

## Later

Conversation slots such as `conversation.chat` remain closed. ACP `session/update` stays the transcript source of truth.

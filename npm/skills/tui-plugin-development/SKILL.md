---
name: tui-plugin-development
description: Companion guidance for dynamic Cordis Plugins that touch the TUI: terminal themes, slots, local slash commands, native overlays, and current ACP Session config or Plan state. Load `cordis-plugin-development` first for the common Plugin model, then load this skill for the TUI half.
---

# Develop TUI Client Plugins

Load `cordis-plugin-development` first for the common dynamic-Plugin lifecycle,
Host/Client split, Package versions, approval, and repair flow. Load this skill
as its TUI companion whenever any requested behavior belongs to the terminal.
For that TUI half, the live TUI Providers and rules below replace generic Web
Slots, React, CSS, settings, and browser-theme assumptions. Keep the generic
skill for shared Cordis behavior and for any genuine Host or Web half.

## Fast path

1. Call `cordis_inspect_list` once. Do this before examining optional visual
   assets: asset contents do not determine which TUI Providers own the UI.
2. Query only the smallest Client Providers that own the requested behavior,
   using the exact methods shown by the list.
3. For an existing `@pluginId`, call `cordis_inspect_self` once; skip it for a
   new Plugin.
4. Write plain JavaScript in `code.client`. Add `code.host` only when the task
   genuinely owns Host data or operations.
5. Call `cordis_define` once, then `cordis_run` once with the returned exact
   ids. Use `run` for a new Plugin and `update` for a new Package replacing a
   current one.
6. If run reports `awaiting-approval` or `starting`, stop the Tool flow and let
   later state updates finish activation. Reply with one short status sentence;
   do not summarize the implementation while approval is pending.
7. After the later state update confirms activation, report success and stop.
   Do not re-query Providers merely to verify effects that activation already
   confirmed; query again only for a concrete runtime diagnostic.

The normal new-plugin trajectory is therefore one list, one query per needed
Provider, one define, and one run. Once inspect returns a sufficient live
contract, author immediately; do not re-prove it from generic Cordis concepts.

`define` stores an immutable Package; `run` activates it. Stop/update/unload
dispose the owning Client Plugin effects. Never depend on a value returned by
`apply()` as the disposer; register effects through the inspected Cordis APIs.

## Inspect is the authority

Treat the current list and query results as the complete Client API. If a
required capability is absent, report that exact gap before define. Do not:

- search source, installed packages, npm, or the web;
- reflect on runtime objects or function source;
- guess shapes and retry calls;
- define/run a probe Package;
- use a visible Slot such as `chrome.right` for diagnostics.

Use the narrowest capability:

| Need | Inspect family | Runtime service |
| --- | --- | --- |
| Palette registration/activation | `Theme` | `tuiTheme` |
| Persistent terminal content | `Slots` | `tuiSlots` |
| Local slash command | `Commands` | `tuiCommands` |
| Transient slider or node view | `Overlay` | `tuiOverlay` |
| Current ACP Session option | advertised config-option Provider | `acpSessionConfig` |
| Current structured ACP Plan | `Plans` | `acpSessionPlan` |
| Current ACP Session statistics | `Stats` | `acpSessionStats` |
| Current ACP run-state facts | `Status` | `acpSessionStatus` |

A transient control is not a side panel. When the user did not request
persistent content, do not query Slots or mount `chrome.right`.

## Compose independent capabilities

Follow the Web dynamic-Plugin model: a returned Client Plugin declares the
services it needs, and one `apply()` may contribute any combination of
features. Every registration belongs to that Plugin's Cordis fiber, so
stop/update/unload retracts all of its contributions together. Do not invent
cross-service option fields or a new bundle abstraction to couple two features.

Select Providers from the user-visible behavior, query each live contract, and
compose only in the Plugin code. The services do not imply one another:

- **Themes:** use the inspected theme-Plugin registration contract. `/theme`
  is a special single-select Plugin switch: selecting another entry starts its
  Plugin and stops the current Theme Plugin, so every contribution shares one
  unload. Register palette, commands, overlays, slots, and RPC unconditionally
  in that one Plugin. Never read/subscribe to active theme or implement a
  theme condition yourself. Immediate preview uses the inspected registration
  option and enters the same Plugin seat.
- **Commands:** register a local slash command. Registration publishes slash
  completion and keeps invocation out of the ACP prompt. Its availability is
  the registration's lifetime; Commands does not know about themes, overlays,
  config categories, or Slots.
- **Slots:** register persistent native `TuiNode` content only when the user
  asked for persistent shell UI. Use stable node ids and update the existing
  contribution instead of creating parallel panels. Select the live seat from
  `Slots.list`: all current seats aggregate contributors. Use
  `conversation.input.dock` for content needing its own line (it owns the
  single cap row and displaces the tip line while present) and keep
  `conversation.composer.dock` contributions compact.
- **Overlays:** open a transient native control only in response to the
  interaction that needs it. A command is one possible trigger, not part of
  the Overlay contract. Do not use a persistent Slot as a transient control.
- **ACP Session config:** resolve the live option through the inspected
  semantic category, or by id only when the Package is intentionally
  agent-specific. Use the inspected transaction when an interaction needs
  preview/commit/rollback; it owns write ordering, deduplication, first-winner
  finalization, and rollback of unfinished previews with the Plugin run.
- **ACP Session Plan:** consume `current()` / `subscribe()` when the requested UI
  reflects agent Plan state. This is already folded from standard ACP updates;
  do not parse raw messages or add a Host RPC. A persistent summary and a
  command-opened full view remain independent Slot, Command, and Overlay contributions.
- **ACP Session statistics:** consume `current()` / `subscribe()` for token,
  cache, turn, step, latency, or throughput UI. The builtin stats line is an
  ordinary Client Plugin in `conversation.composer.dock`; do not scrape the
  transcript or recreate raw ACP event folding in another plugin.
- **Host-backed behavior:** add a Host half only for Host-owned data. Use the
  Package-private JSON call surface exactly as inspected. ACP Session config is
  already Client-owned and needs no Host RPC.

When one Plugin registers several features, register each through its owning
service and rely on the shared Plugin lifetime for joint unload. If a feature
must appear or disappear while the Plugin remains loaded, use an inspected
state/event API and hold that feature's ordinary disposer in the Plugin; do not
add the condition to an unrelated service's registration schema.

Some interactions map a visual range onto a finite ordered choice set. Only
when the task asks for that mapping, keep the domains separate: for `P` visual
positions and `N` live choices, position `p` selects
`min(N - 1, floor(p * N / P))`. Derive the initial visual position from the
current live choice. This is a local mapping technique, not a prescribed
Command, Overlay, Theme, or ACP workflow.

## Treat supplied media as opaque assets

A file-backed theme usually needs stable resource identity, not visual
understanding. First decide whether knowing the pixels can materially change
the requested result. When the user supplies valid media plus an ordering
convention and asks only to use it, treat it as opaque: list the directory once
if exact names are unknown, sort deterministically, and do not inspect every
image merely to choose presentation options.

Make that one inventory sufficient for authoring: collect candidate names,
ordering keys, absolute paths, and only metadata the queried API actually
requires in the same operation. After it succeeds, use the result verbatim;
do not re-list the directory for validation or path conversion, and do not
probe sibling or `references` directories the user did not supply.

When content-aware judgment really matters — for example crop QA, palette
extraction, subject detection, or visual comparison — inspect the smallest
representative subset. If an image call reports that the model or tool lacks
visual capability, remember that result for the rest of the task and do not
retry sibling files. Continue with deterministic metadata and runtime defaults
when visual understanding is optional. If it is essential to the user's
requested outcome, report that exact missing capability while preserving any
independent work that remains possible.

For a static directory, resolve the ordered absolute paths while authoring and
embed that immutable list in `code.client`. Client code has no filesystem API.
Do not add `code.host` or Host RPC just to rediscover a static list at runtime;
use a Host half only when the user requests live-changing Host-owned assets.
Omit optional background presentation fields when the inspected Theme contract
provides safe defaults. This lets the renderer own generic fit, anchor, and
opacity behavior instead of guessing from image content.

When a slider selects both a frame and an ACP option, the frame index remains a
visual domain and the live ACP choices remain a business domain. Update the
background through the Theme Plugin's owned registration and map the index to
the inspected choices separately. Keep that registration, command, overlay,
and transaction in the same Theme Plugin fiber: no active-theme subscription,
theme predicate, or side panel is needed.

Client code is plain JavaScript: no imports, require, TypeScript, JSX, browser
globals, Node globals, native timers, raw ACP methods, guessed services, or
arbitrary compositor events. Every visible effect must disappear with its
owning Plugin run.

/** Built-in Client Plugin: configure the next standalone ACP harness. */

import {
  activateHarness,
  discoverHarnesses,
  selectedHarness,
  upsertHarness,
} from './harnesses.js'

export const name = 'harness-view'
export const inject = ['tuiCommands', 'tuiOverlay']

function entryOptions(settingsPath, options) {
  return discoverHarnesses(settingsPath, options).map((entry) => ({
    value: entry.id,
    label: entry.label,
    description: `${entry.source} · ${[entry.command, ...entry.args]
      .map((part) => /\s/.test(part) ? JSON.stringify(part) : part)
      .join(' ')}`,
  }))
}

export function apply(ctx, options = {}) {
  const settingsPath = options.settingsPath
  const choices = () => entryOptions(settingsPath, options)
  const save = (id) => {
    const entry = discoverHarnesses(settingsPath, options).find(({ id: candidate }) => candidate === id)
    if (entry === undefined) throw new Error(`unknown harness ${JSON.stringify(id)}`)
    upsertHarness(settingsPath, entry)
    activateHarness(settingsPath, id)
    ctx.tuiOverlay.openView({
      id: 'harness-saved',
      title: 'Harness saved',
      nodes: [{
        id: 'notice',
        kind: 'notice',
        level: 'info',
        text: options.hostOwned
          ? `${entry.label} is saved for a new standalone session. The current dsh profile and session remain Host-owned.`
          : `${entry.label} will start in a new session after restarting standalone Martty.`,
      }],
    })
  }
  const command = ctx.tuiCommands.register({
    name: 'harness',
    description: 'Switch harness in a new session on the next standalone launch',
    input: { hint: '[id]', options: choices() },
  }, async (args) => {
    const requested = args.trim()
    if (requested.length > 0) return save(requested)
    const entries = choices()
    const selected = selectedHarness(settingsPath)?.id
    ctx.tuiOverlay.openSelect({
      id: 'harness',
      title: 'Harness · next standalone session',
      value: entries.some(({ value }) => value === selected) ? selected : entries[0].value,
      options: entries,
    }, { onSubmit: save })
  })
  return () => command?.()
}

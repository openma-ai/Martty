/** Built-in Client Plugin: switch the standalone ACP Harness with a fresh session. */

import {
  activateHarness,
  discoverHarnesses,
  selectedHarness,
  upsertHarness,
} from './harnesses.js'

export const name = 'harness-view'
export const inject = ['tuiCommands', 'tuiOverlay', 'acpClient', 'acpSessionStatus']

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
  let runningHarnessId = typeof options.defaultHarness?.id === 'string'
    ? options.defaultHarness.id
    : selectedHarness(settingsPath)?.id
  const switchNow = async (entry) => {
    if (typeof ctx.acpClient?.switchAgent !== 'function') {
      throw new Error('Harness switching is unavailable on this ACP transport')
    }
    await ctx.acpClient.switchAgent({ command: entry.command, args: entry.args })
    upsertHarness(settingsPath, entry)
    activateHarness(settingsPath, entry.id)
    runningHarnessId = entry.id
    return {
      action: 'harness-switched',
      harness: {
        id: entry.id,
        label: entry.label,
        command: entry.command,
        args: entry.args,
      },
    }
  }
  const save = async (id) => {
    const entry = discoverHarnesses(settingsPath, options).find(({ id: candidate }) => candidate === id)
    if (entry === undefined) throw new Error(`unknown harness ${JSON.stringify(id)}`)
    if (!options.hostOwned) {
      if (ctx.acpSessionStatus?.current?.().session?.started === true) {
        ctx.tuiOverlay.openSelect({
          id: 'harness-confirm',
          title: 'Switch Harness? · starts a new session',
          value: 'switch',
          options: [
            {
              value: 'switch',
              label: `Switch to ${entry.label}`,
              description: 'Current session stays available in /session',
            },
            {
              value: 'cancel',
              label: 'Stay in current session',
              description: 'Keep using the current Harness',
            },
          ],
        }, {
          onSubmit(action) {
            if (action === 'switch') return switchNow(entry)
          },
        })
        return
      }
      return switchNow(entry)
    }
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
          : `${entry.label} is active in a new session.`,
      }],
    })
  }
  const command = ctx.tuiCommands.register({
    name: 'harness',
    description: 'Switch Harness now and start a new session',
    input: { hint: '[id]', options: choices() },
  }, async (args) => {
    const requested = args.trim()
    if (requested.length > 0) return save(requested)
    const entries = choices()
    const selected = runningHarnessId
    ctx.tuiOverlay.openSelect({
      id: 'harness',
      title: 'Switch Harness · starts a new session',
      value: entries.some(({ value }) => value === selected) ? selected : entries[0].value,
      options: entries,
    }, { onSubmit: save })
  })
  return () => command?.()
}

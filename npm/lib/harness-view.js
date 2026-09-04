/** Built-in Client Plugin: switch the standalone ACP Harness with a fresh session. */

import {
  addHarnessAsync,
  discoverHarnessCandidates,
  discoverHarnesses,
  fetchAcpRegistry,
  selectedHarness,
  setDefaultHarness,
  tokenizeHarnessArgs,
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

function findDescription(entry) {
  if (entry.status === 'Available to install') {
    return `available · install to ${entry.installPath ?? '~/.martty/bin'}`
  }
  if (entry.status === 'Ready via npx' || entry.status === 'Ready via uvx') {
    return `${entry.status.toLowerCase()} · configure`
  }
  if (entry.status === 'Needs npx' || entry.status === 'Needs uvx') {
    return `${entry.status.toLowerCase()} · install ${entry.distribution.type}`
  }
  if (entry.status === 'Not installed') {
    return `not installed · install ${[entry.install?.command, ...(entry.install?.args ?? [])]
      .filter(Boolean)
      .join(' ')}`
  }
  if (entry.status === 'Configured') {
    return `configured · switch · ${[entry.command, ...entry.args]
      .map((part) => /\s/.test(part) ? JSON.stringify(part) : part)
      .join(' ')}`
  }
  return `found locally · configure · ${[entry.resolvedCommand ?? entry.command, ...entry.args]
    .map((part) => /\s/.test(part) ? JSON.stringify(part) : part)
    .join(' ')}`
}

function findOptions(settingsPath, options, query = '') {
  return discoverHarnessCandidates(settingsPath, options, query)
    .map((entry) => ({
      value: entry.id,
      label: entry.label,
      description: findDescription(entry),
    }))
}

export function apply(ctx, options = {}) {
  const settingsPath = options.settingsPath
  let registryPromise
  const loadRegistry = async () => {
    if (options.registry !== undefined) return options.registry
    if (registryPromise === undefined) {
      registryPromise = (typeof options.fetchRegistry === 'function'
        ? options.fetchRegistry(options)
        : fetchAcpRegistry(options))
    }
    try {
      return await registryPromise
    } catch (error) {
      registryPromise = undefined
      throw error
    }
  }
  const choices = () => entryOptions(settingsPath, options)
  let commandRegistration
  const refreshCommandChoices = () => commandRegistration?.update({
    input: { hint: '[id] | find [query] | add <id> --command <cmd>', options: choices() },
  })
  let runningHarnessId = typeof options.forcedHarness?.id === 'string'
    ? options.forcedHarness.id
    : selectedHarness(settingsPath)?.id
  const switchNow = async (entry) => {
    if (typeof ctx.acpClient?.switchAgent !== 'function') {
      throw new Error('Harness switching is unavailable on this ACP transport')
    }
    await ctx.acpClient.switchAgent({
      command: entry.command,
      args: entry.args,
      ...(entry.env !== undefined ? { env: entry.env } : {}),
    })
    upsertHarness(settingsPath, entry)
    setDefaultHarness(settingsPath, entry.id)
    runningHarnessId = entry.id
    refreshCommandChoices()
    return {
      action: 'harness-switched',
      harness: {
        id: entry.id,
        label: entry.label,
        command: entry.command,
        args: entry.args,
        ...(entry.env !== undefined ? { env: entry.env } : {}),
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
    setDefaultHarness(settingsPath, id)
    refreshCommandChoices()
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
  commandRegistration = ctx.tuiCommands.register({
    name: 'harness',
    description: 'Switch or add a Harness; switching starts a new session',
    input: { hint: '[id] | find [query] | add <id> --command <cmd>', options: choices() },
  }, async (args) => {
    const requested = args.trim()
    if (requested.length > 0) {
      let tokens
      try {
        tokens = tokenizeHarnessArgs(requested)
      } catch (error) {
        ctx.tuiOverlay.openView({
          id: 'harness-add-error',
          title: 'Could not add Harness',
          nodes: [{
            id: 'error',
            kind: 'notice',
            level: 'error',
            text: error instanceof Error ? error.message : String(error),
          }],
        })
        return undefined
      }
      if (tokens[0] === 'add') {
        if (tokens[1] === undefined) {
          ctx.tuiOverlay.openView({
            id: 'harness-add-help',
            title: 'Add a Harness',
            nodes: [{
              id: 'instructions',
              kind: 'markdown',
              text: 'Use /harness find to discover the official ACP Registry and local PATH commands. Select a package distribution to configure it, or a binary distribution to install it into ~/.martty/bin. For anything else use /harness add <id> --command <cmd> [--label <label>] [--arg <arg>].',
            }],
          })
          return undefined
        }
        let entry
        try {
          const addOptions = tokens.slice(2).includes('--command')
            ? options
            : { ...options, registry: await loadRegistry() }
          entry = await addHarnessAsync(settingsPath, tokens[1], tokens.slice(2), addOptions)
        } catch (error) {
          ctx.tuiOverlay.openView({
            id: 'harness-add-error',
            title: 'Could not add Harness',
            nodes: [{
              id: 'error',
              kind: 'notice',
              level: 'error',
              text: error instanceof Error ? error.message : String(error),
            }],
          })
          return undefined
        }
        return save(entry.id)
      }
      if (tokens[0] === 'find') {
        let findScoped
        try {
          findScoped = { ...options, registry: await loadRegistry() }
        } catch (error) {
          ctx.tuiOverlay.openView({
            id: 'harness-find-error',
            title: 'ACP Registry unavailable',
            nodes: [{
              id: 'notice',
              kind: 'notice',
              level: 'error',
              text: `${error instanceof Error ? error.message : String(error)} Use /harness add <id> --command <path> for a local ACP command.`,
            }],
          })
          return undefined
        }
        const query = tokens.slice(1).join(' ')
        const found = findOptions(settingsPath, findScoped, query)
        if (found.length === 0) {
          ctx.tuiOverlay.openView({
            id: 'harness-find-empty',
            title: 'Find ACP Harnesses',
            nodes: [{
              id: 'notice',
              kind: 'notice',
              level: 'info',
              text: 'No ACP Harnesses found in the ACP Registry, local PATH, or settings. Use /harness add <id> --command <cmd> for a manual ACP command.',
            }],
          })
          return undefined
        }
        ctx.tuiOverlay.openSelect({
          id: 'harness-find',
          title: 'Find ACP Harnesses',
          value: found[0].value,
          options: found,
        }, {
          async onSubmit(id) {
            const entry = discoverHarnessCandidates(settingsPath, findScoped)
              .find((candidate) => candidate.id === id)
            if (entry?.status === 'Available to install') {
              ctx.tuiOverlay.openSelect({
                id: 'harness-install-confirm',
                title: `Install ${entry.label} in Martty?`,
                value: 'install',
                options: [
                  {
                    value: 'install',
                    label: `Install ${entry.label}`,
                    description: `download to ${entry.installPath ?? '~/.martty/bin'}`,
                  },
                  { value: 'cancel', label: 'Cancel', description: 'Stay in the current session' },
                ],
              }, {
                async onSubmit(action) {
                  if (action !== 'install') return undefined
                  try {
                    await addHarnessAsync(settingsPath, id, [], findScoped)
                    return save(id)
                  } catch (error) {
                    ctx.tuiOverlay.openView({
                      id: 'harness-install-error',
                      title: `Could not install ${entry.label}`,
                      nodes: [{
                        id: 'error',
                        kind: 'notice',
                        level: 'error',
                        text: error instanceof Error ? error.message : String(error),
                      }],
                    })
                    return undefined
                  }
                },
              })
              return undefined
            }
            if (entry?.distribution !== undefined && entry.status !== 'Not installed') {
              try {
                await addHarnessAsync(settingsPath, id, [], findScoped)
              } catch (error) {
                ctx.tuiOverlay.openView({
                  id: 'harness-add-error',
                  title: 'Could not configure Harness',
                  nodes: [{
                    id: 'error',
                    kind: 'notice',
                    level: 'error',
                    text: error instanceof Error ? error.message : String(error),
                  }],
                })
                return undefined
              }
            }
            if (entry?.status === 'Not installed') {
              const install = [entry.install?.command, ...(entry.install?.args ?? [])]
                .filter(Boolean)
                .join(' ')
              const recommended = entry.fallback === undefined
                ? [
                    `1. Install: \`${install}\``,
                    `2. Verify: \`command -v ${entry.command}\``,
                    `3. Configure: run /harness find ${entry.id} again.`,
                    `If ${entry.command} is outside PATH, use /harness add ${entry.id} --command <path>.`,
                  ].join('\n')
                : `Configure with /harness add ${entry.id}; this saves the registry fallback (\`${install}\`) as the launch command.`
              ctx.tuiOverlay.openView({
                id: 'harness-find-install',
                title: `${entry.label} is not installed`,
                nodes: [{
                  id: 'instructions',
                  kind: 'markdown',
                  text: entry.fallback === undefined
                    ? recommended
                    : `${recommended}\nThen choose ${entry.label} from /harness. If that is not available, use /harness add ${entry.id} --command <cmd>.`,
                }],
              })
              return undefined
            }
            return save(id)
          },
        })
        return undefined
      }
      return save(tokens[0])
    }
    const entries = choices()
    const selected = runningHarnessId
    if (entries.length === 0) {
      ctx.tuiOverlay.openView({
        id: 'harness-add-help',
        title: 'Add a Harness',
        nodes: [{
          id: 'instructions',
          kind: 'markdown',
          text: 'Use /harness find to discover the official ACP Registry and local PATH commands. Select a package distribution to configure it, or a binary distribution to install it into ~/.martty/bin. For anything else use /harness add <id> --command <cmd> [--label <label>] [--arg <arg>].',
        }],
      })
      return undefined
    }
    ctx.tuiOverlay.openSelect({
      id: 'harness',
      title: 'Switch Harness · starts a new session',
      value: entries.some(({ value }) => value === selected) ? selected : entries[0].value,
      options: entries,
    }, { onSubmit: save })
  })
  return () => commandRegistration?.()
}

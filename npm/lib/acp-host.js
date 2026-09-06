/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'

export const name = 'dsh-tui-acp-host'
export const inject = ['loader', 'userQuestions', 'permissionPresets']

function resolvedModule(ctx, specifier) {
  for (const anchor of [ctx.baseUrl, import.meta.url]) {
    if (typeof anchor !== 'string') continue
    try {
      return pathToFileURL(createRequire(anchor).resolve(specifier)).href
    } catch {
      // Try TUI's dependency graph after the active profile.
    }
  }
  return specifier
}

function resolvedHostModule(ctx, specifier) {
  if (typeof ctx.baseUrl !== 'string') return undefined
  try {
    return pathToFileURL(createRequire(ctx.baseUrl).resolve(specifier)).href
  } catch {
    return undefined
  }
}

/**
 * dsh 0.1.2 replaced the single user-question provider slot with a scoped
 * waterfall. Keep the ACP dependency's older provider seam working while it
 * remains protocol-compatible with both Host generations.
 */
export function installUserQuestionsCompatibility(ctx) {
  const service = ctx.get?.('userQuestions')
  if (service === undefined || typeof service.registerProvider === 'function') return false
  const registerProvider = (provider) => {
    if (provider === null || typeof provider !== 'object' || typeof provider.ask !== 'function') {
      throw new TypeError('userQuestions.registerProvider: provider must expose ask()')
    }
    return ctx.on('user-questions/request', (request) => provider.ask(request))
  }
  Object.defineProperty(service, 'registerProvider', {
    configurable: true,
    value: registerProvider,
  })
  ctx.effect(() => () => {
    if (service.registerProvider === registerProvider) delete service.registerProvider
  }, 'dsh-tui.user-questions-compatibility')
  return true
}

/**
 * dsh 0.1.2 replaced Session.events with snapshotEvents(). ACP 0.4.x only
 * reads the old immutable snapshot, so restore that getter until ACP can
 * require the new Host API directly.
 */
export function installSessionEventsCompatibility(Session) {
  const prototype = Session?.prototype
  if (prototype === undefined || 'events' in prototype) return false
  if (typeof prototype.snapshotEvents !== 'function') return false
  Object.defineProperty(prototype, 'events', {
    configurable: true,
    get() {
      if (typeof this.snapshotEvents === 'function') return this.snapshotEvents()
      // Cordis may hand an older plugin a remote Session reference whose
      // declared surface predates snapshotEvents(). The 0.1.2 reference still
      // exposes the scalar seq/eventAt pair, which yields the same snapshot.
      if (typeof this.eventAt === 'function' && Number.isInteger(this.seq)) {
        const events = []
        for (let seq = 0; seq < this.seq; seq += 1) {
          const event = this.eventAt(seq)
          if (event !== undefined) events.push(event)
        }
        return Object.freeze(events)
      }
      return Object.freeze([])
    },
  })
  return true
}

/** Accept ACP 0.4.x's event-array read alongside dsh 0.1.2's Session read. */
export function installPermissionPresetsCompatibility(permissionPresets) {
  if (permissionPresets === undefined) return false
  const current = permissionPresets.current
  if (typeof current !== 'function' || current.dshTuiAcceptsEventArrays === true) return false
  const compatibleCurrent = function (target) {
    if (!Array.isArray(target)) return current.call(this, target)
    let state = { preset: null, sandbox: null, approval: null, seeded: false }
    for (const event of target) {
      switch (event?.type) {
        case 'permission/preset':
          state = { ...state, preset: event.data?.preset ?? null }
          break
        case 'sandbox/mode':
          state = { ...state, sandbox: event.data?.mode ?? null }
          break
        case 'approval/policy':
          state = { ...state, approval: event.data?.policy ?? null }
          break
        case 'session/end-seed':
          state = { ...state, seeded: true }
          break
      }
    }
    return this.derive(state)
  }
  Object.defineProperty(compatibleCurrent, 'dshTuiAcceptsEventArrays', { value: true })
  Object.defineProperty(permissionPresets, 'current', {
    configurable: true,
    value: compatibleCurrent,
  })
  return true
}

async function mountHostCompatibility(ctx) {
  // The service can come from the active Host even when module resolution
  // below lands on ACP's older peer copy, so adapt the live instance directly.
  installPermissionPresetsCompatibility(ctx.permissionPresets ?? ctx.get?.('permissionPresets'))

  const sessionModule = resolvedHostModule(ctx, '@deepseek-ai/dsh-session')
  if (sessionModule !== undefined) {
    const exports = await ctx.loader.import(sessionModule)
    installSessionEventsCompatibility(exports.Session)
  }

  // New dsh presets opt the delegation tool into Host-owned model selection.
  // Older hosts neither export nor require this service, so resolution is
  // deliberately anchored only at the active Host profile.
  if (ctx.get?.('subagentModelSelection') !== undefined) return
  const resolved = resolvedHostModule(
    ctx,
    '@deepseek-ai/dsh-tool-subagent/model-selection-settings',
  )
  if (resolved === undefined) return
  const exports = await ctx.loader.import(resolved)
  await ctx.plugin(ctx.loader.unwrapExports(exports))
}

export async function apply(ctx, config) {
  await mountHostCompatibility(ctx)
  installUserQuestionsCompatibility(ctx)
  const specifier = '@openma/deepseek-harness-acp/plugin'
  const resolved = resolvedModule(ctx, specifier)
  let exports
  try {
    exports = await import(resolved)
  } catch (cause) {
    throw new Error(`dsh-tui: failed to import ${specifier} from ${resolved}`, { cause })
  }
  const plugin = ctx.loader.unwrapExports(exports)
  try {
    await ctx.plugin(plugin, config)
  } catch (cause) {
    throw new Error(`dsh-tui: failed to mount ${specifier}`, { cause })
  }
}

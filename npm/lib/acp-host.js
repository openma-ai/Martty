/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'

export const name = 'dsh-tui-acp-host'
export const inject = ['loader', 'userQuestions']

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

async function mountHostCompatibility(ctx) {
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

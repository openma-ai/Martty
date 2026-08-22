/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { pathToFileURL } from 'node:url'

export const name = 'dsh-tui-acp-host'
export const inject = ['loader']

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

function shippedPresetRoot(ctx) {
  for (const anchor of [ctx.baseUrl, import.meta.url]) {
    if (typeof anchor !== 'string') continue
    try {
      const manifest = createRequire(anchor).resolve('@deepseek-ai/dsh/package.json')
      const root = join(dirname(manifest), 'config', 'agent-presets')
      if (existsSync(root)) return root
    } catch {
      // Try the next resolution anchor.
    }
  }
  throw new Error('dsh-tui: cannot resolve the dsh shipped agent presets')
}

async function mountService(ctx, service, specifier, config) {
  if (ctx.get(service) !== undefined) return
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
  if (ctx.get(service) === undefined) {
    throw new Error(`dsh-tui: ${specifier} did not provide ${service}`)
  }
}

export async function apply(ctx, config) {
  // ACP currently mounts these Host services dynamically. Resolve them here
  // first so Node's Windows ESM loader never receives a raw drive path from
  // Cordis's internal bare-package resolver.
  await mountService(ctx, 'agentPresets', '@deepseek-ai/dsh-agent-presets', {
    default: 'standard',
    roots: [{ path: shippedPresetRoot(ctx), trust: 'system' }],
  })
  await mountService(ctx, 'dynamicCordisRunner', '@deepseek-ai/dsh-cordis-host-runner')

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

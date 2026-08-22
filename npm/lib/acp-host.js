/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { createRequire } from 'node:module'
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

export async function apply(ctx, config) {
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

/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { createRequire } from 'node:module'

export const name = 'dsh-tui-acp-host'
export const inject = ['loader']

const requireFromTui = createRequire(import.meta.url)

function ownAcpPlugin() {
  const specifier = '@openma/deepseek-harness-acp/plugin'
  try {
    return requireFromTui.resolve(specifier)
  } catch {
    // A source-linked package may rely on the active profile's installation.
    return specifier
  }
}

export async function apply(ctx, config) {
  const exports = await ctx.loader.import(ownAcpPlugin())
  const plugin = ctx.loader.unwrapExports(exports)
  await ctx.plugin(plugin, config)
}

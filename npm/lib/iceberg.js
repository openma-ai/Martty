/**
 * Gallery palette pack `iceberg`. Registers complete dark/light token maps:
 * dark from Iceberg Dark, light from Iceberg Light (terminalcolors.com/themes/iceberg). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const icebergPalette = JSON.parse(
  readFileSync(new URL('./palettes/iceberg.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-iceberg'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(icebergPalette, { activate: false }))
}

export { icebergPalette }

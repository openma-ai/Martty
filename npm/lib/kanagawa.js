/**
 * Gallery palette pack `kanagawa`. Registers complete dark/light token maps:
 * dark from the Kanagawa Wave variant, light from Kanagawa Lotus
 * (terminalcolors.com/themes/kanagawa). Does not activate: `/theme` covers
 * it. `inject = ['tuiTheme']`: sibling profile row, not `ctx.plugin` inside
 * the runner.
 */

import { readFileSync } from 'node:fs'

const kanagawaPalette = JSON.parse(
  readFileSync(new URL('./palettes/kanagawa.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-kanagawa'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(kanagawaPalette, { activate: false }))
}

export { kanagawaPalette }

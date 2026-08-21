/**
 * Gallery palette pack `solarized`. Registers complete dark/light token maps:
 * dark from Solarized Dark, light from Solarized Light (terminalcolors.com/themes/solarized). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const solarizedPalette = JSON.parse(
  readFileSync(new URL('./palettes/solarized.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-solarized'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(solarizedPalette, { activate: false }))
}

export { solarizedPalette }

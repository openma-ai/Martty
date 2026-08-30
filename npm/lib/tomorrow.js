/**
 * Gallery palette pack `tomorrow`. Registers complete dark/light token maps:
 * dark from Tomorrow Night Bright, light from Tomorrow
 * (terminalcolors.com/themes/tomorrow). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const tomorrowPalette = JSON.parse(
  readFileSync(new URL('./palettes/tomorrow.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-tomorrow'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(tomorrowPalette, { activate: false }))
}

export { tomorrowPalette }

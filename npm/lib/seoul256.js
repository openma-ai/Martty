/**
 * Gallery palette pack `seoul256`. Registers complete dark/light token maps:
 * dark from Seoul256 Dark, light from Seoul256 Light (terminalcolors.com/themes/seoul256). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const seoul256Palette = JSON.parse(
  readFileSync(new URL('./palettes/seoul256.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-seoul256'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(seoul256Palette, { activate: false }))
}

export { seoul256Palette }

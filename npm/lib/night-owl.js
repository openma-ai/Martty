/**
 * Gallery palette pack `night-owl`. Registers complete dark/light token maps:
 * dark from Night Owl Dark, light from Night Owl Light (terminalcolors.com/themes/night-owl). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const nightowlPalette = JSON.parse(
  readFileSync(new URL('./palettes/night-owl.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-night-owl'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(nightowlPalette, { activate: false }))
}

export { nightowlPalette }

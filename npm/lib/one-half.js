/**
 * Gallery palette pack `one-half`. Registers complete dark/light token maps:
 * dark from One Half Dark, light from One Half Light (terminalcolors.com/themes/one-half). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const onehalfPalette = JSON.parse(
  readFileSync(new URL('./palettes/one-half.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-one-half'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(onehalfPalette, { activate: false }))
}

export { onehalfPalette }

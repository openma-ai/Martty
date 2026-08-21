/**
 * Gallery palette pack `gruvbox`. Registers complete dark/light token
 * maps: dark from the Gruvbox Dark palette, light from Gruvbox Light
 * (both from ricardodantas/ratatui-themes). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const gruvboxPalette = JSON.parse(
  readFileSync(new URL('./palettes/gruvbox.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-gruvbox'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(gruvboxPalette, { activate: false }))
}

export { gruvboxPalette }

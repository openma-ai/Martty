/**
 * Gallery palette pack `catppuccin`. Registers complete token maps: dark
 * from the Catppuccin Mocha variant, light from Catppuccin Latte
 * (terminalcolors.com/themes/catppuccin)
 * (terminalcolors.com/themes/catppuccin). Does not activate: `/theme`
 * covers it. `inject = ['tuiTheme']`: sibling profile row, not `ctx.plugin`
 * inside the runner.
 */

import { readFileSync } from 'node:fs'

const catppuccinPalette = JSON.parse(
  readFileSync(new URL('./palettes/catppuccin.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-catppuccin'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(catppuccinPalette, { activate: false }))
}

export { catppuccinPalette }

/**
 * Gallery palette pack `ayu`. Registers complete token maps: dark from the
 * Ayu dark variant, the second map from Ayu Mirage — both dark-ish variants
 * of the Ayu family (terminalcolors.com/themes/ayu). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const ayuPalette = JSON.parse(
  readFileSync(new URL('./palettes/ayu.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-ayu'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(ayuPalette, { activate: false }))
}

export { ayuPalette }

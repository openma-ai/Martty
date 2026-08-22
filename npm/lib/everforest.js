/**
 * Gallery palette pack `everforest`. Registers complete dark/light token maps:
 * dark from Everforest Dark, light from Everforest Light (terminalcolors.com/themes/everforest). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const everforestPalette = JSON.parse(
  readFileSync(new URL('./palettes/everforest.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-everforest'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(everforestPalette, { activate: false }))
}

export { everforestPalette }

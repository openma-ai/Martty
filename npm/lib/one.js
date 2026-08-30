/**
 * Gallery palette pack `one`. Registers complete dark/light token maps:
 * dark from One Dark, light from One Light (terminalcolors.com/themes/one). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const onePalette = JSON.parse(
  readFileSync(new URL('./palettes/one.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-one'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(onePalette, { activate: false }))
}

export { onePalette }

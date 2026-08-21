/**
 * Gallery palette pack `one-dark-pro`. Registers complete dark/light token
 * maps: dark from the One Dark column, light from the One Light column
 * (nathanbuchar/atom-one-dark-terminal COLORS). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const oneDarkProPalette = JSON.parse(
  readFileSync(new URL('./palettes/one-dark-pro.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-one-dark-pro'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(oneDarkProPalette, { activate: false }))
}

export { oneDarkProPalette }

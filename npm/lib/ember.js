/**
 * Gallery palette pack `ember`. Registers complete dark/light token maps.
 * Does not activate: `--demo-skin` calls `tuiTheme.activate('ember')`.
 * `inject = ['tuiTheme']`: sibling profile row, not `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const emberPalette = JSON.parse(
  readFileSync(new URL('./palettes/ember.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-ember'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(emberPalette, { activate: false }))
}

export { emberPalette }

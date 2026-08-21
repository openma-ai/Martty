/**
 * Gallery palette pack `ayu`. Registers complete dark/light token maps:
 * dark from Ayu.toml, light from "Ayu Light.toml" (mbadolato/iTerm2-Color-
 * Schemes, alacritty directory). Does not activate: `/theme` covers it.
 * `inject = ['tuiTheme']`: sibling profile row, not `ctx.plugin` inside
 * the runner.
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

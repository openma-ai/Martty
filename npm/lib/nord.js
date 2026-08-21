/**
 * Gallery palette pack `nord`. Registers complete dark/light token maps:
 * dark from Nord.toml, light from "Nord Light.toml" (mbadolato/iTerm2-Color-
 * Schemes, alacritty directory). Does not activate: `/theme` covers it.
 * `inject = ['tuiTheme']`: sibling profile row, not `ctx.plugin` inside
 * the runner.
 */

import { readFileSync } from 'node:fs'

const nordPalette = JSON.parse(
  readFileSync(new URL('./palettes/nord.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-nord'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(nordPalette, { activate: false }))
}

export { nordPalette }

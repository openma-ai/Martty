/**
 * Gallery palette pack `ghostty`. Registers complete dark/light token maps:
 * dark from "Ghostty Default Style Dark", light from "Tomorrow" — the two
 * halves of the Tomorrow family that Ghostty's default style is built on
 * (mbadolato/iTerm2-Color-Schemes, ghostty directory). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const ghosttyPalette = JSON.parse(
  readFileSync(new URL('./palettes/ghostty.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-ghostty'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(ghosttyPalette, { activate: false }))
}

export { ghosttyPalette }

/**
 * Gallery palette pack `nvim`. Registers complete dark/light token maps:
 * dark from "Nvim Dark.toml", light from "Nvim Light.toml"
 * (mbadolato/iTerm2-Color-Schemes, alacritty directory). Does not activate:
 * `/theme` covers it. `inject = ['tuiTheme']`: sibling profile row, not
 * `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const nvimPalette = JSON.parse(
  readFileSync(new URL('./palettes/nvim.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-nvim'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(nvimPalette, { activate: false }))
}

export { nvimPalette }

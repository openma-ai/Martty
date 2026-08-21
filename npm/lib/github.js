/**
 * Gallery palette pack `github`. Registers complete token maps: dark from
 * the GitHub Dark variant, the second map from GitHub Dark Dimmed — both
 * dark variants of the GitHub family (terminalcolors.com/themes/github).
 * Does not activate: `/theme` covers it. `inject = ['tuiTheme']`: sibling
 * profile row, not `ctx.plugin` inside the runner.
 */

import { readFileSync } from 'node:fs'

const githubPalette = JSON.parse(
  readFileSync(new URL('./palettes/github.json', import.meta.url), 'utf8'),
)

export const name = 'tui-theme-github'
export const inject = ['tuiTheme']

export function apply(ctx) {
  ctx.effect(() => ctx.tuiTheme.register(githubPalette, { activate: false }))
}

export { githubPalette }

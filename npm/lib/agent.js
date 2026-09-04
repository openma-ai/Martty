/** Resolve the default ACP agent used by the standalone TUI entry. */

import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { selectedHarness } from './harnesses.js'

/**
 * Resolve the ACP package and TUI's internal Creator overlay bundle.
 * @param {string | URL} [anchor]
 * @returns {{ command: string, args: string[], env?: Record<string, string> }}
 */
export function resolveDependencyStack(anchor = import.meta.url) {
  const req = createRequire(anchor)
  const acpPackageJson = req.resolve('@openma/deepseek-harness-acp/package.json')
  const creatorBundle = fileURLToPath(new URL('../creator', import.meta.url))
  const manifest = JSON.parse(readFileSync(acpPackageJson, 'utf8'))
  const declared = typeof manifest.bin === 'string'
    ? manifest.bin
    : manifest.bin?.['dsh-acp']
  if (typeof declared !== 'string' || declared.length === 0) {
    throw new Error('@openma/deepseek-harness-acp declares no dsh-acp bin')
  }
  return {
    command: resolve(dirname(acpPackageJson), declared),
    args: ['--bundle', creatorBundle],
  }
}

/**
 * Resolve the standalone ACP command without affecting the profile path.
 * `forcedHarness` is an internal product initialization value, not a CLI
 * argument; null/omitted means that no product Harness is forced.
 * @param {string | URL} [anchor]
 * @param {{ settingsPath?: string, forcedHarness?: { id: string, label: string, command: string, args?: string[] } | null }} [options]
 * @returns {{ command: string, args: string[] }}
 */
export function resolveStackedAgent(anchor = import.meta.url, options = {}) {
  const envCmd = process.env.DSH_TUI_AGENT
  if (typeof envCmd === 'string' && envCmd.trim().length > 0) {
    const tokens = envCmd.trim().split(/\s+/)
    return { command: tokens[0], args: tokens.slice(1) }
  }
  if (options.forcedHarness !== undefined && options.forcedHarness !== null) {
    const configured = options.forcedHarness
    if (typeof configured !== 'object' || Array.isArray(configured)
      || typeof configured.command !== 'string' || configured.command.trim().length === 0) {
      throw new Error('forcedHarness must declare a non-empty command')
    }
    return {
      command: configured.command,
      args: Array.isArray(configured.args) ? configured.args.map(String) : [],
      ...(configured.env !== undefined ? { env: { ...configured.env } } : {}),
    }
  }
  if (typeof options.settingsPath === 'string') {
    const selected = selectedHarness(options.settingsPath)
    if (selected !== undefined) {
      return {
        command: selected.command,
        args: selected.args,
        ...(selected.env !== undefined ? { env: { ...selected.env } } : {}),
      }
    }
  }
  try {
    return resolveDependencyStack(anchor)
  } catch {
    // Source checkouts may not have materialized the package dependencies.
  }
  const home = process.env.DSH_HOME ?? join(homedir(), '.dsh')
  for (const profile of ['tui-test', 'martty', 'tui']) {
    const bin = join(home, 'profiles', profile, 'node_modules', '.bin', 'dsh-acp')
    const creator = join(
      home,
      'profiles',
      profile,
      'node_modules',
      '@openma',
      'deepseek-harness-tui',
      'creator',
    )
    if (existsSync(bin) && existsSync(join(creator, 'package.json'))) {
      return { command: bin, args: ['--bundle', creator] }
    }
  }
  return { command: 'dsh-acp', args: [] }
}

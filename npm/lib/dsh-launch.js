/**
 * `martty --dsh <profile>`: run against the user's global dsh instead of
 * the bundled dsh-acp runtime.
 *
 * The profile decides how Martty attaches:
 * - a profile carrying the `martty` bundle (`dsh plugin --profile <name> add
 *   martty`) is a complete TUI surface: dsh boots its Host tree and its own
 *   runner starts the TUI Client, so the launcher hands the terminal to
 *   `dsh --profile <name>`;
 * - a profile carrying only `@openma/deepseek-harness-acp` is an ACP server
 *   on stdio, so `dsh --profile <name>` becomes the ACP agent this Client
 *   spawns.
 *
 * Either way the global dsh version is checked up front against the floor
 * every installed plugin declares. A dsh below a floor is not attempted:
 * the launcher falls back to the bundled dsh-acp runtime and the TUI shows a
 * short notice saying why. A dsh newer than the newest tested one is
 * attempted with a warning — versions are never rejected for being new.
 */

import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { homedir } from 'node:os'
import path from 'node:path'
import crossSpawn from 'cross-spawn'
import semver from 'semver'
import { DSH_FLOOR, isUntested, requirementOf, satisfiesRange, TESTED_DSH_THROUGH } from './dsh-compat.js'

export const MARTTY_BUNDLE = 'martty'
export const ACP_BUNDLE = '@openma/deepseek-harness-acp'
const OWN_VERSION = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version
const INSTALL_HINT = 'npm install -g @deepseek-ai/dsh@latest'

const usage = (message) => Object.assign(new Error(message), { exitCode: 2 })
const failure = (message) => Object.assign(new Error(message), { exitCode: 1 })

/**
 * Split `--dsh <profile>` / `--dsh=<profile>` off argv.
 * @param {string[]} argv
 * @returns {{ profile: string | null, rest: string[] }}
 */
export function takeDshFlag(argv) {
  let profile = null
  const rest = []
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i]
    let value
    if (token === '--dsh') {
      value = argv[i + 1]
      i += 1
    } else if (token.startsWith('--dsh=')) {
      value = token.slice('--dsh='.length)
    } else {
      rest.push(token)
      continue
    }
    if (typeof value !== 'string' || value.trim() === '' || value.startsWith('-')) {
      throw usage('--dsh needs a dsh profile name (for example --dsh martty)')
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value)) {
      throw usage(`--dsh: ${JSON.stringify(value)} is not a dsh profile name`)
    }
    if (profile !== null && profile !== value) throw usage('--dsh may name only one profile')
    profile = value
  }
  if (profile !== null && rest.some((arg) => arg === '--agent' || arg === '--agent-arg')) {
    throw usage('--dsh selects the agent itself; it cannot be combined with --agent/--agent-arg')
  }
  return { profile, rest }
}

/**
 * The global dsh: whatever `dsh` resolves to on PATH, asked for its version.
 * @returns {{ command: string, version: string }}
 */
export function resolveGlobalDsh({ env = process.env, spawnSync = crossSpawn.sync } = {}) {
  const command = 'dsh'
  const result = spawnSync(command, ['--version'], { encoding: 'utf8', env, timeout: 30_000 })
  if (result.error?.code === 'ENOENT') {
    throw failure(`martty: --dsh needs a global dsh on PATH, but \`${command}\` was not found. Install it with: ${INSTALL_HINT}`)
  }
  const version = semver.valid(String(result.stdout ?? '').trim().split(/\s+/).at(-1))
  if (result.status !== 0 || version === null) {
    const detail = String(result.stderr ?? result.error?.message ?? '').trim()
    throw failure(`martty: could not read the version of the global \`${command}\`${detail ? `: ${detail}` : ''}`)
  }
  return { command, version }
}

export function dshHome(env = process.env) {
  return env.DSH_HOME?.trim() ? path.resolve(env.DSH_HOME) : path.join(homedir(), '.dsh')
}

function installedManifest(profileDir, name) {
  try {
    const file = createRequire(path.join(profileDir, 'package.json')).resolve(`${name}/package.json`)
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch {
    return undefined
  }
}

/**
 * Read a profile and decide whether the given dsh version may run it.
 * @returns {{ mode: 'profile' | 'agent', dir: string, requirements: { name: string, version: string, range: string, supported: boolean }[] }}
 */
export function inspectDshProfile(profile, dshVersion, { env = process.env } = {}) {
  const dir = path.join(dshHome(env), 'profiles', profile)
  const manifestPath = path.join(dir, 'package.json')
  if (!existsSync(manifestPath)) {
    throw failure([
      `martty: dsh profile "${profile}" does not exist (${dir}).`,
      `Install Martty into it first: dsh plugin --profile ${profile} add martty@latest`,
    ].join('\n'))
  }
  let manifest
  try {
    manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  } catch (cause) {
    throw failure(`martty: cannot read dsh profile "${profile}" (${manifestPath}): ${cause.message}`)
  }
  const bundles = Array.isArray(manifest?.dsh?.profile?.bundles) ? manifest.dsh.profile.bundles : []
  const mode = bundles.includes(MARTTY_BUNDLE)
    ? 'profile'
    : bundles.includes(ACP_BUNDLE) ? 'agent' : null
  if (mode === null) {
    throw failure([
      `martty: dsh profile "${profile}" carries neither the ${MARTTY_BUNDLE} nor the ${ACP_BUNDLE} bundle.`,
      `Add Martty to it: dsh plugin --profile ${profile} add martty@latest`,
    ].join('\n'))
  }
  // Every floor that gates the Host: this Martty's, the profile's installed
  // Martty's (`engines.dsh`; releases <= 0.3.2 declared `dsh.supported`), and
  // the ACP plugin's, which it enforces at mount time.
  const requirements = [{ name: `${MARTTY_BUNDLE} (launcher)`, version: OWN_VERSION, range: DSH_FLOOR }]
  const martty = mode === 'profile' ? installedManifest(dir, MARTTY_BUNDLE) : undefined
  const marttyRange = requirementOf(martty) ?? martty?.dsh?.supported
  if (typeof marttyRange === 'string') {
    requirements.push({ name: MARTTY_BUNDLE, version: martty.version, range: marttyRange })
  }
  const acp = installedManifest(dir, ACP_BUNDLE)
  const acpRange = requirementOf(acp)
  if (typeof acpRange === 'string') {
    requirements.push({ name: ACP_BUNDLE, version: acp.version, range: acpRange })
  }
  for (const requirement of requirements) {
    requirement.supported = satisfiesRange(dshVersion, requirement.range)
  }
  return { mode, dir, requirements }
}

/** The notice shown in the TUI when `--dsh` falls back to the bundled runtime. */
export function fallbackNotice(profile, dsh, requirements) {
  const unmet = requirements.filter(({ supported }) => !supported)
  const needs = unmet.map(({ name, version, range }) => `${name} ${version} needs dsh ${range}`).join('; ')
  return [
    `--dsh ${profile}: the global dsh ${dsh.version} is not supported (${needs}), so Martty is using its bundled harness.`,
    `Install a supported dsh (${INSTALL_HINT}) or update the profile: dsh plugin --profile ${profile} update`,
  ].join('\n')
}

export function untestedNotice(dsh) {
  return `The global dsh ${dsh.version} is newer than the newest dsh Martty was tested with (${TESTED_DSH_THROUGH}); continuing.`
}

/**
 * Resolve `--dsh <profile>`:
 * - `profile`: hand the terminal to `dsh --profile <name>` (the profile carries Martty);
 * - `agent`: spawn `dsh --profile <name>` as this Client's ACP agent;
 * - `bundled`: the global dsh is below a floor — keep the bundled harness and
 *   show `notice` in the TUI.
 * Missing dsh / missing profile still fail with a message: there is nothing
 * to attach to and the user named it explicitly.
 * @returns {{ mode: 'profile', command: string, args: string[], warning?: string }
 *   | { mode: 'agent', agent: { command: string, args: string[] }, notice?: { text: string, level: 'info' } }
 *   | { mode: 'bundled', notice: { text: string, level: 'warn' } }}
 */
export function planDshLaunch(profile, appArgs, options = {}) {
  const dsh = resolveGlobalDsh(options)
  const { mode, requirements } = inspectDshProfile(profile, dsh.version, options)
  if (requirements.some((requirement) => !requirement.supported)) {
    return { mode: 'bundled', notice: { text: fallbackNotice(profile, dsh, requirements), level: 'warn' } }
  }
  const untested = isUntested(dsh.version) ? untestedNotice(dsh) : undefined
  if (mode === 'profile') {
    return {
      mode, command: dsh.command, args: ['--profile', profile, ...appArgs],
      ...(untested ? { warning: `martty: ${untested}` } : {}),
    }
  }
  return {
    mode, agent: { command: dsh.command, args: ['--profile', profile] },
    ...(untested ? { notice: { text: untested, level: 'info' } } : {}),
  }
}

/**
 * Hand the terminal to `dsh --profile <name>` and return the exit code to
 * use. The child owns the terminal, so this process ignores Ctrl-C while it
 * runs and lets dsh decide what it means.
 */
export function runDshProfile(plan, { spawnSync = crossSpawn.sync } = {}) {
  if (plan.warning) console.error(plan.warning)
  const ignore = () => {}
  for (const signal of ['SIGINT', 'SIGQUIT']) process.on(signal, ignore)
  try {
    const result = spawnSync(plan.command, plan.args, { stdio: 'inherit' })
    if (result.error) {
      console.error(`martty: failed to launch dsh: ${result.error.message}`)
      return 1
    }
    if (result.status !== null) return result.status
    return result.signal === 'SIGINT' ? 130 : result.signal === 'SIGTERM' ? 143 : 1
  } finally {
    for (const signal of ['SIGINT', 'SIGQUIT']) process.off(signal, ignore)
  }
}

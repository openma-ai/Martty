/**
 * Cordis client boot: tuiTheme + tuiSlots + acp-client + TUI shell.
 *
 * The process root is a client tree. A profile launch injects Host ACP stdio;
 * standalone launches may spawn an agent instead. This module does not import
 * dsh or dsh-acp.
 */

import { apply as applyAcpClient } from './acp-client.js'
import { constants, copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
import { apply as applyCordisClientRunner } from './inspect.js'
import { apply as applyShell } from './index.js'
import { resolveStackedAgent } from './agent.js'
import { apply as applySlots } from './tui-slots.js'
import { apply as applyTheme } from './tui-theme.js'
import { apply as applyOne, inject as oneInject } from './one.js'
import { apply as applyAyu, inject as ayuInject } from './ayu.js'
import { apply as applyCatppuccin, inject as catppuccinInject } from './catppuccin.js'
import { apply as applyGithub, inject as githubInject } from './github.js'
import { apply as applyKanagawa, inject as kanagawaInject } from './kanagawa.js'
import { apply as applyEverforest, inject as everforestInject } from './everforest.js'
import { apply as applyGruvbox, inject as gruvboxInject } from './gruvbox.js'
import { apply as applyIceberg, inject as icebergInject } from './iceberg.js'
import { apply as applyNightOwl, inject as nightOwlInject } from './night-owl.js'
import { apply as applyOneHalf, inject as oneHalfInject } from './one-half.js'
import { apply as applySolarized, inject as solarizedInject } from './solarized.js'
import { apply as applyCommands } from './tui-commands.js'
import { apply as applyOverlay } from './tui-overlay.js'
import { apply as applyPresets, inject as presetsInject } from './tui-presets.js'
import { apply as applyMarttyPreset, inject as marttyPresetInject } from './martty-preset.js'
import { apply as applyPlanView, inject as planViewInject } from './plan-view.js'
import { apply as applyStatsView, inject as statsViewInject } from './stats-view.js'
import { apply as applySessionStatus, inject as sessionStatusInject } from './acp-session-status.js'
import { apply as applyStatusView, inject as statusViewInject } from './status-view.js'
import { apply as applyDeepseekLogo, inject as deepseekLogoInject } from './deepseek-logo.js'
import { createTuiPluginStore, marttyHome } from './tui-plugin-store.js'
import { installTuiLocalPlugins } from './tui-local-plugins.js'

/**
 * @param {object} [options]
 * @param {{ command: string, args?: string[] }} [options.agent]
 * @param {{ stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, child?: import('node:child_process').ChildProcess }} [options.stream]
 * @param {{ stdin: number | 'inherit', stdout: number | 'inherit' }} [options.tty]
 * @param {string} [options.settingsPath]
 * @param {string} [options.artifactRoot]
 * @param {Array<{ id: string, kind: 'theme' | 'ui-preset', entry: string }>} [options.packagePlugins]
 */
export async function bootClient(options = {}) {
  const { Context } = await import('@deepseek-ai/cordis')
  const ctx = new Context()
  const acpConfig = options.stream !== undefined
    ? { stream: options.stream }
    : { agent: options.agent ?? resolveStackedAgent() }
  const settingsPath = options.settingsPath ?? uiSettingsPath(options.extraArgs ?? [])
  if (options.settingsPath === undefined) {
    migrateLegacyUiSettings(settingsPath, legacyUiSettingsPaths(options.extraArgs ?? []))
  }
  const presetConfig = { settingsPath }
  if (typeof ctx.plugin === 'function') {
    await ctx.plugin({ name: 'tui-theme', inject: [], apply: applyTheme }, presetConfig)
    await ctx.plugin({ name: 'tui-theme-one', inject: oneInject, apply: applyOne })
    await ctx.plugin({ name: 'tui-theme-ayu', inject: ayuInject, apply: applyAyu })
    await ctx.plugin({ name: 'tui-theme-catppuccin', inject: catppuccinInject, apply: applyCatppuccin })
    await ctx.plugin({ name: 'tui-theme-github', inject: githubInject, apply: applyGithub })
    await ctx.plugin({ name: 'tui-theme-kanagawa', inject: kanagawaInject, apply: applyKanagawa })
    await ctx.plugin({ name: 'tui-theme-everforest', inject: everforestInject, apply: applyEverforest })
    await ctx.plugin({ name: 'tui-theme-gruvbox', inject: gruvboxInject, apply: applyGruvbox })
    await ctx.plugin({ name: 'tui-theme-iceberg', inject: icebergInject, apply: applyIceberg })
    await ctx.plugin({ name: 'tui-theme-night-owl', inject: nightOwlInject, apply: applyNightOwl })
    await ctx.plugin({ name: 'tui-theme-one-half', inject: oneHalfInject, apply: applyOneHalf })
    await ctx.plugin({ name: 'tui-theme-solarized', inject: solarizedInject, apply: applySolarized })
    await ctx.plugin({ name: 'tui-slots', inject: [], apply: applySlots })
    await ctx.plugin({ name: 'tui-commands', inject: [], apply: applyCommands })
    await ctx.plugin({ name: 'tui-overlay', inject: [], apply: applyOverlay })
    await ctx.plugin({ name: 'tui-presets', inject: presetsInject, apply: applyPresets }, presetConfig)
    await ctx.plugin({ name: 'martty-preset', inject: marttyPresetInject, apply: applyMarttyPreset })
    await ctx.plugin({ name: 'acp-client', inject: [], apply: applyAcpClient }, acpConfig)
    await ctx.plugin({ name: 'plan-view', inject: planViewInject, apply: applyPlanView })
    await ctx.plugin({ name: 'stats-view', inject: statsViewInject, apply: applyStatsView })
    await ctx.plugin({ name: 'acp-session-status', inject: sessionStatusInject, apply: applySessionStatus })
    await ctx.plugin({ name: 'status-view', inject: statusViewInject, apply: applyStatusView })
    await ctx.plugin({ name: 'deepseek-logo', inject: deepseekLogoInject, apply: applyDeepseekLogo })
    const localPlugins = installTuiLocalPlugins(ctx, {
      store: createTuiPluginStore({ root: options.artifactRoot }),
      packages: options.packagePlugins ?? [],
      tuiTheme: ctx.get('tuiTheme'),
      tuiPresets: ctx.get('tuiPresets'),
      tuiSlots: ctx.get('tuiSlots'),
      tuiCommands: ctx.get('tuiCommands'),
      tuiOverlay: ctx.get('tuiOverlay'),
      acpSessionConfig: ctx.get('acpSessionConfig'),
      acpSessionPlan: ctx.get('acpSessionPlan'),
      acpSessionStats: ctx.get('acpSessionStats'),
      acpSessionStatus: ctx.get('acpSessionStatus'),
    })
    await localPlugins.discover()
    await ctx.plugin({
      name: 'tui-cordis-client-runner',
      inject: [
        'tuiTheme', 'tuiPresets', 'tuiSlots', 'tuiCommands', 'tuiOverlay', 'acpSessionConfig',
        'acpSessionPlan', 'acpSessionStats', 'acpSessionStatus',
      ],
      apply: applyCordisClientRunner,
    })
    await ctx.plugin(
      {
        name: 'dsh-tui-shell',
        inject: [
          'acpClient', 'tuiTheme', 'tuiSlots', 'tuiCommands', 'tuiOverlay',
          'acpClientEvents', 'acpSessionConfig', 'tuiCordisClientRunner',
        ],
        apply: applyShell,
      },
      { extraArgs: options.extraArgs ?? [], tty: options.tty },
    )
  } else {
    applyTheme(ctx, presetConfig)
    applyOne(ctx)
    applyAyu(ctx)
    applyCatppuccin(ctx)
    applyGithub(ctx)
    applyKanagawa(ctx)
    applyEverforest(ctx)
    applyGruvbox(ctx)
    applyIceberg(ctx)
    applyNightOwl(ctx)
    applyOneHalf(ctx)
    applySolarized(ctx)
    applySlots(ctx)
    applyCommands(ctx)
    applyOverlay(ctx)
    applyPresets(ctx, presetConfig)
    applyMarttyPreset(ctx)
    applyAcpClient(ctx, acpConfig)
    applyPlanView(ctx)
    applyStatsView(ctx)
    applySessionStatus(ctx)
    applyStatusView(ctx)
    applyDeepseekLogo(ctx)
    const localPlugins = installTuiLocalPlugins(ctx, {
      store: createTuiPluginStore({ root: options.artifactRoot }),
      packages: options.packagePlugins ?? [],
      tuiTheme: ctx.tuiTheme,
      tuiPresets: ctx.tuiPresets,
      tuiSlots: ctx.tuiSlots,
      tuiCommands: ctx.tuiCommands,
      tuiOverlay: ctx.tuiOverlay,
      acpSessionConfig: ctx.acpSessionConfig,
      acpSessionPlan: ctx.acpSessionPlan,
      acpSessionStats: ctx.acpSessionStats,
      acpSessionStatus: ctx.acpSessionStatus,
    })
    await localPlugins.discover()
    applyCordisClientRunner(ctx)
    await applyShell(ctx, { extraArgs: options.extraArgs ?? [], tty: options.tty })
  }
  return ctx
}

/** Parse the Host registry snapshot passed across the process boundary. */
export function parseClientPluginsEnv(value) {
  if (value === undefined || value === '') return []
  let parsed
  try {
    parsed = JSON.parse(value)
  } catch (error) {
    throw new Error(`dsh-tui: invalid installed Client plugin registry JSON: ${error.message}`)
  }
  if (!Array.isArray(parsed)) {
    throw new Error('dsh-tui: installed Client plugin registry must be an array')
  }
  return parsed.map((entry, index) => {
    if (entry === null || typeof entry !== 'object' || Array.isArray(entry)) {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} must be an object`)
    }
    if (typeof entry.id !== 'string' || !/^[a-z0-9][a-z0-9-]*$/.test(entry.id)) {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} has an invalid id`)
    }
    if (!['theme', 'ui-preset'].includes(entry.kind)) {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} has an invalid kind`)
    }
    if (typeof entry.entry !== 'string') {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} has no file entry`)
    }
    let url
    try {
      url = new URL(entry.entry)
    } catch {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} has an invalid file entry`)
    }
    if (url.protocol !== 'file:') {
      throw new Error(`dsh-tui: installed Client plugin entry ${index} must use a file URL`)
    }
    return { id: entry.id, kind: entry.kind, entry: url.href }
  })
}

export function uiSettingsPath(extraArgs = [], env = process.env, userHome = homedir()) {
  for (let index = 0; index < extraArgs.length; index += 1) {
    if (extraArgs[index] === '--session-root' && typeof extraArgs[index + 1] === 'string') {
      return path.join(extraArgs[index + 1], 'settings.json')
    }
  }
  return path.join(marttyHome(env, userHome), 'settings.json')
}

export function legacyUiSettingsPaths(extraArgs = [], userHome = homedir()) {
  const paths = []
  for (let index = 0; index < extraArgs.length; index += 1) {
    if (extraArgs[index] === '--session-root' && typeof extraArgs[index + 1] === 'string') {
      paths.push(path.join(extraArgs[index + 1], 'dsh-tui-settings.json'))
      break
    }
  }
  paths.push(path.join(userHome, '.dsh-tui', 'sessions', 'dsh-tui-settings.json'))
  return [...new Set(paths)]
}

export function migrateLegacyUiSettings(settingsPath, legacyPaths) {
  if (existsSync(settingsPath)) return false
  for (const legacyPath of legacyPaths) {
    if (!existsSync(legacyPath)) continue
    try {
      const value = JSON.parse(readFileSync(legacyPath, 'utf8'))
      if (value === null || typeof value !== 'object' || Array.isArray(value)) continue
      mkdirSync(path.dirname(settingsPath), { recursive: true })
      copyFileSync(legacyPath, settingsPath, constants.COPYFILE_EXCL)
      return true
    } catch {
      // A malformed or racing legacy file must not block TUI startup.
    }
  }
  return false
}

/**
 * Parse `--agent` / `--agent-arg` from argv. Remaining flags pass through to Rust.
 * `--agent` is also forwarded via {@link painterArgs} so Terminal Auth can
 * re-exec the same command.
 * @param {string[]} argv
 * @returns {{ agent: { command: string, args: string[] }, rustArgs: string[] }}
 */
export function parseClientArgv(argv) {
  const rustArgs = []
  let command
  const args = []
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i]
    if (token === '--agent') {
      command = argv[i + 1]
      i += 1
      continue
    }
    if (token === '--agent-arg') {
      args.push(argv[i + 1] ?? '')
      i += 1
      continue
    }
    rustArgs.push(token)
  }
  if (command === undefined) {
    return { agent: resolveStackedAgent(), rustArgs }
  }
  return { agent: { command, args }, rustArgs }
}

/**
 * Extra argv for the native painter: user flags plus `--agent` so `/auth`
 * can launch `{command} login` the same way Backchat launches Terminal Auth.
 * @param {{ agent: { command: string, args?: string[] }, rustArgs: string[] }} parsed
 * @returns {string[]}
 */
export function painterArgs(parsed) {
  const flags = []
  if (parsed.agent?.command) {
    flags.push('--agent', parsed.agent.command)
    for (const arg of parsed.agent.args ?? []) {
      flags.push('--agent-arg', arg)
    }
  }
  return [...parsed.rustArgs, ...flags]
}

/**
 * Cordis client boot: tuiTheme + tuiSlots + acp-client + TUI shell.
 *
 * The process root is a client tree. A profile launch injects Host ACP stdio;
 * standalone launches may spawn an agent instead. This module does not import
 * dsh or dsh-acp.
 */

import { apply as applyAcpClient } from './acp-client.js'
import { apply as applyCordisClientRunner } from './inspect.js'
import { apply as applyShell } from './index.js'
import { resolveStackedAgent } from './agent.js'
import { apply as applySlots } from './tui-slots.js'
import { apply as applyTheme } from './tui-theme.js'
import { apply as applyCommands } from './tui-commands.js'
import { apply as applyOverlay } from './tui-overlay.js'
import { apply as applyPlanView, inject as planViewInject } from './plan-view.js'
import { apply as applyStatsView, inject as statsViewInject } from './stats-view.js'
import { apply as applySessionStatus, inject as sessionStatusInject } from './acp-session-status.js'
import { apply as applyStatusView, inject as statusViewInject } from './status-view.js'
import { apply as applyDeepseekLogo, inject as deepseekLogoInject } from './deepseek-logo.js'

/**
 * @param {object} [options]
 * @param {{ command: string, args?: string[] }} [options.agent]
 * @param {{ stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, child?: import('node:child_process').ChildProcess }} [options.stream]
 * @param {{ stdin: number | 'inherit', stdout: number | 'inherit' }} [options.tty]
 */
export async function bootClient(options = {}) {
  const { Context } = await import('@deepseek-ai/cordis')
  const ctx = new Context()
  const acpConfig = options.stream !== undefined
    ? { stream: options.stream }
    : { agent: options.agent ?? resolveStackedAgent() }
  if (typeof ctx.plugin === 'function') {
    await ctx.plugin({ name: 'tui-theme', inject: [], apply: applyTheme })
    await ctx.plugin({ name: 'tui-slots', inject: [], apply: applySlots })
    await ctx.plugin({ name: 'tui-commands', inject: [], apply: applyCommands })
    await ctx.plugin({ name: 'tui-overlay', inject: [], apply: applyOverlay })
    await ctx.plugin({ name: 'acp-client', inject: [], apply: applyAcpClient }, acpConfig)
    await ctx.plugin({ name: 'plan-view', inject: planViewInject, apply: applyPlanView })
    await ctx.plugin({ name: 'stats-view', inject: statsViewInject, apply: applyStatsView })
    await ctx.plugin({ name: 'acp-session-status', inject: sessionStatusInject, apply: applySessionStatus })
    await ctx.plugin({ name: 'status-view', inject: statusViewInject, apply: applyStatusView })
    await ctx.plugin({ name: 'deepseek-logo', inject: deepseekLogoInject, apply: applyDeepseekLogo })
    await ctx.plugin({
      name: 'tui-cordis-client-runner',
      inject: [
        'tuiTheme', 'tuiSlots', 'tuiCommands', 'tuiOverlay', 'acpSessionConfig',
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
    applyTheme(ctx)
    applySlots(ctx)
    applyCommands(ctx)
    applyOverlay(ctx)
    applyAcpClient(ctx, acpConfig)
    applyPlanView(ctx)
    applyStatsView(ctx)
    applySessionStatus(ctx)
    applyStatusView(ctx)
    applyDeepseekLogo(ctx)
    applyCordisClientRunner(ctx)
    await applyShell(ctx, { extraArgs: options.extraArgs ?? [], tty: options.tty })
  }
  return ctx
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

/**
 * Cordis plugin: attach to any ACP agent by spawn or an existing stream.
 *
 * Provides `ctx.acpClient` plus the standard-ACP-backed
 * `ctx.acpSessionConfig`. Does not import a harness, dsh, or dsh-acp.
 * Switching agents is `{ command, args }` (or `config.stream`).
 */

import { spawn } from 'node:child_process'
import { installAcpClientEvents } from './acp-client-events.js'
import { installAcpSessionConfig } from './acp-session-config.js'
import { installAcpSessionPlan } from './acp-session-plan.js'
import { installAcpSessionStats } from './acp-session-stats.js'

export { installAcpSessionConfig } from './acp-session-config.js'
export { installAcpSessionPlan } from './acp-session-plan.js'
export { installAcpSessionStats } from './acp-session-stats.js'
export { installAcpClientEvents } from './acp-client-events.js'

export const name = 'acp-client'
export const inject = []

/** @type {null | { command: string, args: string[], child: import('node:child_process').ChildProcess, stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, kind: 'spawn' }} */
let liveAgent = null

/**
 * One kill-on-exit hook per process, replaced (not accumulated) whenever a
 * new agent is spawned. Registering `process.once('exit', …)` per apply()
 * would stack listeners and kill every past child again on exit.
 * @type {null | (() => void)}
 */
let liveAgentExitHook = null

/**
 * @typedef {{ command: string, args?: string[], env?: Record<string, string> }} AgentSpec
 */

/**
 * @param {unknown} config
 * @returns {AgentSpec}
 */
export function resolveAgent(config = {}) {
  const fromConfig = config && typeof config === 'object' ? config.agent : undefined
  if (fromConfig && typeof fromConfig === 'object' && typeof fromConfig.command === 'string') {
    return {
      command: fromConfig.command,
      args: Array.isArray(fromConfig.args) ? fromConfig.args.map(String) : [],
      ...(fromConfig.env !== undefined ? { env: fromConfig.env } : {}),
    }
  }
  const envCmd = process.env.DSH_TUI_AGENT
  if (typeof envCmd === 'string' && envCmd.trim().length > 0) {
    const tokens = envCmd.trim().split(/\s+/)
    return { command: tokens[0], args: tokens.slice(1) }
  }
  return { command: 'dsh-acp', args: [] }
}

/**
 * @param {object} ctx
 * @param {object} [config]
 */
export function apply(ctx, config = {}) {
  const events = installAcpClientEvents(ctx)
  const sessionConfig = installAcpSessionConfig(ctx)
  const sessionPlan = installAcpSessionPlan(ctx)
  const sessionStats = installAcpSessionStats(ctx)
  events.register(sessionConfig)
  events.register(sessionPlan)
  events.register(sessionStats)
  if (config.stream !== undefined && config.stream !== null) {
    const service = {
      kind: 'stream',
      stdin: config.stream.stdin,
      stdout: config.stream.stdout,
      child: config.stream.child,
    }
    provide(ctx, service)
    return
  }
  const agent = resolveAgent(config)
  if (
    liveAgent !== null
    && liveAgent.child.exitCode === null
    && liveAgent.command === agent.command
    && JSON.stringify(liveAgent.args) === JSON.stringify(agent.args ?? [])
  ) {
    provide(ctx, liveAgent)
    return
  }
  // Replacing the agent: the previous child must not outlive its spec.
  if (liveAgent !== null && liveAgent.child.exitCode === null) {
    try {
      liveAgent.child.kill('SIGTERM')
    } catch {
      // already gone
    }
  }
  liveAgent = null
  const child = spawn(agent.command, agent.args ?? [], {
    stdio: ['pipe', 'pipe', 'inherit'],
    env: { ...process.env, ...(agent.env ?? {}) },
  })
  child.stdin.on('error', () => {})
  child.stdout.on('error', () => {})
  child.on('error', (err) => {
    // A failed spawn (ENOENT, EACCES) emits 'error' on the child; without a
    // listener it is an uncaught exception. Surface EOF to the transport
    // instead so pending requests fail instead of hanging, and drop the
    // cached handle so the next apply() retries the spawn.
    console.error(`acp-client: failed to spawn agent ${agent.command}: ${err.message}`)
    if (liveAgent?.child === child) liveAgent = null
    child.stdin.destroy()
    child.stdout.destroy()
  })
  const service = {
    kind: 'spawn',
    command: agent.command,
    args: agent.args ?? [],
    stdin: child.stdin,
    stdout: child.stdout,
    child,
  }
  liveAgent = service
  provide(ctx, service)
  if (liveAgentExitHook !== null) {
    process.removeListener('exit', liveAgentExitHook)
  }
  liveAgentExitHook = () => {
    try {
      child.kill('SIGTERM')
    } catch {
      // already gone
    }
  }
  process.once('exit', liveAgentExitHook)
}

function provide(ctx, service) {
  if (typeof ctx.provide === 'function') {
    ctx.provide('acpClient', service)
  } else {
    ctx.acpClient = service
  }
}

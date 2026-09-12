/**
 * Cordis plugin: attach to any ACP agent by spawn or an existing stream.
 *
 * Provides `ctx.acpClient` plus the standard-ACP-backed
 * `ctx.acpSessionConfig`. Does not import a harness, dsh, or dsh-acp.
 * Spawned sessions retain their connection; setDefaultAgent changes the recipe
 * used by the next session/new. An externally owned config.stream stays fixed.
 */

import spawn from 'cross-spawn'
import { installAcpClientEvents } from './acp-client-events.js'
import { installAcpSessionConfig } from './acp-session-config.js'
import { installAcpSessionPlan } from './acp-session-plan.js'
import { installAcpSessionStats } from './acp-session-stats.js'
import { tokenizeCommandArgs } from './command-args.js'
import { createAgentPool } from './acp-agent-pool.js'

export { installAcpSessionConfig } from './acp-session-config.js'
export { installAcpSessionPlan } from './acp-session-plan.js'
export { installAcpSessionStats } from './acp-session-stats.js'
export { installAcpClientEvents } from './acp-client-events.js'

export const name = 'acp-client'
export const inject = []

/** @type {null | { command: string, args: string[], env?: Record<string, string>, child: import('node:child_process').ChildProcess, stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, kind: 'spawn' }} */
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
    const tokens = tokenizeCommandArgs(envCmd)
    if (!tokens[0]) throw new Error('DSH_TUI_AGENT needs a non-empty command')
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
    && !liveAgent.child.killed
    && !liveAgent.stdin.destroyed
    && liveAgent.command === agent.command
    && JSON.stringify(liveAgent.args) === JSON.stringify(agent.args ?? [])
    && JSON.stringify(liveAgent.env ?? {}) === JSON.stringify(agent.env ?? {})
  ) {
    provide(ctx, liveAgent)
    return
  }
  // A new Client tree owns a new pool; dispose every child of the old tree.
  if (liveAgent !== null) {
    try {
      liveAgent.close()
    } catch {
      // already gone
    }
  }
  liveAgent = null
  const service = createAgentPool(agent, { spawnAgent, resolveAgent, diagnosticError })
  if (liveAgentExitHook !== null) process.removeListener('exit', liveAgentExitHook)
  liveAgentExitHook = () => service.close()
  process.once('exit', liveAgentExitHook)
  liveAgent = service
  provide(ctx, service)
}

function spawnAgent(agent) {
  const child = spawn(agent.command, agent.args ?? [], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, ...(agent.env ?? {}) },
  })
  child.stdin.on('error', () => {})
  child.stdout.on('error', () => {})
  const diagnostics = drainDiagnostics(child.stderr)
  return {
    kind: 'spawn',
    command: agent.command,
    args: agent.args ?? [],
    ...(agent.env !== undefined ? { env: { ...agent.env } } : {}),
    stdin: child.stdin,
    stdout: child.stdout,
    diagnostics,
    child,
  }
}

function drainDiagnostics(stream) {
  const limit = 8192
  let tail = Buffer.alloc(0)
  let state = 'text'
  // stderr is not a UI surface. Drain it even without a diagnostics consumer,
  // strip control strings incrementally, and never retain their payloads.
  stream.setEncoding('utf8')
  stream.on('error', () => {})
  stream.on('data', (chunk) => {
    let text = ''
    for (const char of chunk) {
      const code = char.codePointAt(0)
      if (state === 'osc' || state === 'string') {
        if (code === 0x9c || (state === 'osc' && code === 7)) state = 'text'
        else if (code === 27) state += '-escape'
        continue
      }
      if (state === 'osc-escape' || state === 'string-escape') {
        if (char === '\\' || code === 0x9c || (state === 'osc-escape' && code === 7)) state = 'text'
        else if (code !== 27) state = state.replace('-escape', '')
        continue
      }
      if (code === 27) { state = 'escape'; continue }
      if (state === 'escape') {
        if (char === '[') state = 'csi'
        else if (char === ']') state = 'osc'
        else if (['P', 'X', '^', '_'].includes(char)) state = 'string'
        else state = code >= 0x20 && code <= 0x2f ? 'intermediate' : 'text'
        continue
      }
      if (state === 'csi' || state === 'intermediate') {
        if (code >= (state === 'csi' ? 0x40 : 0x30) && code <= 0x7e) state = 'text'
        continue
      }
      if (code === 0x9b) { state = 'csi'; continue }
      if (code === 0x9d) { state = 'osc'; continue }
      if ([0x90, 0x98, 0x9e, 0x9f].includes(code)) { state = 'string'; continue }
      if ((code < 0x20 && code !== 10) || (code >= 0x7f && code <= 0x9f)) continue
      text += char
    }
    if (text.length === 0) return
    const combined = Buffer.concat([tail, Buffer.from(text)])
    let start = Math.max(0, combined.length - limit)
    // Never split a UTF-8 code point or retain the large incoming allocation.
    while (start < combined.length && (combined[start] & 0xc0) === 0x80) start++
    tail = Buffer.from(combined.subarray(start))
  })
  return () => tail.toString('utf8').trim()
}

function diagnosticError(error, handle) {
  if (typeof error?.diagnostics === 'string') return error
  const diagnostics = handle.diagnostics()
  if (!diagnostics) return error
  return Object.assign(new Error(`${error.message}\nAgent stderr:\n${diagnostics}`, { cause: error }), {
    ...error, diagnostics,
  })
}

function provide(ctx, service) {
  if (typeof ctx.provide === 'function') {
    ctx.provide('acpClient', service)
  } else {
    ctx.acpClient = service
  }
}

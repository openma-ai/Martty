/**
 * Cordis plugin: attach to any ACP agent by spawn or an existing stream.
 *
 * Provides `ctx.acpClient` plus the standard-ACP-backed
 * `ctx.acpSessionConfig`. Does not import a harness, dsh, or dsh-acp.
 * Switching agents is `{ command, args }` (or `config.stream`).
 */

import { spawn } from 'node:child_process'
import { installAcpSessionConfig } from './acp-session-config.js'

export { installAcpSessionConfig } from './acp-session-config.js'

export const name = 'acp-client'
export const inject = []

/** @type {null | { command: string, args: string[], child: import('node:child_process').ChildProcess, stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, kind: 'spawn' }} */
let liveAgent = null

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
  installAcpSessionConfig(ctx)
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
  const child = spawn(agent.command, agent.args ?? [], {
    stdio: ['pipe', 'pipe', 'inherit'],
    env: { ...process.env, ...(agent.env ?? {}) },
  })
  child.stdin.on('error', () => {})
  child.stdout.on('error', () => {})
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
  process.once('exit', () => {
    try {
      child.kill('SIGTERM')
    } catch {
      // already gone
    }
  })
}

function provide(ctx, service) {
  if (typeof ctx.provide === 'function') {
    ctx.provide('acpClient', service)
  } else {
    ctx.acpClient = service
  }
}

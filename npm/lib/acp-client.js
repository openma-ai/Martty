/**
 * Cordis plugin: attach to any ACP agent by spawn or an existing stream.
 *
 * Provides `ctx.acpClient` plus the standard-ACP-backed
 * `ctx.acpSessionConfig`. Does not import a harness, dsh, or dsh-acp.
 * Switching agents is `{ command, args }` (or `config.stream`).
 */

import { spawn } from 'node:child_process'
import { PassThrough } from 'node:stream'
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
  const service = createSpawnService(agent)
  liveAgent = service
  provide(ctx, service)
}

function spawnAgent(agent) {
  const child = spawn(agent.command, agent.args ?? [], {
    stdio: ['pipe', 'pipe', 'inherit'],
    env: { ...process.env, ...(agent.env ?? {}) },
  })
  child.stdin.on('error', () => {})
  child.stdout.on('error', () => {})
  return {
    kind: 'spawn',
    command: agent.command,
    args: agent.args ?? [],
    stdin: child.stdin,
    stdout: child.stdout,
    child,
  }
}

function waitForSpawn(handle) {
  if (handle.child.pid !== undefined) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const spawned = () => {
      handle.child.off('error', failed)
      resolve()
    }
    const failed = (error) => {
      handle.child.off('spawn', spawned)
      reject(error)
    }
    handle.child.once('spawn', spawned)
    handle.child.once('error', failed)
  })
}

function createSpawnService(agent) {
  const input = new PassThrough()
  const output = new PassThrough()
  const switchListeners = new Set()
  let current = spawnAgent(agent)
  let closed = false

  const attach = (handle) => {
    input.pipe(handle.stdin, { end: false })
    handle.stdout.pipe(output, { end: false })
  }
  const detach = (handle) => {
    input.unpipe(handle.stdin)
    handle.stdout.unpipe(output)
  }
  attach(current)

  const service = {
    kind: 'spawn',
    command: current.command,
    args: current.args,
    stdin: input,
    stdout: output,
    child: current.child,
    onSwitch(listener) {
      if (typeof listener !== 'function') throw new Error('acpClient.onSwitch needs a function')
      switchListeners.add(listener)
      return () => switchListeners.delete(listener)
    },
    async switchAgent(nextAgent) {
      if (closed) throw new Error('acpClient is closed')
      const spec = resolveAgent({ agent: nextAgent })
      const next = spawnAgent(spec)
      try {
        await waitForSpawn(next)
        for (const listener of switchListeners) await listener(next, current)
      } catch (error) {
        try {
          next.child.kill('SIGTERM')
        } catch {
          // failed spawns may not own a process
        }
        throw error
      }
      const previous = current
      detach(previous)
      attach(next)
      current = next
      service.command = next.command
      service.args = next.args
      service.child = next.child
      watchCurrent(next)
      try {
        previous.child.kill('SIGTERM')
      } catch {
        // already gone
      }
    },
    close() {
      if (closed) return
      closed = true
      detach(current)
      try {
        current.child.kill('SIGTERM')
      } catch {
        // already gone
      }
      input.destroy()
      output.destroy()
      if (liveAgent === service) liveAgent = null
    },
  }

  function watchCurrent(handle) {
    handle.child.on('error', (err) => {
      if (current !== handle || closed) return
      console.error(`acp-client: failed to spawn agent ${handle.command}: ${err.message}`)
      service.close()
    })
  }
  watchCurrent(current)
  process.once('exit', () => {
    try {
      service.close()
    } catch {
      // already gone
    }
  })
  return service
}

function provide(ctx, service) {
  if (typeof ctx.provide === 'function') {
    ctx.provide('acpClient', service)
  } else {
    ctx.acpClient = service
  }
}

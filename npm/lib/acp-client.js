/**
 * Cordis plugin: attach to any ACP agent by spawn or an existing stream.
 *
 * Provides `ctx.acpClient` plus the standard-ACP-backed
 * `ctx.acpSessionConfig`. Does not import a harness, dsh, or dsh-acp.
 * Switching agents is `{ command, args }` (or `config.stream`).
 */

import spawn from 'cross-spawn'
import { PassThrough } from 'node:stream'
import { installAcpClientEvents } from './acp-client-events.js'
import { installAcpSessionConfig } from './acp-session-config.js'
import { installAcpSessionPlan } from './acp-session-plan.js'
import { installAcpSessionStats } from './acp-session-stats.js'
import { tokenizeCommandArgs } from './command-args.js'

export { installAcpSessionConfig } from './acp-session-config.js'
export { installAcpSessionPlan } from './acp-session-plan.js'
export { installAcpSessionStats } from './acp-session-stats.js'
export { installAcpClientEvents } from './acp-client-events.js'

export const name = 'acp-client'
export const inject = []

/** @type {null | { command: string, args: string[], env?: Record<string, string>, child: import('node:child_process').ChildProcess, stdin: import('node:stream').Writable, stdout: import('node:stream').Readable, kind: 'spawn' }} */
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
    && liveAgent.command === agent.command
    && JSON.stringify(liveAgent.args) === JSON.stringify(agent.args ?? [])
    && JSON.stringify(liveAgent.env ?? {}) === JSON.stringify(agent.env ?? {})
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
  const failureListeners = new Set()
  let current = spawnAgent(agent)
  let closed = false
  let pendingSwitch
  let hasSwitched = false

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
    ...(current.env !== undefined ? { env: current.env } : {}),
    stdin: input,
    stdout: output,
    child: current.child,
    diagnostics() { return current.diagnostics() },
    onSwitch(listener) {
      if (typeof listener !== 'function') throw new Error('acpClient.onSwitch needs a function')
      switchListeners.add(listener)
      return () => switchListeners.delete(listener)
    },
    onFailure(listener) {
      if (typeof listener !== 'function') throw new Error('acpClient.onFailure needs a function')
      failureListeners.add(listener)
      return () => failureListeners.delete(listener)
    },
    observeClient(message) {
      if (pendingSwitch === undefined || message?.id === undefined) return
      if (!['initialize', 'authenticate', 'session/new'].includes(message.method)) return
      pendingSwitch.requests.set(message.id, message.method)
      // Browser/device sign-in is user-driven, not a machine setup operation.
      if (message.method === 'authenticate') pendingSwitch.pause()
      else pendingSwitch.arm()
    },
    observeAgent(message) {
      const pending = pendingSwitch
      if (pending === undefined || message?.id === undefined) return
      const method = pending.requests.get(message.id)
      if (method === undefined) return
      pending.requests.delete(message.id)
      if (message.error !== undefined) {
        // Authentication is user-driven; do not time out while an auth form is open.
        if (method !== 'initialize' && message.error?.code === -32000) {
          pending.pause()
          return
        }
        pending.reject(Object.assign(new Error(`${method}: ${message.error?.message ?? 'ACP setup failed'}`), {
          method, acpError: structuredClone(message.error),
        }))
        return
      }
      if (method === 'initialize') {
        if (!Number.isInteger(message.result?.protocolVersion)) {
          pending.reject(new Error('initialize: invalid ACP response'))
          return
        }
        pending.initialized = true
        pending.server = message.result?.agentInfo?.name
      } else if (method === 'authenticate') {
        // Once sign-in returns, the ensuing session/new must finish promptly.
        pending.arm()
      } else if (method === 'session/new') {
        const sessionId = message.result?.sessionId
        if (!pending.initialized || typeof sessionId !== 'string' || sessionId.length === 0) {
          pending.reject(new Error('session/new: invalid ACP session response'))
          return
        }
        pending.resolve({
          sessionId,
          ...(typeof pending.server === 'string' ? { server: pending.server } : {}),
        })
      }
    },
    async switchAgent(nextAgent, { timeoutMs = 20 * 60_000 } = {}) {
      if (closed) throw new Error('acpClient is closed')
      if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) throw new Error('ACP setup timeout must be positive')
      const spec = resolveAgent({ agent: nextAgent })
      const next = spawnAgent(spec)
      try {
        await waitForSpawn(next)
        for (const listener of switchListeners) await listener(next, current)
        if (next.child.exitCode !== null || next.child.signalCode !== null) {
          throw new Error(`ACP process exited (${next.child.signalCode ?? next.child.exitCode}) during handoff`)
        }
      } catch (error) {
        try {
          next.child.kill('SIGTERM')
        } catch {
          // failed spawns may not own a process
        }
        throw diagnosticError(error, next)
      }
      pendingSwitch?.reject(new Error('Harness switch was superseded'))
      const previous = current
      detach(previous)
      attach(next)
      current = next
      service.command = next.command
      service.args = next.args
      service.env = next.env
      service.child = next.child
      hasSwitched = true
      const ready = waitForReady(timeoutMs)
      watchCurrent(next)
      try {
        previous.child.kill('SIGTERM')
      } catch {
        // already gone
      }
      // Rust starts initialize only after the local command returns. Awaiting
      // ready here would deadlock; callers persist selection when this settles.
      return { ready }
    },
    close(error = new Error('acpClient is closed')) {
      if (closed) return
      failTransport(error)
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

  function waitForReady(timeoutMs) {
    let resolve
    let reject
    let timer
    const ready = new Promise((yes, no) => { resolve = yes; reject = no })
    // A child may fail before the caller has received the handoff result.
    ready.catch(() => {})
    const pending = {
      requests: new Map(),
      initialized: false,
      server: undefined,
      arm() {
        clearTimeout(timer)
        timer = setTimeout(() => {
          failTransport(new Error(`ACP setup timed out after ${timeoutMs / 1000}s`))
          detach(current)
          try { current.child.kill('SIGTERM') } catch { /* already gone */ }
        }, timeoutMs)
        timer.unref?.()
      },
      pause() { clearTimeout(timer) },
      resolve(value) { settle(resolve, value) },
      reject(error) { settle(reject, diagnosticError(error, current)) },
    }
    const settle = (complete, value) => {
      if (pendingSwitch !== pending) return
      pendingSwitch = undefined
      clearTimeout(timer)
      complete(value)
    }
    pendingSwitch = pending
    pending.arm()
    return ready
  }

  function failTransport(error) {
    const failure = diagnosticError(error, current)
    pendingSwitch?.reject(failure)
    for (const listener of failureListeners) listener(failure)
  }

  function watchCurrent(handle) {
    handle.child.on('error', (err) => {
      if (current !== handle || closed) return
      if (hasSwitched) {
        failTransport(err)
        return
      }
      service.close(err)
    })
    // 'close' follows the stdio drain, so final startup errors are not lost.
    handle.child.once('close', (code, signal) => {
      if (current !== handle || closed) return
      failTransport(new Error(`ACP process exited (${signal ?? code ?? 'unknown'}) before completing the request`))
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

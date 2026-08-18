/** Standard-ACP-backed token and live prompt timing projection. */

import { Service } from '@deepseek-ai/cordis'

export const name = 'acp-session-stats'
export const inject = []

const SETUP_METHODS = new Set(['session/new', 'session/load'])

class AcpSessionStatsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpSessionStats')
    this.core = core
  }

  current() { return this.core.current() }
  subscribe(listener) { return this.core.subscribe(this.ctx, listener) }
  observeClient(message) { return this.core.observeClient(message) }
  observeAgent(message) { return this.core.observeAgent(message) }
}

function zero(sessionId) {
  return {
    sessionId,
    usage: {
      input: 0, output: 0, cached: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0,
    },
    stats: {
      turns: 0,
      steps: 0,
      llmMillis: 0,
      toolMillis: 0,
      ttftTotalMillis: 0,
      ttftCount: 0,
    },
  }
}

export function installAcpSessionStats(ctx, options = {}) {
  const now = typeof options.now === 'function' ? options.now : () => performance.now()
  const listeners = new Set()
  const pendingSetup = new Map()
  const pendingPrompts = new Map()
  const activePrompts = new Map()
  const toolStarts = new Map()
  let value = zero(undefined)

  function current() { return structuredClone(value) }

  function publish() {
    const snapshot = current()
    for (const listener of [...listeners]) listener(snapshot)
  }

  function reset(sessionId) {
    value = zero(sessionId)
    activePrompts.clear()
    pendingPrompts.clear()
    toolStarts.clear()
    publish()
  }

  function subscribe(effectCtx, listener) {
    if (typeof listener !== 'function') {
      throw new Error('acpSessionStats.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'acpSessionStats.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observeClient(message) {
    if (!object(message) || message.id === undefined || typeof message.method !== 'string') return
    if (SETUP_METHODS.has(message.method)) {
      pendingSetup.set(message.id, {
        sessionId: message.method === 'session/load'
          ? readString(message.params, 'sessionId', 'session_id')
          : undefined,
      })
      return
    }
    if (message.method !== 'session/prompt') return
    const sessionId = readString(message.params, 'sessionId', 'session_id')
    if (sessionId === undefined) return
    if (value.sessionId === undefined) value.sessionId = sessionId
    if (value.sessionId !== sessionId) return
    const prompt = { sessionId, started: now(), firstToken: undefined, toolMillis: 0 }
    pendingPrompts.set(message.id, prompt)
    activePrompts.set(sessionId, prompt)
    value.stats.turns += 1
    publish()
  }

  function observeAgent(message) {
    if (!object(message)) return
    if (message.id !== undefined && pendingSetup.has(message.id)) {
      const tracked = pendingSetup.get(message.id)
      pendingSetup.delete(message.id)
      if (message.error !== undefined || !object(message.result)) return
      reset(readString(message.result, 'sessionId', 'session_id') ?? tracked.sessionId)
      return
    }
    if (message.id !== undefined && pendingPrompts.has(message.id)) {
      const prompt = pendingPrompts.get(message.id)
      pendingPrompts.delete(message.id)
      activePrompts.delete(prompt.sessionId)
      if (message.error === undefined && object(message.result)) {
        addUsage(message.result.usage)
      }
      const elapsed = Math.max(0, now() - prompt.started)
      value.stats.llmMillis += Math.max(0, elapsed - prompt.toolMillis)
      publish()
      return
    }
    if (message.method !== 'session/update' || !object(message.params)) return
    const sessionId = readString(message.params, 'sessionId', 'session_id')
    if (value.sessionId !== undefined && sessionId !== value.sessionId) return
    if (value.sessionId === undefined) value.sessionId = sessionId
    const update = message.params.update
    if (!object(update)) return

    const replay = update?._meta?.dsh
    if (object(replay) && replay.event === 'prompt/usage') {
      value.usage = usageOf(replay.usage)
      publish()
      return
    }

    const type = readString(update, 'sessionUpdate', 'session_update')
    const prompt = sessionId === undefined ? undefined : activePrompts.get(sessionId)
    if (prompt !== undefined && prompt.firstToken === undefined
      && (type === 'agent_message_chunk' || type === 'agent_thought_chunk')
      && textOf(update.content).length > 0) {
      prompt.firstToken = now()
      value.stats.ttftTotalMillis += Math.max(0, prompt.firstToken - prompt.started)
      value.stats.ttftCount += 1
    }
    if (type === 'agent_message_chunk'
      && update?._meta?.dsh?.event === 'assistant_message') {
      value.stats.steps += 1
      publish()
      return
    }
    const callId = readString(update, 'toolCallId', 'tool_call_id')
    if (type === 'tool_call' && callId !== undefined) {
      toolStarts.set(`${sessionId ?? ''}\u0000${callId}`, now())
      return
    }
    if (type === 'tool_call_update' && callId !== undefined
      && ['completed', 'failed'].includes(update.status)) {
      const key = `${sessionId ?? ''}\u0000${callId}`
      const started = toolStarts.get(key)
      toolStarts.delete(key)
      if (started === undefined) return
      const duration = Math.max(0, now() - started)
      value.stats.toolMillis += duration
      if (prompt !== undefined) prompt.toolMillis += duration
      publish()
    }
  }

  function addUsage(usage) {
    const next = usageOf(usage)
    value.usage.input += next.input
    value.usage.output += next.output
    value.usage.cached += next.cached
    value.usage.cacheRead += next.cacheRead
    value.usage.cacheWrite += next.cacheWrite
    value.usage.reasoning += next.reasoning
  }

  const core = { current, subscribe, observeClient, observeAgent }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionStatsService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observeClient,
        observeAgent,
      }
  if (typeof ctx.provide !== 'function') ctx.acpSessionStats = service
  return service
}

function usageOf(value) {
  if (!object(value)) {
    return { input: 0, output: 0, cached: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0 }
  }
  const cacheRead = number(
    value.cachedReadTokens ?? value.cacheReadTokens ?? value.cached_read_tokens,
  )
  const cacheWrite = number(
    value.cachedWriteTokens ?? value.cacheWriteTokens ?? value.cached_write_tokens,
  )
  return {
    input: number(value.inputTokens ?? value.input_tokens),
    output: number(value.outputTokens ?? value.output_tokens),
    cached: cacheRead + cacheWrite,
    cacheRead,
    cacheWrite,
    reasoning: number(
      value.thoughtTokens ?? value.reasoningTokens ?? value.thought_tokens ?? value.reasoning_tokens,
    ),
  }
}

function textOf(value) {
  return object(value) && typeof value.text === 'string' ? value.text : ''
}

function number(value) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : 0
}

function readString(value, ...keys) {
  if (!object(value)) return undefined
  for (const key of keys) {
    if (typeof value[key] === 'string' && value[key].length > 0) return value[key]
  }
  return undefined
}

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

export function apply(ctx, options = {}) {
  installAcpSessionStats(ctx, options)
}

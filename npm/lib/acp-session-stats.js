/** Standard-ACP-backed token and live prompt timing projection. */

import { Service } from '@deepseek-ai/cordis'

export const name = 'acp-session-stats'
export const inject = []

const SETUP_METHODS = new Set(['session/new', 'session/load', 'session/resume'])

class AcpSessionStatsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpSessionStats')
    this.core = core
  }

  current() { return this.core.current() }
  subscribe(listener) { return this.core.subscribe(this.ctx, listener) }
  observeClient(message) { return this.core.observeClient(message) }
  observeAgent(message) { return this.core.observeAgent(message) }
  selectSession(sessionId) { return this.core.selectSession(sessionId) }
}

function zero(sessionId) {
  return {
    sessionId,
    usage: {
      input: 0, output: 0, cached: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0,
    },
    context: { used: 0, size: 0 },
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
  const values = new Map()
  let sessionId
  let selectionKnown = false

  function stateFor(targetSessionId) {
    if (typeof targetSessionId !== 'string' || targetSessionId.length === 0) {
      return zero(undefined)
    }
    let value = values.get(targetSessionId)
    if (value === undefined) {
      value = zero(targetSessionId)
      values.set(targetSessionId, value)
    }
    return value
  }

  function current() { return structuredClone(stateFor(sessionId)) }

  function publish() {
    const snapshot = current()
    for (const listener of [...listeners]) listener(snapshot)
  }

  function publishIf(targetSessionId) {
    if (targetSessionId === sessionId) publish()
  }

  function reset(targetSessionId) {
    if (typeof targetSessionId !== 'string' || targetSessionId.length === 0) return
    values.set(targetSessionId, zero(targetSessionId))
    activePrompts.delete(targetSessionId)
    for (const [id, prompt] of pendingPrompts) {
      if (prompt.sessionId === targetSessionId) pendingPrompts.delete(id)
    }
    for (const key of toolStarts.keys()) {
      if (key.startsWith(`${targetSessionId}\u0000`)) toolStarts.delete(key)
    }
    publishIf(targetSessionId)
  }

  function selectSession(nextSessionId) {
    selectionKnown = true
    sessionId = typeof nextSessionId === 'string' && nextSessionId.length > 0
      ? nextSessionId
      : undefined
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
        sessionId: message.method !== 'session/new'
          ? readString(message.params, 'sessionId', 'session_id')
          : undefined,
      })
      return
    }
    if (message.method !== 'session/prompt') return
    const promptSessionId = readString(message.params, 'sessionId', 'session_id')
    if (promptSessionId === undefined) return
    if (!selectionKnown && sessionId === undefined) sessionId = promptSessionId
    const prompt = {
      sessionId: promptSessionId,
      started: now(),
      firstToken: undefined,
      toolMillis: 0,
    }
    pendingPrompts.set(message.id, prompt)
    activePrompts.set(promptSessionId, prompt)
    stateFor(promptSessionId).stats.turns += 1
    publishIf(promptSessionId)
  }

  function observeAgent(message) {
    if (!object(message)) return
    if (message.id !== undefined && pendingSetup.has(message.id)) {
      const tracked = pendingSetup.get(message.id)
      pendingSetup.delete(message.id)
      if (message.error !== undefined || !object(message.result)) return
      const bound = readString(message.result, 'sessionId', 'session_id') ?? tracked.sessionId
      if (!selectionKnown) sessionId = bound
      reset(bound)
      return
    }
    if (message.id !== undefined && pendingPrompts.has(message.id)) {
      const prompt = pendingPrompts.get(message.id)
      pendingPrompts.delete(message.id)
      activePrompts.delete(prompt.sessionId)
      const value = stateFor(prompt.sessionId)
      if (message.error === undefined && object(message.result)) {
        addUsage(value, message.result.usage)
      }
      const elapsed = Math.max(0, now() - prompt.started)
      value.stats.llmMillis += Math.max(0, elapsed - prompt.toolMillis)
      publishIf(prompt.sessionId)
      return
    }
    if (message.method !== 'session/update' || !object(message.params)) return
    const updateSessionId = readString(message.params, 'sessionId', 'session_id')
    if (updateSessionId === undefined) return
    if (!selectionKnown && sessionId === undefined) sessionId = updateSessionId
    const value = stateFor(updateSessionId)
    const update = message.params.update
    if (!object(update)) return

    const replay = update?._meta?.dsh
    if (object(replay) && replay.event === 'prompt/usage') {
      value.usage = usageOf(replay.usage)
      publishIf(updateSessionId)
      return
    }

    const type = readString(update, 'sessionUpdate', 'session_update')
    if (type === 'usage_update') {
      // Harness-authoritative context gauge: `used` is the final prompt
      // size (input + cache + output) of the last step and `size` the
      // real model window from `request/context`. Client-side
      // accumulation cannot derive either: per-prompt input sums every
      // step's full context, and standard ACP does not carry the window.
      const size = number(update.size)
      if (size > 0) {
        value.context = { used: number(update.used), size }
        publishIf(updateSessionId)
      }
      return
    }
    const prompt = activePrompts.get(updateSessionId)
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
      publishIf(updateSessionId)
      return
    }
    const callId = readString(update, 'toolCallId', 'tool_call_id')
    if (type === 'tool_call' && callId !== undefined) {
      toolStarts.set(`${updateSessionId}\u0000${callId}`, now())
      return
    }
    if (type === 'tool_call_update' && callId !== undefined
      && ['completed', 'failed'].includes(update.status)) {
      const key = `${updateSessionId}\u0000${callId}`
      const started = toolStarts.get(key)
      toolStarts.delete(key)
      if (started === undefined) return
      const duration = Math.max(0, now() - started)
      value.stats.toolMillis += duration
      if (prompt !== undefined) prompt.toolMillis += duration
      publishIf(updateSessionId)
    }
  }

  function addUsage(value, usage) {
    const next = usageOf(usage)
    value.usage.input += next.input
    value.usage.output += next.output
    value.usage.cached += next.cached
    value.usage.cacheRead += next.cacheRead
    value.usage.cacheWrite += next.cacheWrite
    value.usage.reasoning += next.reasoning
  }

  const core = { current, subscribe, observeClient, observeAgent, selectSession }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionStatsService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observeClient,
        observeAgent,
        selectSession,
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

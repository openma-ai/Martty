import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const statsModule = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/acp-session-stats.js'),
).href).catch(() => ({}))

function makeCtx() {
  return {
    effect(fn) { return fn() },
    get() {},
    on() { return () => {} },
  }
}

test('standard ACP prompt traffic produces the durable stats footer facts', () => {
  assert.equal(typeof statsModule.installAcpSessionStats, 'function')
  if (typeof statsModule.installAcpSessionStats !== 'function') return

  let now = 1_000
  const stats = statsModule.installAcpSessionStats(makeCtx(), { now: () => now })
  stats.observeClient({ jsonrpc: '2.0', id: 1, method: 'session/new', params: {} })
  stats.observeAgent({ jsonrpc: '2.0', id: 1, result: { sessionId: 's-1' } })
  stats.observeClient({
    jsonrpc: '2.0', id: 2, method: 'session/prompt', params: { sessionId: 's-1', prompt: [] },
  })

  now = 1_200
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'hmm' } },
    },
  })
  now = 1_300
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-1',
      update: {
        sessionUpdate: 'tool_call', toolCallId: 'call-1', title: 'Read', status: 'in_progress',
      },
    },
  })
  now = 1_500
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'tool_call_update', toolCallId: 'call-1', status: 'completed' },
    },
  })
  now = 1_700
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-1',
      update: {
        sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: '' },
        _meta: { dsh: { event: 'assistant_message' } },
      },
    },
  })
  now = 1_700
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'usage_update', used: 12_300, size: 128_000 },
    },
  })
  now = 2_000
  stats.observeAgent({
    jsonrpc: '2.0', id: 2,
    result: {
      stopReason: 'end_turn',
      usage: {
        inputTokens: 100, outputTokens: 20, cacheReadTokens: 50,
        cacheWriteTokens: 5, reasoningTokens: 7,
      },
    },
  })

  assert.deepEqual(stats.current(), {
    sessionId: 's-1',
    usage: {
      input: 100, output: 20, cached: 55, cacheRead: 50, cacheWrite: 5, reasoning: 7,
    },
    context: { used: 12_300, size: 128_000 },
    stats: {
      turns: 1,
      steps: 1,
      llmMillis: 800,
      toolMillis: 200,
      ttftTotalMillis: 200,
      ttftCount: 1,
    },
  })
})

test('session load restores only the latest prompt usage snapshot', () => {
  assert.equal(typeof statsModule.installAcpSessionStats, 'function')
  if (typeof statsModule.installAcpSessionStats !== 'function') return

  const stats = statsModule.installAcpSessionStats(makeCtx(), { now: () => 0 })
  stats.observeClient({
    jsonrpc: '2.0', id: 'load', method: 'session/load', params: { sessionId: 's-old' },
  })
  stats.observeAgent({ jsonrpc: '2.0', id: 'load', result: { sessionId: 's-old' } })
  stats.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-old',
      update: {
        sessionUpdate: 'session_info_update',
        _meta: { dsh: { event: 'prompt/usage', usage: {
          inputTokens: 7, outputTokens: 2, cachedReadTokens: 3, cachedWriteTokens: 1,
        } } },
      },
    },
  })

  assert.deepEqual(stats.current().usage, {
    input: 7, output: 2, cached: 4, cacheRead: 3, cacheWrite: 1, reasoning: 0,
  })
  assert.deepEqual(stats.current().context, { used: 0, size: 0 })
  assert.equal(stats.current().stats.turns, 0)
})

test('concurrent Session statistics remain isolated across native tab switches', () => {
  let now = 0
  const stats = statsModule.installAcpSessionStats(makeCtx(), { now: () => now })
  stats.selectSession('s-1')
  stats.observeClient({
    jsonrpc: '2.0', id: 'p1', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  stats.observeClient({
    jsonrpc: '2.0', id: 'p2', method: 'session/prompt', params: { sessionId: 's-2' },
  })
  now = 10
  stats.observeAgent({
    jsonrpc: '2.0', id: 'p2',
    result: { usage: { inputTokens: 20, outputTokens: 2 } },
  })
  assert.equal(stats.current().stats.turns, 1)
  assert.equal(stats.current().usage.input, 0, 'background completion does not pollute s-1')

  stats.selectSession('s-2')
  assert.equal(stats.current().stats.turns, 1)
  assert.equal(stats.current().usage.input, 20)
  stats.selectSession('s-1')
  assert.equal(stats.current().usage.input, 0)
})

test('usage replay before the load response survives with background stats isolated', () => {
  const stats = statsModule.installAcpSessionStats(makeCtx())
  const context = (sessionId, used) => stats.observeAgent({
    method: 'session/update', params: { sessionId,
      update: { sessionUpdate: 'usage_update', used, size: 1000 } },
  })
  context('background', 90)
  stats.selectSession('restored')
  context('restored', 999)
  stats.observeClient({ id: 'load', method: 'session/load', params: { sessionId: 'restored' } })
  context('restored', 400)
  stats.observeAgent({ method: 'session/update', params: { sessionId: 'restored', update: {
    sessionUpdate: 'session_info_update', _meta: { dsh: { event: 'prompt/usage', usage: { inputTokens: 7 } } },
  } } })
  stats.observeAgent({ id: 'load', result: {} })
  assert.equal(stats.current().context.used, 400)
  assert.equal(stats.current().usage.input, 7)
  stats.selectSession('background')
  assert.equal(stats.current().context.used, 90)
})

test('a rejected steer cannot steal first-token and tool timing from the main prompt', () => {
  let now = 0
  const stats = statsModule.installAcpSessionStats(makeCtx(), { now: () => now })
  stats.selectSession('s')
  stats.observeClient({ id: 1, method: 'session/prompt', params: { sessionId: 's' } })
  now = 10
  stats.observeClient({ id: 2, method: 'session/prompt', params: { sessionId: 's' } })
  now = 20
  stats.observeAgent({ id: 2, error: { code: -1, message: 'busy' } })
  const update = (value) => stats.observeAgent({ method: 'session/update', params: { sessionId: 's', update: value } })
  now = 30
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'hello' } })
  now = 40
  update({ sessionUpdate: 'tool_call', toolCallId: 't' })
  now = 60
  update({ sessionUpdate: 'tool_call_update', toolCallId: 't', status: 'completed' })
  now = 100
  stats.observeAgent({ id: 1, result: {} })
  assert.deepEqual(stats.current().stats, {
    turns: 1, steps: 0, llmMillis: 80, toolMillis: 20, ttftTotalMillis: 30, ttftCount: 1,
  })
})

test('late steer completion cannot clear the next main prompt timing', () => {
  let now = 0
  const stats = statsModule.installAcpSessionStats(makeCtx(), { now: () => now })
  stats.selectSession('s')
  for (const id of [1, 2]) stats.observeClient({ id, method: 'session/prompt', params: { sessionId: 's' } })
  now = 10
  stats.observeAgent({ id: 1, result: {} })
  now = 20
  stats.observeClient({ id: 3, method: 'session/prompt', params: { sessionId: 's' } })
  now = 30
  stats.observeAgent({ id: 2, result: {} })
  now = 40
  stats.observeAgent({ method: 'session/update', params: { sessionId: 's', update: {
    sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'next turn' },
  } } })
  now = 50
  stats.observeAgent({ id: 3, result: {} })
  assert.equal(stats.current().stats.turns, 2)
  assert.equal(stats.current().stats.ttftCount, 1)
  assert.equal(stats.current().stats.ttftTotalMillis, 20)
  assert.equal(stats.current().stats.llmMillis, 40)
})

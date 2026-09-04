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

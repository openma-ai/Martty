import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const statsView = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/stats-view.js'),
).href).catch(() => ({}))

test('the stats Client Plugin contributes only through the composer dock', () => {
  assert.deepEqual(statsView.inject, ['acpSessionStats', 'tuiSlots'])
  assert.equal(typeof statsView.apply, 'function')
  if (typeof statsView.apply !== 'function') return

  let listener
  let nodes = []
  const ctx = {
    acpSessionStats: {
      current: () => ({
        sessionId: 's-1',
        usage: {
          input: 1834, output: 412, cached: 1200,
          cacheRead: 1200, cacheWrite: 0, reasoning: 0,
        },
        stats: {
          turns: 1, steps: 67, llmMillis: 15_000, toolMillis: 0,
          ttftTotalMillis: 1500, ttftCount: 1,
        },
      }),
      subscribe(next) { listener = next; return () => { listener = undefined } },
    },
    tuiSlots: {
      inject(name, callback) {
        assert.equal(name, 'conversation.composer.dock')
        return callback()
      },
      register(options, initial) {
        assert.deepEqual(options, { name: 'conversation.composer.dock', id: 'stats' })
        nodes = initial
        return { update(next) { nodes = next }, dispose() {} }
      },
    },
  }

  statsView.apply(ctx)
  // No `usage_update` yet: the gauge is hidden rather than guessed from
  // the model name (issue #77 — the old 128K guess pegged early 100%).
  assert.equal(nodes.length, 5)
  assert.deepEqual(nodes.map((node) => node.title), [
    '↑1.8K · ↓412',
    '1 turn · 67 steps',
    'Cache hit 40%',
    'LLM 15s · Tool call 0s',
    'TTFT avg 1.5s · 27.5 tok/s',
  ])

  listener({
    sessionId: 's-1',
    usage: {
      input: 0, output: 0, cached: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0,
    },
    stats: {
      turns: 0, steps: 0, llmMillis: 0, toolMillis: 0,
      ttftTotalMillis: 0, ttftCount: 0,
    },
  })
  assert.deepEqual(nodes, [])

  // The harness `usage_update` gauge wins over the model-name heuristic:
  // 17000/128000 = 13.3%, not the guessed 1.0M window of the model.
  listener({
    sessionId: 's-1',
    usage: {
      input: 5000, output: 300, cached: 4000, cacheRead: 4000, cacheWrite: 0, reasoning: 0,
    },
    context: { used: 17_000, size: 128_000 },
    stats: {
      turns: 2, steps: 4, llmMillis: 0, toolMillis: 0,
      ttftTotalMillis: 0, ttftCount: 0,
    },
  })
  assert.deepEqual(nodes.map((node) => node.title), [
    '↑5K · ↓300',
    '13.3%/128K',
    '2 turns · 4 steps',
    'Cache hit 44%',
  ])
})

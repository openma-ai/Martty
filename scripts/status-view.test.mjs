import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const statusView = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/status-view.js'),
).href).catch(() => ({}))

test('the status Client Plugin registers /status and opens the markdown overlay', () => {
  assert.deepEqual(
    statusView.inject,
    ['acpSessionStatus', 'acpSessionStats', 'tuiCommands', 'tuiOverlay'],
  )
  assert.equal(typeof statusView.apply, 'function')
  if (typeof statusView.apply !== 'function') return

  let registered
  let overlay
  const ctx = {
    acpSessionStatus: {
      current: () => ({
        state: 'running',
        connection: 'attached',
        server: 'dsh-acp',
        auth: { status: 'configured', method: 'agent' },
        session: { sessionId: 's-1', bound: true },
        model: 'deepseek-v4-flash',
        effort: 'high',
        permission: 'workspace-write',
        plan: true,
        agent: 'Standard',
      }),
    },
    acpSessionStats: {
      current: () => ({
        sessionId: 's-1',
        usage: {
          input: 1834, output: 412, cached: 1200,
          cacheRead: 1200, cacheWrite: 0, reasoning: 0,
        },
        stats: {
          turns: 3, steps: 47, llmMillis: 127_000, toolMillis: 8_000,
          ttftTotalMillis: 4_500, ttftCount: 3,
        },
      }),
    },
    tuiCommands: {
      register(options, handler) {
        registered = { options, handler }
        return () => { registered = undefined }
      },
    },
    tuiOverlay: {
      openView(options) {
        overlay = options
        return { close() {} }
      },
    },
  }

  statusView.apply(ctx)
  assert.equal(registered.options.name, 'status')
  assert.equal(registered.options.description, 'Session run state and key stats')

  registered.handler()
  assert.equal(overlay.id, 'status')
  assert.equal(overlay.title, 'Status')
  assert.equal(overlay.nodes.length, 1)
  assert.equal(overlay.nodes[0].kind, 'markdown')

  const text = overlay.nodes[0].text
  assert.ok(text.startsWith('- state · running'), text)
  assert.ok(text.includes('- state · running'), text)
  assert.ok(text.includes('- acp · attached'), text)
  assert.ok(text.includes('- auth · configured · agent'), text)
  assert.ok(text.includes('- session · s-1'), text)
  assert.ok(text.includes('- server · dsh-acp'), text)
  assert.ok(text.includes('- model · deepseek-v4-flash'), text)
  assert.ok(text.includes('- effort · high'), text)
  assert.ok(text.includes('- permission · workspace-write'), text)
  assert.ok(text.includes('- plan · on'), text)
  assert.ok(text.includes('- tokens · ↑1.8K ↓412 (cached 1.2K) · Σ 3.4K'), text)
  assert.ok(text.includes('- turns · 3 · steps · 47'), text)
  assert.ok(text.includes('- LLM · 2m7s · tool · 8s'), text)
  assert.ok(text.includes('- TTFT avg · 1.5s'), text)
  assert.ok(text.includes('- rate · 3.2 tok/s'), text)
})

test('the status overlay omits unset facts', () => {
  const { statusMarkdown } = statusView
  if (typeof statusMarkdown !== 'function') return
  const text = statusMarkdown(
    {
      state: 'idle',
      connection: 'connecting',
      session: { sessionId: undefined, bound: false },
    },
    { usage: {}, stats: {} },
  )
  assert.ok(text.includes('- session · unbound'), text)
  assert.ok(!text.includes('- auth ·'), text)
  assert.ok(!text.includes('- server ·'), text)
  assert.ok(!text.includes('- effort ·'), text)
  assert.ok(!text.includes('- TTFT avg ·'), text)
  assert.ok(!text.includes('- rate ·'), text)
  assert.ok(text.includes('- tokens · ↑0 ↓0 (cached 0) · Σ 0'), text)
})

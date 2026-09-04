import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const planModule = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/acp-session-plan.js'),
).href).catch(() => ({}))

function makeCtx() {
  return {
    effect(fn) {
      return fn()
    },
    get() {},
    on() {
      return () => {}
    },
  }
}

test('standard ACP plan snapshots stay structured and plan_removed retracts them', () => {
  assert.equal(typeof planModule.installAcpSessionPlan, 'function')
  if (typeof planModule.installAcpSessionPlan !== 'function') return

  const ctx = makeCtx()
  const plan = planModule.installAcpSessionPlan(ctx)
  const observed = []
  plan.subscribe((snapshot) => observed.push(snapshot))
  plan.observeClient({ jsonrpc: '2.0', id: 1, method: 'session/new', params: {} })
  plan.observeAgent({ jsonrpc: '2.0', id: 1, result: { sessionId: 's-1' } })
  plan.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: {
        sessionUpdate: 'plan_update',
        plan: {
          type: 'items',
          planId: 'primary',
          entries: [
            { content: 'Inspect', priority: 'high', status: 'in_progress' },
            { content: 'Implement', priority: 'medium', status: 'pending' },
          ],
        },
      },
    },
  })

  assert.deepEqual(plan.current(), {
    id: 'primary',
    kind: 'items',
    entries: [
      { content: 'Inspect', priority: 'high', status: 'in_progress' },
      { content: 'Implement', priority: 'medium', status: 'pending' },
    ],
  })
  assert.equal(observed.at(-1).sessionId, 's-1')

  plan.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'plan_removed', planId: 'primary' },
    },
  })
  assert.equal(plan.current(), null)
  assert.deepEqual(observed.at(-1).plans, [])
})

test('plan snapshots follow explicit native tab selection', () => {
  const plan = planModule.installAcpSessionPlan(makeCtx())
  for (const [id, sessionId, content] of [[11, 's-1', 'one'], [12, 's-2', 'two']]) {
    plan.observeClient({ jsonrpc: '2.0', id, method: 'session/load', params: { sessionId } })
    plan.observeAgent({ jsonrpc: '2.0', id, result: { sessionId } })
    plan.observeAgent({
      jsonrpc: '2.0', method: 'session/update', params: {
        sessionId,
        update: { sessionUpdate: 'plan', plan: { type: 'markdown', content } },
      },
    })
  }

  plan.selectSession('s-1')
  assert.equal(plan.current().content, 'one')
  plan.selectSession('s-2')
  assert.equal(plan.current().content, 'two')
})

import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const eventsModule = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/acp-client-events.js'),
).href).catch(() => ({}))

function makeCtx() {
  return {
    effect(fn) { return fn() },
    get() {},
    on() { return () => {} },
  }
}

test('ACP projections register independently behind one transport observer', () => {
  assert.equal(typeof eventsModule.installAcpClientEvents, 'function')
  if (typeof eventsModule.installAcpClientEvents !== 'function') return

  const ctx = makeCtx()
  const events = eventsModule.installAcpClientEvents(ctx)
  const seen = []
  const disposePlan = events.register({
    observeClient(message) { seen.push(['plan-client', message.method]) },
    observeAgent(message) { seen.push(['plan-agent', message.method]) },
  })
  events.register({
    observeAgent(message) { seen.push(['stats-agent', message.method]) },
  })

  events.observeClient({ method: 'session/prompt' })
  events.observeAgent({ method: 'session/update' })
  disposePlan()
  events.observeAgent({ method: 'session/update' })

  assert.deepEqual(seen, [
    ['plan-client', 'session/prompt'],
    ['plan-agent', 'session/update'],
    ['stats-agent', 'session/update'],
    ['stats-agent', 'session/update'],
  ])
})

test('ACP observer registration rejects a contribution with no observer', () => {
  assert.equal(typeof eventsModule.installAcpClientEvents, 'function')
  if (typeof eventsModule.installAcpClientEvents !== 'function') return
  const events = eventsModule.installAcpClientEvents(makeCtx())
  assert.throws(() => events.register({}), /observeClient|observeAgent/i)
})

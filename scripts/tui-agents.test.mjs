import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const tuiAgents = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-agents.js'),
).href).catch(() => ({}))

function makeCtx() {
  return {
    effect(setup) {
      return setup()
    },
  }
}

test('the native Agent projection publishes immutable session navigation snapshots', () => {
  assert.equal(typeof tuiAgents.installTuiAgents, 'function')
  if (typeof tuiAgents.installTuiAgents !== 'function') return

  const service = tuiAgents.installTuiAgents(makeCtx())
  const seen = []
  const stop = service.subscribe((snapshot) => seen.push(snapshot))

  assert.equal(service.observe({
    protocol: 0,
    activeId: 'root-1',
    selectedId: '__martty_internal__:agent-history',
    items: [
      { id: 'root-1', label: 'main', kind: 'main', status: 'running' },
      { id: 'child-1', label: 'subagent 1', kind: 'subagent', status: 'running', current: true },
      { id: 'child-2', label: 'subagent 2', kind: 'subagent', status: 'finished', current: false },
      { id: '__martty_internal__:agent-history', label: 'History (1)', kind: 'history', status: 'idle' },
    ],
  }), true)

  assert.deepEqual(service.current(), {
    activeId: 'root-1',
    selectedId: '__martty_internal__:agent-history',
    items: [
      { id: 'root-1', label: 'main', kind: 'main', status: 'running' },
      { id: 'child-1', label: 'subagent 1', kind: 'subagent', status: 'running', current: true },
      { id: 'child-2', label: 'subagent 2', kind: 'subagent', status: 'finished', current: false },
      { id: '__martty_internal__:agent-history', label: 'History (1)', kind: 'history', status: 'idle' },
    ],
  })
  assert.equal(seen.length, 1)

  seen[0].items[0].label = 'mutated outside'
  assert.equal(service.current().items[0].label, 'main')

  stop()
  service.observe({ protocol: 0, activeId: 'root-1', items: [] })
  assert.equal(seen.length, 1)
})

test('Agent selection and inline navigation stay on the local compositor transport', () => {
  assert.equal(typeof tuiAgents.installTuiAgents, 'function')
  if (typeof tuiAgents.installTuiAgents !== 'function') return

  const sent = []
  const service = tuiAgents.installTuiAgents(makeCtx())
  service.observe({
    protocol: 0,
    activeId: 'root-1',
    items: [
      { id: 'root-1', label: 'main', kind: 'main', status: 'idle' },
      { id: 'child-1', label: 'subagent 1', kind: 'subagent', status: 'running' },
    ],
  })
  service.bindNotify((method, params) => sent.push({ method, params }))

  assert.equal(service.select('child-1'), true)
  assert.equal(service.navigate('begin'), true)
  assert.deepEqual(sent, [
    {
      method: '_dsh/cordis/tui/agents/select',
      params: { protocol: 0, id: 'child-1' },
    },
    {
      method: '_dsh/cordis/tui/agents/navigate',
      params: { protocol: 0, action: 'begin' },
    },
  ])
  assert.equal(service.select('missing'), false)
  assert.equal(service.navigate('diagonal'), false)
  assert.equal(sent.length, 2)
})

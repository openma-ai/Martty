import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const agentsView = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/agents-view.js'),
).href).catch(() => ({}))

function snapshot(activeId = 'root-1', selectedId = null) {
  return {
    activeId,
    selectedId,
    items: [
      { id: 'root-1', label: 'main', kind: 'main', status: 'running' },
      { id: 'child-1', label: 'subagent 1', kind: 'subagent', status: 'running' },
      { id: 'child-2', label: 'subagent 2', kind: 'subagent', status: 'finished' },
    ],
  }
}

function harness(initial = snapshot()) {
  let current = initial
  let listener
  let nodes
  let command
  const selected = []
  const navigated = []
  const ctx = {
    tuiAgents: {
      current: () => structuredClone(current),
      subscribe(next) {
        listener = next
        return () => { listener = undefined }
      },
      select(id) {
        selected.push(id)
        return true
      },
      navigate(action) {
        navigated.push(action)
        return true
      },
    },
    tuiSlots: {
      inject(name, callback) {
        assert.equal(name, 'conversation.navigation.dock')
        return callback()
      },
      register(options, initialNodes) {
        assert.deepEqual(options, {
          name: 'conversation.navigation.dock', id: 'agents-view', order: 0,
        })
        nodes = initialNodes
        return { update(next) { nodes = next }, dispose() {} }
      },
    },
    tuiCommands: {
      register(options, handler) {
        assert.deepEqual(options, {
          name: 'agents',
          description: 'Toggle the Agent panel (on/off)',
          input: { hint: '[on|off|agent-id]' },
        })
        command = handler
        return () => {}
      },
    },
  }
  return {
    ctx,
    nodes: () => nodes,
    command: () => command,
    selected,
    navigated,
    update(next) {
      current = next
      listener(structuredClone(next))
    },
  }
}

test('Agent view stays collapsed until inline navigation begins', () => {
  assert.deepEqual(
    agentsView.inject,
    ['tuiAgents', 'tuiSlots', 'tuiCommands'],
  )
  assert.equal(typeof agentsView.apply, 'function')
  if (typeof agentsView.apply !== 'function') return

  const view = harness()
  agentsView.apply(view.ctx)

  assert.deepEqual(view.nodes(), [
    {
      id: 'summary', kind: 'generic', title: '· Agents', body: '1/2', status: 'running', tone: 'caption',
    },
    {
      id: 'switch', kind: 'generic', title: '↓ expand', body: '', tone: 'caption',
    },
  ])

  view.update(snapshot('child-1'))
  assert.equal(view.nodes().length, 2, 'switching transcripts does not expand the rail')
  assert.equal(view.nodes().some((node) => node.id.startsWith('agent-')), false)
})

test('Agent view expands every Agent only during inline selection', () => {
  assert.equal(typeof agentsView.apply, 'function')
  if (typeof agentsView.apply !== 'function') return

  const view = harness(snapshot('root-1', 'child-2'))
  agentsView.apply(view.ctx)

  assert.equal(view.nodes()[0].title, '· Agents')
  assert.equal(view.nodes()[0].tone, 'caption', 'the rail label stays visually secondary')
  assert.equal(view.nodes()[1].title, '• main', 'the active transcript remains marked')
  assert.equal(view.nodes()[3].title, '▸ subagent 2', 'inline focus stays in the stable rail order')
  assert.equal(view.nodes()[3].tone, 'brand')
  assert.equal(view.nodes()[3].selected, true, 'the focused Agent gets a strong generic focus state')
  assert.equal(view.nodes()[3].status, 'done', 'selection preserves the Agent lifecycle status')
  assert.equal(view.nodes()[4].title, '←/→ · enter · esc close')
  assert.equal(view.nodes()[4].tone, 'caption')
})

test('Agent view reserves lifecycle colors for status glyphs', () => {
  const running = snapshot('root-1', 'root-1')
  const runningExpanded = agentsView.dockNodes(running)
  assert.equal(runningExpanded[2].status, 'running')
  assert.equal(runningExpanded[2].tone, 'fg')
  assert.equal(runningExpanded[3].status, 'done')
  assert.equal(runningExpanded[3].tone, 'fg')

  const failed = snapshot()
  failed.items[1].status = 'failed'
  failed.items[2].status = 'finished'

  const collapsed = agentsView.dockNodes(failed)
  assert.deepEqual(collapsed[0], {
    id: 'summary', kind: 'generic', title: '· Agents', body: '2/2', status: 'err', tone: 'caption',
  })

  failed.selectedId = failed.items[0].id
  const expanded = agentsView.dockNodes(failed)
  assert.equal(expanded[2].status, 'err')
  assert.equal(expanded[2].tone, 'fg')
  assert.equal(expanded[3].status, 'done')
  assert.equal(expanded[3].tone, 'fg')

  // Issue #80: every subagent task ended without a failure → the panel
  // auto-closes instead of leaving a static summary behind.
  const completed = snapshot()
  completed.items[1].status = 'finished'
  assert.deepEqual(agentsView.dockNodes(completed), [])
})

test('collapsed progress counts only the current batch while selection keeps history', () => {
  const batched = snapshot('root-1')
  batched.items = [
    batched.items[0],
    { ...batched.items[1], status: 'finished', current: false },
    { ...batched.items[2], status: 'running', current: true },
    { id: '__martty_internal__:agent-history', label: 'History (1)', kind: 'history', status: 'idle' },
  ]

  assert.deepEqual(agentsView.dockNodes(batched)[0], {
    id: 'summary', kind: 'generic', title: '· Agents', body: '0/1', status: 'running', tone: 'caption',
  })

  batched.selectedId = 'root-1'
  const expanded = agentsView.dockNodes(batched)
  assert.equal(expanded.some((node) => node.title.includes('subagent 1')), false)
  assert.equal(expanded.some((node) => node.title.includes('subagent 2')), true)
  assert.equal(expanded.at(-2).title, 'History (1)')
  assert.equal(expanded.at(-2).tone, 'caption')
})

test('Agent view stays keyboard-owned while the command toggles the panel', async () => {
  assert.equal(typeof agentsView.apply, 'function')
  if (typeof agentsView.apply !== 'function') return

  const view = harness()
  agentsView.apply(view.ctx)

  view.update(snapshot('root-1', 'child-1'))
  assert.equal(
    view.nodes().every((node) => node.action === undefined),
    true,
    'the Agent rail must not expose mouse command actions',
  )

  // Bare `/agents` toggles the panel (issue #80). An active inline
  // selection stays visible even while off, so ↓ navigation never vanishes
  // mid-flight.
  await view.command()('')
  assert.equal(view.nodes().length, 5, 'active selection keeps the panel while off')

  // Leaving selection applies the off preference.
  view.update(snapshot('root-1'))
  assert.deepEqual(view.nodes(), [], '/agents off hides the panel')
  await view.command()('')
  assert.equal(view.nodes().length, 2, 'bare /agents turns the panel back on')
  await view.command()('child-1')
  assert.deepEqual(view.selected, ['child-1'], 'an id argument still selects the transcript')
})

test('Agent view auto-closes after the batch ends unless forced open', async () => {
  assert.equal(typeof agentsView.apply, 'function')
  if (typeof agentsView.apply !== 'function') return

  const view = harness()
  agentsView.apply(view.ctx)

  const done = snapshot()
  done.items[1].status = 'finished'
  done.items[2].status = 'finished'
  view.update(done)
  assert.deepEqual(view.nodes(), [], 'all tasks ended → the panel auto-closes')

  // `/agents on` forces the summary open while idle.
  await view.command()('on')
  assert.equal(view.nodes().length, 2, '/agents on keeps the summary open')
  assert.equal(view.nodes()[0].status, 'done')

  // A new running batch clears the forced state, so the panel auto-closes
  // again once this batch ends.
  view.update(snapshot())
  assert.equal(view.nodes()[0].status, 'running')
  view.update(done)
  assert.deepEqual(view.nodes(), [], 'auto-close applies again after a new batch')
})

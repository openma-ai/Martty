import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const planView = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/plan-view.js'),
).href).catch(() => ({}))

test('the Plan Client Plugin owns its dock, fallback command, and modal together', async () => {
  assert.deepEqual(
    planView.inject,
    ['acpSessionPlan', 'tuiSlots', 'tuiCommands', 'tuiOverlay'],
  )
  assert.equal(typeof planView.apply, 'function')
  if (typeof planView.apply !== 'function') return

  let listener
  let current = null
  let dockNodes = []
  let command
  let opened
  const ctx = {
    acpSessionPlan: {
      current: () => structuredClone(current),
      subscribe(next) {
        listener = next
        return () => { listener = undefined }
      },
    },
    tuiSlots: {
      inject(name, callback) {
        assert.equal(name, 'conversation.input.dock')
        return callback()
      },
      register(options, nodes) {
        assert.deepEqual(options, { name: 'conversation.input.dock', id: 'plan-view' })
        dockNodes = nodes
        return {
          update(nodes) { dockNodes = nodes },
          dispose() {},
        }
      },
    },
    tuiCommands: {
      register(options, handler) {
        command = { options, handler }
        return () => {}
      },
    },
    tuiOverlay: {
      openView(view) {
        opened = view
        return { close() {} }
      },
    },
  }

  planView.apply(ctx)
  assert.equal(command.options.name, 'plan-view')
  assert.deepEqual(dockNodes, [])

  current = {
    id: 'primary',
    kind: 'items',
    entries: [
      { content: 'Inspect', priority: 'high', status: 'completed' },
      { content: 'Implement', priority: 'medium', status: 'in_progress' },
    ],
  }
  listener({ sessionId: 's-1', plans: [current] })
  assert.deepEqual(dockNodes, [
    {
      id: 'summary', kind: 'generic', title: '· Plan', body: '1/2',
      tone: 'caption', status: 'running',
      action: { kind: 'command', name: 'plan-view', args: '' },
    },
    {
      id: 'focus', kind: 'generic', title: 'Implement', body: '', tone: 'caption',
      action: { kind: 'command', name: 'plan-view', args: '' },
    },
  ])
  assert.deepEqual(dockNodes[0].action, {
    kind: 'command', name: 'plan-view', args: '',
  })

  await command.handler('')
  assert.equal(opened.id, 'plan-view')
  // The counter lives in the window title; the body is a task-list review
  // the transcript pipeline can fully render ([x]/[ ], bold, strikethrough).
  assert.equal(opened.title, 'Plan 1/2')
  assert.equal(opened.nodes.length, 1)
  assert.equal(opened.nodes[0].kind, 'markdown')
  const md = opened.nodes[0].text
  assert.ok(!md.includes('## Plan'), md)
  assert.match(md, /- \[x\] Inspect · priority · high/)
  assert.match(md, /- \[ \] \*\*Implement\*\* · priority · medium/)

  current = null
  listener({ sessionId: 's-1', plans: [] })
  assert.deepEqual(dockNodes, [])
})

import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const queueView = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/queue-view.js'),
).href).catch(() => ({}))

test('Queue view shows every queued prompt in the composer before selection', () => {
  assert.deepEqual(queueView.inject, ['tuiQueue', 'tuiSlots'])
  assert.equal(typeof queueView.apply, 'function')
  if (typeof queueView.apply !== 'function') return

  let listener
  let nodes = []
  let current = { count: 0, items: [], selectedId: null, editingId: null, deleteConfirm: false }
  const ctx = {
    tuiQueue: {
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
      register(options, initial) {
        assert.deepEqual(options, {
          name: 'conversation.input.dock', id: 'queue-view', order: -10,
        })
        nodes = initial
        return { update(next) { nodes = next }, dispose() {} }
      },
    },
  }

  queueView.apply(ctx)
  assert.deepEqual(nodes, [])

  current = {
    count: 12,
    items: [
      { id: 7, ordinal: 1, summary: 'first queued prompt' },
      { id: 9, ordinal: 2, summary: 'second queued prompt' },
    ],
    selectedId: null,
    editingId: null,
    deleteConfirm: false,
  }
  listener(current)
  assert.deepEqual(nodes, [
    {
      id: 'summary',
      kind: 'generic',
      title: 'Queue · 12 · enter send first · ⌥↑ edit',
      body: '',
      status: 'running',
    },
    {
      id: 'items',
      kind: 'group',
      children: [
        { id: 'item-7', kind: 'text', text: '› 1  first queued prompt', tone: 'fg_secondary' },
        { id: 'item-9', kind: 'text', text: '› 2  second queued prompt', tone: 'fg_secondary' },
      ],
    },
  ])

  current.selectedId = 9
  listener(current)
  assert.equal(nodes[0].kind, 'generic')
  assert.equal(nodes[0].title, 'Queue · 12 · ↑/↓ choose · enter edit · esc close')
  assert.deepEqual(nodes[1], {
    id: 'items',
    kind: 'group',
    children: [
      { id: 'item-7', kind: 'text', text: '› 1  first queued prompt', tone: 'fg_secondary' },
      { id: 'item-9', kind: 'text', text: '▸ 2  second queued prompt', tone: 'brand' },
    ],
  })

  current.selectedId = null
  current.editingId = 7
  listener(current)
  assert.equal(nodes[1].children[0].text, '✎ 1  first queued prompt')
  assert.equal(nodes[1].children[0].tone, 'warn')
})

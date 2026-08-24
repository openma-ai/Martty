import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const tuiQueue = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-queue.js'),
).href).catch(() => ({}))

function makeCtx() {
  return {
    effect(setup) {
      return setup()
    },
  }
}

test('the native Queue projection publishes immutable snapshots to Client plugins', () => {
  assert.equal(typeof tuiQueue.installTuiQueue, 'function')
  if (typeof tuiQueue.installTuiQueue !== 'function') return

  const service = tuiQueue.installTuiQueue(makeCtx())
  const seen = []
  const stop = service.subscribe((snapshot) => seen.push(snapshot))

  service.observe({
    protocol: 0,
    count: 12,
    items: [
      { id: 7, ordinal: 1, summary: 'first queued prompt' },
      { id: 9, ordinal: 2, summary: 'second queued prompt' },
    ],
    selectedId: 9,
    editingId: null,
    deleteConfirm: false,
  })

  assert.deepEqual(service.current(), {
    count: 12,
    items: [
      { id: 7, ordinal: 1, summary: 'first queued prompt' },
      { id: 9, ordinal: 2, summary: 'second queued prompt' },
    ],
    selectedId: 9,
    editingId: null,
    deleteConfirm: false,
  })
  assert.equal(seen.length, 1)

  seen[0].items[0].summary = 'mutated outside'
  assert.equal(service.current().items[0].summary, 'first queued prompt')

  stop()
  service.observe({ protocol: 0, items: [] })
  assert.equal(seen.length, 1)
})

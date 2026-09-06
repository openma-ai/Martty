import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const overlayUrl = pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-overlay.js'),
).href
const overlayPlugin = await import(overlayUrl).catch(() => ({}))

test('delete is an opt-in row action and never submits or deletes a protected option', async () => {
  const overlay = overlayPlugin.installTuiOverlay(makeCtx())
  const deleted = []
  overlay.openSelect({ id: 'items', title: 'Items', options: [
    { value: 'saved', label: 'Saved', deletable: true },
    { value: 'current', label: 'Current', deletable: true, disabled: true },
    { value: 'add', label: 'Add' },
  ] }, { onDelete(value) { deleted.push(value) }, onSubmit() { assert.fail('delete is not submit') } })
  assert.equal(overlay.active().options[0].deletable, true)
  for (const value of ['current', 'add']) {
    await overlay.dispatch({ protocol: 0, id: 'items', event: 'delete', value })
    assert.equal(overlay.active().id, 'items')
    assert.deepEqual(deleted, [])
  }
  await overlay.dispatch({ protocol: 0, id: 'items', event: 'delete', value: 'saved' })
  assert.deepEqual(deleted, ['saved'])
  assert.equal(overlay.active(), null)
})

test('select overlays opt into native search without changing ordinary selectors', () => {
  const overlay = overlayPlugin.installTuiOverlay(makeCtx())
  const picker = { id: 'catalog', title: 'Add Harness', value: 'agent',
    options: [{ value: 'agent', label: 'Agent' }], searchable: true }
  overlay.openSelect(picker)
  assert.equal(overlay.active().searchable, true)
})

test('delete validates its opt-in and requires a handler', async () => {
  const overlay = overlayPlugin.installTuiOverlay(makeCtx())
  const spec = { id: 'items', title: 'Items', options: [{ value: 'saved', label: 'Saved', deletable: 'yes' }] }
  assert.throws(() => overlay.openSelect(spec), /deletable must be a boolean/)
  spec.options[0].deletable = true
  overlay.openSelect(spec)
  await overlay.dispatch({ protocol: 0, id: 'items', event: 'delete', value: 'saved' })
  assert.equal(overlay.active().id, 'items')
  await assert.rejects(overlay.dispatch({ protocol: 0, id: 'items', event: 'delete', value: 'missing' }), /not an option/)
})

test('disabled select options remain visible but cannot submit or close the picker', async () => {
  const overlay = overlayPlugin.installTuiOverlay(makeCtx())
  let submits = 0
  overlay.openSelect({ id: 'disabled', title: 'Choices', options: [
    { value: 'current', label: 'Current', disabled: true },
    { value: 'other', label: 'Other' },
  ] }, { onSubmit() { submits++ } })
  assert.equal(overlay.active().options[0].disabled, true)
  await overlay.dispatch({ protocol: 0, id: 'disabled', event: 'submit', value: 'current' })
  assert.equal(submits, 0)
  assert.ok(overlay.active())
})

test('select option groups remain metadata rather than selectable rows', async () => {
  const sent = [], submitted = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(_method, params) { sent.push(params) },
  })
  const options = [
    { value: 'local', label: 'Local agent', group: 'Downloaded' },
    { value: 'remote', label: 'Remote agent', group: 'Not downloaded' },
    { value: 'manual', label: 'Configure manually' },
  ]
  overlay.openSelect({ id: 'catalog', title: 'Harnesses', options }, {
    onSubmit(value) { submitted.push(value) },
  })
  assert.deepEqual(sent.at(-1).overlay.options, options)
  assert.deepEqual(overlay.active().options, options)
  options[0].group = 'mutated'
  assert.equal(overlay.active().options[0].group, 'Downloaded')
  await overlay.dispatch({ protocol: 0, id: 'catalog', event: 'submit', value: 'remote' })
  assert.deepEqual(submitted, ['remote'])
})

test('select groups must be nonempty single-line strings when present', () => {
  for (const group of ['', '   ', 'Downloaded\nOther', 'Downloaded\x1b[2J', 42, null]) {
    const overlay = overlayPlugin.installTuiOverlay(makeCtx())
    assert.throws(() => overlay.openSelect({ id: 'catalog', title: 'Harnesses',
      options: [{ value: 'agent', label: 'Agent', group }],
    }), /options\[0\]\.group/)
  }
})

test('refreshing the same select replaces its snapshot and handlers without closing the panel', async () => {
  const sent = [], submitted = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(_method, params) { sent.push(params) },
  })
  const first = overlay.openSelect({ id: 'catalog', title: 'Searching', options: [
    { value: 'local', label: 'Local', group: 'Downloaded' },
  ] }, { onSubmit(value) { submitted.push(['old', value]) } })
  const options = [
    { value: 'local', label: 'Local', group: 'Downloaded' },
    { value: 'remote', label: 'Remote', group: 'Not downloaded' },
  ]
  const second = overlay.openSelect({ id: 'catalog', title: 'Harnesses', options }, {
    onSubmit(value) { submitted.push(['new', value]) },
  })
  assert.equal(sent.length, 2)
  assert.ok(sent.every(({ overlay }) => overlay?.kind === 'select'), 'refresh has no intermediate close')
  assert.deepEqual(overlay.active().options, options)
  first.close()
  assert.equal(overlay.active().title, 'Harnesses', 'a stale handle cannot close the refreshed panel')
  assert.equal(sent.length, 2)
  await overlay.dispatch({ protocol: 0, id: 'catalog', event: 'submit', value: 'remote' })
  assert.deepEqual(submitted, [['new', 'remote']])
  assert.equal(sent.at(-1).overlay, null)
  second.close()
  assert.equal(sent.length, 3)
})

test('invalid select refresh or a different open id leaves the current select unchanged', () => {
  const overlay = overlayPlugin.installTuiOverlay(makeCtx())
  overlay.openSelect({ id: 'catalog', title: 'Original', options: [{ value: 'a', label: 'Alpha' }] })
  assert.throws(() => overlay.openSelect({ id: 'catalog', title: 'Invalid', options: [] }), /non-empty array/)
  assert.throws(() => overlay.openSelect({ id: 'other', title: 'Other', options: [{ value: 'b', label: 'Beta' }] }), /already open/)
  assert.equal(overlay.active().title, 'Original')
})

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

test('numeric slider previews every step and submits an optional snapped mark', async () => {
  assert.equal(typeof overlayPlugin.installTuiOverlay, 'function')
  if (typeof overlayPlugin.installTuiOverlay !== 'function') return

  const sent = []
  const events = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  overlay.openSlider({
    id: 'liang-effort',
    title: 'Liang reasoning effort',
    min: 0,
    max: 30,
    step: 1,
    value: 15,
    marks: [
      { value: 0, id: 'off', label: 'Off' },
      { value: 15, id: 'high', label: 'High' },
      { value: 30, id: 'max', label: 'Max' },
    ],
    snapToMarks: true,
  }, {
    onChange(value) {
      events.push(['change', value])
    },
    onSubmit(value, mark) {
      events.push(['submit', value, mark?.id])
    },
  })

  assert.equal(sent.at(-1).method, '_dsh/cordis/tui/overlay/update')
  assert.equal(sent.at(-1).params.protocol, 0)
  assert.equal(sent.at(-1).params.overlay.kind, 'slider')
  assert.equal(sent.at(-1).params.overlay.max, 30)

  await overlay.dispatch({
    protocol: 0,
    id: 'liang-effort',
    event: 'change',
    value: 16,
  })
  await overlay.dispatch({
    protocol: 0,
    id: 'liang-effort',
    event: 'submit',
    value: 15,
  })
  assert.deepEqual(events, [
    ['change', 16],
    ['submit', 15, 'high'],
  ])
  assert.deepEqual(sent.at(-1), {
    method: '_dsh/cordis/tui/overlay/update',
    params: { protocol: 0, overlay: null },
  })
})

test('plain sliders need no marks and close cleanly with their owner', () => {
  assert.equal(typeof overlayPlugin.installTuiOverlay, 'function')
  if (typeof overlayPlugin.installTuiOverlay !== 'function') return

  const sent = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const controller = overlay.openSlider({
    id: 'volume',
    title: 'Volume',
    min: 0,
    max: 100,
    step: 5,
    value: 40,
  })
  assert.deepEqual(sent.at(-1).params.overlay.marks, [])
  assert.equal(sent.at(-1).params.overlay.snapToMarks, false)

  controller.close()
  assert.equal(overlay.active(), null)
  assert.deepEqual(sent.at(-1).params.overlay, null)
})

test('a plugin can open a read-only node view and escape closes it', async () => {
  assert.equal(typeof overlayPlugin.installTuiOverlay, 'function')
  if (typeof overlayPlugin.installTuiOverlay !== 'function') return

  const sent = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  overlay.openView({
    id: 'plan-view',
    title: 'Plan',
    nodes: [{ id: 'step-1', kind: 'generic', title: 'Inspect', body: '', status: 'running' }],
  })

  assert.deepEqual(sent.at(-1).params.overlay, {
    kind: 'view',
    id: 'plan-view',
    title: 'Plan',
    nodes: [{ id: 'step-1', kind: 'generic', title: 'Inspect', body: '', status: 'running' }],
  })
  await overlay.dispatch({ protocol: 0, id: 'plan-view', event: 'cancel' })
  assert.equal(overlay.active(), null)
  assert.equal(sent.at(-1).params.overlay, null)
})

test('a plugin can open a native single-select form', async () => {
  assert.equal(typeof overlayPlugin.installTuiOverlay, 'function')
  if (typeof overlayPlugin.installTuiOverlay !== 'function') return

  const sent = []
  const submitted = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  overlay.openSelect({
    id: 'ui-preset',
    title: 'UI preset',
    value: 'default',
    options: [
      { value: 'default', label: 'Martty', description: 'Ocean blue terminal identity' },
      { value: 'deepseek', label: 'DeepSeek', description: 'Classic Harness identity' },
    ],
  }, {
    onSubmit(value) { submitted.push(value) },
  })

  assert.deepEqual(sent.at(-1).params.overlay, {
    kind: 'select',
    id: 'ui-preset',
    title: 'UI preset',
    value: 'default',
    options: [
      { value: 'default', label: 'Martty', description: 'Ocean blue terminal identity' },
      { value: 'deepseek', label: 'DeepSeek', description: 'Classic Harness identity' },
    ],
  })
  await overlay.dispatch({
    protocol: 0,
    id: 'ui-preset',
    event: 'submit',
    value: 'deepseek',
  })
  assert.deepEqual(submitted, ['deepseek'])
  assert.equal(overlay.active(), null)
  assert.equal(sent.at(-1).params.overlay, null)
})

test('opening the same view refreshes its snapshot and handlers idempotently', async () => {
  assert.equal(typeof overlayPlugin.installTuiOverlay, 'function')
  if (typeof overlayPlugin.installTuiOverlay !== 'function') return

  const sent = []
  const overlay = overlayPlugin.installTuiOverlay(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const cancelled = []
  const first = overlay.openView({
    id: 'plan-view',
    title: 'Plan',
    nodes: [{ id: 'step-1', kind: 'generic', title: 'Inspect', body: '' }],
  }, {
    onCancel() { cancelled.push('old') },
  })
  const second = overlay.openView({
    id: 'plan-view',
    title: 'Plan',
    nodes: [{ id: 'step-1', kind: 'generic', title: 'Implement', body: '' }],
  }, {
    onCancel() { cancelled.push('new') },
  })

  assert.equal(sent.length, 2, 'the replacement snapshot must reach the TUI')
  assert.equal(overlay.active().nodes[0].title, 'Implement')
  await overlay.dispatch({ protocol: 0, id: 'plan-view', event: 'cancel' })
  assert.equal(overlay.active(), null)
  assert.deepEqual(cancelled, ['new'])
  assert.equal(sent.length, 3)
  first.close()
  second.close()
  assert.equal(sent.length, 3, 'both handles close the same overlay idempotently')
})

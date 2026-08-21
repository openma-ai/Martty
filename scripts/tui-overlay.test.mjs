import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const overlayUrl = pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-overlay.js'),
).href
const overlayPlugin = await import(overlayUrl).catch(() => ({}))

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

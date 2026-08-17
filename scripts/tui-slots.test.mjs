import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'
import { Context } from '../npm/node_modules/@deepseek-ai/cordis/lib/index.js'

const slotsUrl = pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-slots.js'),
).href
const slotsPlugin = await import(slotsUrl).catch(() => ({}))
const demoPlugin = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/right-demo.js'),
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

test('chrome.right contributions publish a complete live snapshot', () => {
  assert.equal(typeof slotsPlugin.installTuiSlots, 'function')
  if (typeof slotsPlugin.installTuiSlots !== 'function') return

  const sent = []
  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })

  let panel
  const stop = ctx.tuiSlots.inject('chrome.right', () => {
    panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'build', order: 20 },
      [
        { id: 'title', kind: 'markdown', text: '# Build monitor' },
        {
          id: 'checks',
          kind: 'group',
          title: 'Checks',
          children: [{ id: 'test', kind: 'generic', title: 'Tests', body: '97 passed', status: 'ok' }],
        },
      ],
    )
    return panel.dispose
  })

  assert.equal(sent.at(-1).method, '_dsh/cordis/tui/slots/update')
  assert.deepEqual(sent.at(-1).params, {
    protocol: 0,
    slot: 'chrome.right',
    rev: 1,
    nodes: [
      { id: 'build:title', kind: 'markdown', text: '# Build monitor' },
      {
        id: 'build:checks',
        kind: 'group',
        title: 'Checks',
        children: [
          { id: 'build:test', kind: 'generic', title: 'Tests', body: '97 passed', status: 'ok' },
        ],
      },
    ],
  })

  panel.update([{ id: 'done', kind: 'notice', level: 'info', text: 'Ready to ship' }])
  assert.equal(sent.at(-1).params.rev, 2)
  assert.deepEqual(sent.at(-1).params.nodes, [
    { id: 'build:done', kind: 'notice', level: 'info', text: 'Ready to ship' },
  ])

  stop()
  assert.equal(sent.at(-1).params.rev, 3)
  assert.deepEqual(sent.at(-1).params.nodes, [])
})

test('slots reject undeclared seats and invalid node trees before painting', () => {
  assert.equal(typeof slotsPlugin.installTuiSlots, 'function')
  if (typeof slotsPlugin.installTuiSlots !== 'function') return

  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx)
  assert.throws(
    () => ctx.tuiSlots.inject('conversation.chat', () => {}),
    /not declared|chrome\.right/i,
  )
  assert.throws(
    () => ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'broken' },
      [{ id: 'x', kind: 'generic', title: 'Missing body' }],
    ),
    /body/i,
  )
})

test('the gallery is a real sibling plugin whose whole view unloads with its fiber', async () => {
  assert.deepEqual(demoPlugin.inject, ['tuiSlots'])
  assert.equal(typeof demoPlugin.apply, 'function')
  if (typeof demoPlugin.apply !== 'function') return

  const ctx = new Context()
  await ctx.plugin(slotsPlugin)
  const sent = []
  ctx.tuiSlots.bindNotify((method, params) => sent.push({ method, params }))

  const fiber = ctx.plugin(demoPlugin)
  await fiber
  const kinds = sent.at(-1).params.nodes.map((node) => node.kind)
  assert.deepEqual(kinds, ['group', 'notice'])
  assert.ok(sent.at(-1).params.nodes[0].children.some((node) => node.kind === 'terminal'))
  assert.ok(sent.at(-1).params.nodes[0].children.some((node) => node.kind === 'diff'))

  await fiber.dispose()
  assert.deepEqual(sent.at(-1).params.nodes, [])
})

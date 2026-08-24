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

test('generic done status survives the Client slot boundary', () => {
  const sent = []
  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx, {
    notify(method, params) { sent.push({ method, params }) },
  })

  ctx.tuiSlots.register(
    { name: 'conversation.navigation.dock', id: 'agents' },
    [{ id: 'finished', kind: 'generic', title: 'subagent 1', body: '', status: 'done', tone: 'fg' }],
  )

  assert.equal(sent.at(-1).params.nodes[0].status, 'done')
})

test('conversation docks are additive slots with independent live snapshots', () => {
  assert.equal(typeof slotsPlugin.installTuiSlots, 'function')
  if (typeof slotsPlugin.installTuiSlots !== 'function') return

  const sent = []
  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })

  const plan = ctx.tuiSlots.register(
    { name: 'conversation.input.dock', id: 'plan' },
    [{ id: 'summary', kind: 'generic', title: 'Plan · 1/2', body: '', status: 'running' }],
  )
  const goal = ctx.tuiSlots.register(
    { name: 'conversation.input.dock', id: 'goal', order: 10 },
    [{ id: 'summary', kind: 'generic', title: 'Goal · ship it', body: '' }],
  )
  const stats = ctx.tuiSlots.register(
    { name: 'conversation.composer.dock', id: 'stats' },
    [{ id: 'summary', kind: 'generic', title: '1 turn · 2 steps', body: '' }],
  )
  const agents = ctx.tuiSlots.register(
    { name: 'conversation.navigation.dock', id: 'agents' },
    [{
      id: 'active', kind: 'generic', title: '▸ main', body: '', tone: 'brand', selected: true,
      action: { kind: 'command', name: 'agents', args: 'root-1' },
    }],
  )

  const inputSnapshot = sent.findLast((entry) => entry.params.slot === 'conversation.input.dock').params
  assert.deepEqual(inputSnapshot.nodes.map((node) => node.id), ['plan:summary', 'goal:summary'])
  assert.equal(
    ctx.tuiSlots.list().find((slot) => slot.name === 'conversation.input.dock').kind,
    'list',
  )
  assert.equal(sent.at(-1).params.slot, 'conversation.navigation.dock')
  assert.equal(sent.at(-1).params.nodes[0].id, 'agents:active')
  assert.equal(sent.at(-1).params.nodes[0].tone, 'brand')
  assert.equal(sent.at(-1).params.nodes[0].selected, true)
  assert.equal(
    ctx.tuiSlots.list().find((slot) => slot.name === 'conversation.navigation.dock').scope,
    'session',
  )

  plan.dispose()
  goal.dispose()
  stats.dispose()
  agents.dispose()
  assert.deepEqual(sent.at(-1).params.nodes, [])
})

test('generic selected state is boolean before reaching the painter', () => {
  assert.equal(typeof slotsPlugin.installTuiSlots, 'function')
  if (typeof slotsPlugin.installTuiSlots !== 'function') return

  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx)
  assert.throws(
    () => ctx.tuiSlots.register(
      { name: 'conversation.navigation.dock', id: 'broken-selection' },
      [{ id: 'agent', kind: 'generic', title: 'main', body: '', selected: 'yes' }],
    ),
    /selected.*boolean/i,
  )
})

test('welcome hero and info are independent single root seats', () => {
  assert.equal(typeof slotsPlugin.installTuiSlots, 'function')
  if (typeof slotsPlugin.installTuiSlots !== 'function') return

  const sent = []
  const ctx = makeCtx()
  slotsPlugin.installTuiSlots(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })

  const hero = ctx.tuiSlots.register(
    { name: 'welcome.hero', id: 'deepseek-logo' },
    [
      { id: 'logo', kind: 'logo', name: 'deepseek' },
      { id: 'hint', kind: 'text', text: 'Into the Unknown', tone: 'fg_tertiary' },
    ],
  )

  assert.deepEqual(
    ctx.tuiSlots.list().find((slot) => slot.name === 'welcome.hero'),
    {
      name: 'welcome.hero',
      kind: 'single',
      scope: 'root',
      occupants: [{ id: 'deepseek-logo', order: 0 }],
    },
  )
  assert.deepEqual(sent.at(-1).params.nodes, [
    { id: 'deepseek-logo:logo', kind: 'logo', name: 'deepseek' },
    {
      id: 'deepseek-logo:hint',
      kind: 'text',
      text: 'Into the Unknown',
      tone: 'fg_tertiary',
    },
  ])
  const info = ctx.tuiSlots.register(
    { name: 'welcome.info', id: 'session-info' },
    [{ id: 'info', kind: 'welcomeinfo' }],
  )
  assert.deepEqual(
    ctx.tuiSlots.list().find((slot) => slot.name === 'welcome.info'),
    {
      name: 'welcome.info',
      kind: 'single',
      scope: 'root',
      occupants: [{ id: 'session-info', order: 0 }],
    },
  )
  assert.deepEqual(sent.at(-1).params.nodes, [
    { id: 'session-info:info', kind: 'welcomeinfo' },
  ])
  assert.throws(
    () => ctx.tuiSlots.register(
      { name: 'welcome.hero', id: 'second' },
      [{ id: 'other', kind: 'ascii', lines: ['other'] }],
    ),
    /single.*occupied/i,
  )

  hero.dispose()
  info.dispose()
  assert.deepEqual(sent.at(-1).params.nodes, [])
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

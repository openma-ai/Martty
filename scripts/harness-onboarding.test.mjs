import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { apply } from '../npm/lib/harness-view.js'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { installTuiSlots } from '../npm/lib/tui-slots.js'
import { selectedHarness, setDefaultHarness, upsertHarness } from '../npm/lib/harnesses.js'

function setup(t, options = {}, client = { async switchAgent() {} }) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-onboard-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  const ctx = { effect: (fn) => fn(), get() {}, on: () => () => {}, acpClient: client,
    acpSessionStatus: { current: () => ({ session: { started: false } }) } }
  installTuiCommands(ctx)
  installTuiOverlay(ctx)
  const notifications = []
  installTuiSlots(ctx, { notify: (_method, params) => notifications.push(params) })
  const dispose = apply(ctx, { settingsPath, pathValue: '', defaults: [], registry: [], ...options })
  t.after(dispose)
  return { root, settingsPath, ctx, dispose, notifications, command: async (args) => {
    const result = await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args })
    await waitUntil(() => !ctx.tuiOverlay.active()?.title.includes('checking Registry'))
    return result
  },
    submit: (value) => ctx.tuiOverlay.dispatch({ protocol: 0, id: ctx.tuiOverlay.active().id, event: 'submit', value }) }
}

async function selectConfigured(ctx, id) {
  const active = ctx.tuiOverlay.active()
  if (active) await ctx.tuiOverlay.dispatch({ protocol: 0, id: active.id, event: 'cancel' })
  await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
  return ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'submit', value: id })
}

test('Harness picker exposes Add without requiring a command or Registry name', async (t) => {
  const { ctx, command, submit, settingsPath } = setup(t)
  upsertHarness(settingsPath, { id: 'saved', command: 'saved-acp', args: [] })
  await command('')
  const add = ctx.tuiOverlay.active().options.find((option) => /Add Harness/.test(option.label))
  assert.ok(add, 'visible add action')
  await submit(add.value)
  assert.equal(ctx.tuiOverlay.active().id, 'harness-find')
  assert.equal(ctx.tuiOverlay.active().searchable, true)
})

test('Harness removal is discoverable, confirms the target and removes only configuration by default', async t => {
  const { ctx, command, submit, settingsPath } = setup(t)
  upsertHarness(settingsPath, { id: 'saved', label: 'Saved Agent', command: 'saved-acp' })
  setDefaultHarness(settingsPath, 'saved')
  await command('')
  assert.ok(!ctx.tuiOverlay.active().options.some(option => option.value === ':remove'))
  assert.equal(ctx.tuiOverlay.active().options.find(option => option.value === 'saved').deletable, true)
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'delete', value: 'saved' })
  assert.equal(ctx.tuiOverlay.active().value, 'config')
  assert.equal(ctx.tuiOverlay.active().options.find(option => option.value === 'cleanup').disabled, true)
  await submit('config')
  assert.equal(ctx.tuiOverlay.active().kind, 'view', 'show complete paths, not a truncated option description')
  assert.match(JSON.stringify(ctx.tuiOverlay.active()), /Saved Agent|settings.json/)
  assert.equal(selectedHarness(settingsPath).id, 'saved', 'confirmation has not yet removed anything')
  await submit('remove')
  assert.equal(selectedHarness(settingsPath), undefined)
  assert.equal(ctx.tuiOverlay.active().id, 'harness-removed')
})

test('Removal Esc returns one level and preserves selection without deleting anything', async t => {
  const { root, ctx, command, submit, settingsPath } = setup(t)
  const resource = path.join(root, 'bin', 'saved', '1', 'darwin-arm64')
  mkdirSync(resource, { recursive: true })
  const executable = path.join(resource, 'agent')
  writeFileSync(executable, 'fixture')
  upsertHarness(settingsPath, { id: 'first', command: 'first-acp' })
  upsertHarness(settingsPath, { id: 'saved', command: executable })
  const before = readFileSync(settingsPath, 'utf8')
  const cancel = () => ctx.tuiOverlay.dispatch({ protocol: 0, id: ctx.tuiOverlay.active().id, event: 'cancel' })
  await command('')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'delete', value: 'saved' })
  await submit('cleanup')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-remove-confirm')
  await cancel()
  assert.equal(ctx.tuiOverlay.active()?.id, 'harness-remove-mode')
  assert.equal(ctx.tuiOverlay.active().value, 'cleanup')
  assert.match(ctx.tuiOverlay.active().title, /esc back/)
  await cancel()
  assert.equal(ctx.tuiOverlay.active()?.id, 'harness')
  assert.equal(ctx.tuiOverlay.active().value, 'saved')
  await cancel()
  assert.equal(ctx.tuiOverlay.active(), null)
  assert.equal(readFileSync(settingsPath, 'utf8'), before)
  assert.ok(existsSync(executable))
})

test('Typed removal picker is the parent of removal mode', async t => {
  const { ctx, command, submit, settingsPath } = setup(t)
  upsertHarness(settingsPath, { id: 'saved', command: 'saved-acp' })
  await command('remove')
  await submit('saved')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-remove-mode', event: 'cancel' })
  assert.equal(ctx.tuiOverlay.active()?.id, 'harness-remove')
  assert.equal(ctx.tuiOverlay.active().value, 'saved')
})

test('Current removal entry is disabled and cannot be bypassed by a typed command', async t => {
  const { ctx, command, settingsPath } = setup(t, {}, { command: 'live-acp', args: [] })
  upsertHarness(settingsPath, { id: 'live', command: 'live-acp' })
  await command('')
  for (const value of ['live', ':add']) {
    await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'delete', value })
    assert.equal(ctx.tuiOverlay.active().id, 'harness')
  }
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'cancel' })
  await command('remove')
  assert.equal(ctx.tuiOverlay.active().options[0].disabled, true)
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: ctx.tuiOverlay.active().id, event: 'cancel' })
  assert.equal(ctx.tuiOverlay.active().id, 'harness')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'cancel' })
  await command('remove live')
  assert.match(JSON.stringify(ctx.tuiOverlay.active()), /Switch to another Harness/)
  assert.equal(JSON.parse(readFileSync(settingsPath)).harnesses.length, 1)
})

test('Removal waits for download cancellation and prevents late configuration writeback', async t => {
  const app = parallelPackages(t)
  await app.start('alpha'); await app.hide()
  upsertHarness(app.settingsPath, { id: 'alpha', command: 'old-alpha' })
  await app.command('remove alpha'); await app.submit('config')
  const pending = app.submit('remove')
  await new Promise(setImmediate)
  assert.equal(app.preparations[0].signal.aborted, true)
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses.length, 1, 'wait for worker and open handles')
  app.preparations[0].resolve()
  await pending
  assert.deepEqual(JSON.parse(readFileSync(app.settingsPath)).harnesses, [])
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-removed')
})

test('Current Harness follows the live recipe, not the saved default, and cannot be selected again', async (t) => {
  let switches = 0
  const app = setup(t, {}, { command: 'live-acp', args: [], async switchAgent() { switches++ } })
  upsertHarness(app.settingsPath, { id: 'live', command: 'live-acp', args: [] })
  upsertHarness(app.settingsPath, { id: 'other', command: 'other-acp', args: [] })
  setDefaultHarness(app.settingsPath, 'other')
  await app.command('')
  const live = app.ctx.tuiOverlay.active().options.find(({ value }) => value === 'live')
  assert.equal(live.disabled, true)
  assert.match(live.label, /current/i)
  await app.submit('live')
  assert.equal(app.ctx.tuiOverlay.active()?.id, 'harness')
  await app.command('live')
  assert.equal(switches, 0)
  const option = app.ctx.tuiCommands.list()[0].input.options.find(({ value }) => value === 'live')
  assert.equal(option.disabled, true)
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'cancel' })
  await app.command('add')
  assert.equal(app.ctx.tuiOverlay.active().options.find(({ value }) => value === 'live').disabled, true)
})

test('Current Harness is first in the switch picker, completion and catalog without reordering its peers', async (t) => {
  const app = setup(t, {}, { command: 'live-acp', args: [], async switchAgent() {} })
  for (const id of ['first', 'second', 'live', 'last']) {
    upsertHarness(app.settingsPath, { id, command: `${id}-acp`, args: [] })
  }
  const expected = ['live', 'first', 'second', 'last']
  await app.command('')
  assert.deepEqual(app.ctx.tuiOverlay.active().options.slice(0, 4).map(({ value }) => value), expected)
  assert.equal(app.ctx.tuiOverlay.active().options[0].disabled, true)
  assert.deepEqual(app.ctx.tuiCommands.list()[0].input.options.slice(0, 4).map(({ value }) => value), expected)
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'cancel' })
  await app.command('add')
  assert.deepEqual(app.ctx.tuiOverlay.active().options.slice(0, 4).map(({ value }) => value), expected)
})

test('Current completion follows the handoff while the new session is still awaiting readiness', async (t) => {
  let finish
  const client = { command: 'old-acp', args: [], async switchAgent(next) {
    this.command = next.command
    this.args = next.args
    return { ready: new Promise((resolve) => { finish = resolve }) }
  } }
  const app = setup(t, {}, client)
  upsertHarness(app.settingsPath, { id: 'old', command: 'old-acp', args: [] })
  upsertHarness(app.settingsPath, { id: 'next', command: 'next-acp', args: [] })
  const result = await app.command('next')
  assert.equal(result.action, 'harness-switched')
  const options = app.ctx.tuiCommands.list()[0].input.options
  assert.equal(options.find(({ value }) => value === 'next').disabled, true)
  assert.notEqual(options.find(({ value }) => value === 'old').disabled, true)
  assert.equal(selectedHarness(app.settingsPath), undefined)
  finish({ sessionId: 'ready' })
  await new Promise(setImmediate)
  assert.equal(selectedHarness(app.settingsPath).id, 'next')
})

test('An exited Harness is no longer marked current and can be selected for recovery', async (t) => {
  let switches = 0
  const client = { command: 'dead-acp', args: [], child: { exitCode: 1, signalCode: null }, async switchAgent() { switches++ } }
  const app = setup(t, {}, client)
  upsertHarness(app.settingsPath, { id: 'dead', command: 'dead-acp', args: [] })
  await app.command('')
  const item = app.ctx.tuiOverlay.active().options.find(({ value }) => value === 'dead')
  assert.notEqual(item.disabled, true)
  assert.doesNotMatch(item.label, /current/)
  await app.submit('dead')
  assert.equal(switches, 1)
})

test('Empty add opens browsable Registry choices instead of command instructions', async (t) => {
  const { ctx, command } = setup(t)
  await command('add')
  assert.equal(ctx.tuiOverlay.active().kind, 'select')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-find')
  assert.ok(ctx.tuiOverlay.active().options.some((option) => /Manual/.test(option.label)))
})

test('Selecting a catalog item reuses its displayed snapshot without rediscovering the Registry', async (t) => {
  let inspections = 0
  const record = { id: 'binary-agent', label: 'Binary Agent', version: '1',
    get distributions() {
      inspections++
      return [{ type: 'binary', target: 'test-platform', command: 'binary-acp', args: [], env: {}, archive: 'https://example.test/agent.tar.gz' }]
    },
  }
  const { ctx, command, submit } = setup(t, { registry: [record] })
  await command('add')
  inspections = 0
  await submit('binary-agent')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-install-confirm')
  assert.equal(inspections, 0, 'selecting a displayed item cannot re-scan the catalog')
})

test('Add groups configured and local Harnesses above downloads with semantic dividers', async (t) => {
  const { ctx, command, settingsPath } = setup(t, {
    registry: [
      { id: 'not-downloaded', label: 'Not downloaded', version: '1', distributions: [{
        type: 'binary', target: 'test-platform', command: 'missing-acp', args: [], env: {}, archive: 'https://example.test/missing.tar.gz',
      }] },
      { id: 'found-local', label: 'Found local', commands: [{ command: process.execPath }], install: 'unused' },
    ],
  })
  upsertHarness(settingsPath, { id: 'saved', command: 'saved-acp', args: [] })
  await command('add')
  const candidates = ctx.tuiOverlay.active().options.filter(({ value }) => !value.startsWith(':'))
  assert.deepEqual(candidates.map(({ value, group }) => ({ value, group })), [
    { value: 'saved', group: 'Installed / configured' },
    { value: 'found-local', group: 'Installed / configured' },
    { value: 'not-downloaded', group: 'Not downloaded' },
  ])
  assert.equal(ctx.tuiOverlay.active().options.some(({ value }) => value === 'Not downloaded'), false,
    'a divider is metadata, never a selectable option')
})

test('Registry failure preserves local choices and retry loads the recovered catalog', async (t) => {
  let attempts = 0
  const { ctx, command, submit, settingsPath } = setup(t, {
    registry: undefined,
    fetchRegistry: async () => {
      if (++attempts === 1) throw new Error('network offline')
      return [{ id: 'catalog-agent', label: 'Catalog Agent', commands: [{ command: 'catalog-acp' }], install: 'install-catalog' }]
    },
  })
  upsertHarness(settingsPath, { id: 'local', command: 'local-acp', args: [] })
  await command('add')
  assert.ok(ctx.tuiOverlay.active().options.some(({ value }) => value === 'local'))
  const retry = ctx.tuiOverlay.active().options.find(({ label }) => /Retry/.test(label))
  assert.ok(retry)
  await submit(retry.value)
  await waitUntil(() => ctx.tuiOverlay.active().options.some(({ value }) => value === 'catalog-agent'))
  assert.equal(attempts, 2)
  assert.ok(ctx.tuiOverlay.active().options.some(({ value }) => value === 'catalog-agent'))
})

test('Local discovery and Registry fetch run concurrently, and late catalog results cannot replace a selection', async (t) => {
  let resolveRegistry
  let resolveLocal
  const events = []
  const { ctx, command, submit } = setup(t, {
    registry: undefined,
    fetchRegistry: () => { events.push('registry'); return new Promise((resolve) => { resolveRegistry = resolve }) },
    scanCandidates: (_settings, options) => {
      events.push('scan')
      return new Promise((resolve) => { resolveLocal = resolve })
    },
  })
  const pending = command('add')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-find')
  await new Promise(setImmediate)
  assert.deepEqual(new Set(events), new Set(['registry', 'scan']))
  resolveLocal([{ id: 'local', label: 'Local', command: process.execPath, args: [], source: 'path' }])
  await new Promise(setImmediate)
  assert.equal(ctx.tuiOverlay.active().id, 'harness-find', 'local entries appear before the catalog finishes')
  await submit(':manual')
  resolveRegistry([])
  await pending
  assert.equal(ctx.tuiOverlay.active().id, 'harness-manual', 'late results cannot reopen a dismissed catalog')
})

test('Cancelling Registry loading does not reopen the browser after fetch completes', async (t) => {
  let resolveRegistry
  const { ctx, command } = setup(t, { registry: undefined,
    fetchRegistry: () => new Promise((resolve) => { resolveRegistry = resolve }) })
  const pending = command('add')
  assert.equal(ctx.tuiOverlay.active()?.id, 'harness-find')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-find', event: 'cancel' })
  resolveRegistry([])
  await pending
  assert.equal(ctx.tuiOverlay.active(), null)
})

test('Bundled Registry is visible immediately while network and local probes are pending', async (t) => {
  const { ctx, command } = setup(t, { registry: undefined,
    fetchRegistry: () => new Promise(() => {}),
    scanCandidates: () => new Promise(() => {}),
  })
  const start = performance.now()
  const result = await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'add' })
  assert.equal(result, undefined, 'command returns without waiting for network or probes')
  const panel = ctx.tuiOverlay.active()
  assert.equal(panel.kind, 'select', 'first paint must be the usable catalog, not Loading')
  assert.ok(panel.options.some(({ value }) => value === 'antigravity-acp'))
  assert.ok(panel.options.some(({ value }) => value === ':manual'))
  assert.ok(performance.now() - start < 200, 'first paint does not wait for network or workers')
})

test('Reopening and offline refresh retain catalog entries without publishing Loading', async (t) => {
  const published = []
  let fail = false
  const app = setup(t, { registry: undefined, fetchRegistry: async () => {
    if (fail) throw new Error('offline')
    return [{ id: 'cached-agent', label: 'Cached Agent', commands: [{ command: 'cached-acp' }], install: 'install-cached' }]
  } })
  app.ctx.tuiOverlay.bindNotify((_method, params) => published.push(params.overlay))
  await app.command('add')
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-find', event: 'cancel' })
  const reopened = app.command('add')
  assert.ok(app.ctx.tuiOverlay.active().options.some(({ value }) => value === 'cached-agent'))
  await reopened
  fail = true
  await app.submit(':refresh')
  assert.ok(app.ctx.tuiOverlay.active().options.some(({ value }) => value === 'cached-agent'))
  assert.ok(published.filter(Boolean).every(({ kind }) => kind === 'select'))
})

test('Offline Registry still lets the local probe replace unchecked catalog entries', async (t) => {
  let finishScan
  const app = setup(t, { registry: undefined, fetchRegistry: async () => { throw new Error('offline') },
    scanCandidates: () => new Promise((resolve) => { finishScan = resolve }),
  })
  await app.command('add')
  finishScan([{ id: 'antigravity-acp', label: 'Google Antigravity', command: '/installed/agy', args: [], source: 'managed', status: 'Installed' }])
  await new Promise(setImmediate)
  const installed = app.ctx.tuiOverlay.active().options.find(({ value }) => value === 'antigravity-acp')
  assert.equal(installed.group, 'Installed / configured')
})

test('Selecting an unchecked catalog entry probes only that recipe and prefers its local command', async (t) => {
  const switched = []
  const registry = ['alpha', 'beta'].map((id) => ({ id, label: id, version: '1', distributions: [{ type: 'npx', command: 'npx', args: [id] }] }))
  const app = setup(t, { registry, scanCandidates: async (_path, scoped) => {
    if (scoped.registry.length !== 1) return new Promise(() => {})
    return [{ id: 'alpha', label: 'alpha', command: '/installed/alpha', args: [], source: 'path', status: 'Found locally' }]
  } }, { async switchAgent(agent) { switched.push(agent) } })
  await app.ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'add' })
  const result = await app.submit('alpha')
  assert.equal(result, undefined)
  assert.deepEqual(switched, [])
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].command, '/installed/alpha')
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-saved')
})

test('Missing runtime offers recheck without persisting a broken recipe', async (t) => {
  const { ctx, command, submit, settingsPath } = setup(t, { registry: [{
    id: 'python-agent', label: 'Python Agent', version: '1',
    distributions: [{ type: 'uvx', command: 'uvx', args: ['python-agent'], env: {} }],
  }] })
  await command('add')
  await submit('python-agent')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-runner-missing')
  assert.ok(ctx.tuiOverlay.active().options.some(({ label }) => /Recheck/.test(label)))
  assert.ok(ctx.tuiOverlay.active().options.some(({ description }) => description?.includes('docs.astral.sh')))
  assert.equal(existsSync(settingsPath), false)
})

test('Default changes only after the target completes ACP readiness', async (t) => {
  let ready
  const { command, settingsPath } = setup(t, {}, { async switchAgent() {
    return { ready: new Promise((resolve) => { ready = resolve }) }
  } })
  upsertHarness(settingsPath, { id: 'previous', command: 'previous-acp', args: [] })
  upsertHarness(settingsPath, { id: 'next', command: 'next-acp', args: [] })
  setDefaultHarness(settingsPath, 'previous')
  const result = await command('next')
  assert.equal(result.action, 'harness-switched', 'return handoff so painter can initialize')
  assert.equal(selectedHarness(settingsPath).id, 'previous')
  ready()
  await new Promise(setImmediate)
  assert.equal(selectedHarness(settingsPath).id, 'next')
})

test('ACP readiness failure preserves default and provides retry', async (t) => {
  let fail
  const { ctx, command, submit, settingsPath } = setup(t, {}, { async switchAgent() {
    return { ready: new Promise((_, reject) => { fail = reject }) }
  } })
  upsertHarness(settingsPath, { id: 'previous', command: 'previous-acp', args: [] })
  upsertHarness(settingsPath, { id: 'next', command: 'next-acp', args: [] })
  setDefaultHarness(settingsPath, 'previous')
  await command('next')
  fail(new Error('initialize failed'))
  await new Promise(setImmediate)
  assert.equal(selectedHarness(settingsPath).id, 'previous')
  assert.equal(ctx.tuiOverlay.active().kind, 'view')
  assert.equal((await submit()).action, 'harness-switched')
})

test('A session setup failure is not current even if the adapter remains alive', async t => {
  let fail
  const client = { command: 'old', args: [], child: { exitCode: null, signalCode: null }, async switchAgent(next) {
    this.command = next.command; this.args = next.args
    return { ready: new Promise((_, reject) => { fail = reject }) }
  } }
  const app = setup(t, {}, client)
  upsertHarness(app.settingsPath, { id: 'bad', command: 'bad-acp' })
  await app.command('bad'); fail(new Error('session/new failed'))
  await new Promise(setImmediate)
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: app.ctx.tuiOverlay.active().id, event: 'cancel' })
  assert.equal(app.ctx.tuiOverlay.active(), null, 'Esc closes diagnostics, not another menu')
  await app.command('')
  assert.notEqual(app.ctx.tuiOverlay.active().options[0].disabled, true)
  assert.doesNotMatch(app.ctx.tuiOverlay.active().options[0].label, /current/)
})

test('Connection errors directly show diagnostics and Enter retries without a recovery menu', async (t) => {
  let fail
  let attempts = 0
  const { ctx, command, submit, settingsPath } = setup(t, {}, { async switchAgent() {
    attempts++
    return { ready: new Promise((_, reject) => { fail = reject }) }
  } })
  upsertHarness(settingsPath, { id: 'previous', command: 'previous-acp', args: [] })
  upsertHarness(settingsPath, { id: 'pi', label: 'pi ACP', command: 'pi-acp', args: [] })
  setDefaultHarness(settingsPath, 'previous')
  await command('pi')
  const message = 'Internal error: Could not start pi: executable not found (command: pi). Install Pi or ensure pi is on your PATH. Then try again.'
  const diagnostics = 'Error handling request: redundant stack trace'
  fail(Object.assign(new Error(`session/new: ${message}\nAgent stderr:\n${diagnostics}`), {
    method: 'session/new', acpError: { code: -32603, message, data: { code: 'ENOENT' } }, diagnostics,
  }))
  await new Promise(setImmediate)
  const panel = ctx.tuiOverlay.active()
  assert.equal(panel.kind, 'view')
  assert.match(panel.title, /Could not connect pi ACP/)
  const content = panel.nodes.map((node) => node.text ?? node.body ?? '').join('\n')
  assert.equal(content.split(message).length - 1, 1)
  assert.match(content, /session\/new/)
  assert.match(content, /ENOENT/)
  assert.match(content, /Enter retries this operation/)
  assert.match(content, /redundant stack trace/)
  assert.equal(attempts, 1, 'viewing errors must not reconnect')
  assert.equal(selectedHarness(settingsPath).id, 'previous')
  const retried = await submit()
  assert.equal(retried.action, 'harness-switched')
  assert.equal(attempts, 2)
})

test('Cancelling an explicit switch leaves the newly configured recipe saved', async (t) => {
  const switched = []
  const { ctx, command, submit, settingsPath } = setup(t, {}, {
    async switchAgent(agent) { switched.push(agent) },
  })
  ctx.acpSessionStatus.current = () => ({ session: { started: true } })
  const previous = upsertHarness(settingsPath, { id: 'local', label: 'Working ACP', command: 'working-acp', args: ['--stdio'] })
  setDefaultHarness(settingsPath, 'local')
  await command('add local --command replacement-acp')
  await selectConfigured(ctx, 'local')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-confirm')
  await submit('cancel')
  assert.equal(selectedHarness(settingsPath).command, 'replacement-acp')
  assert.deepEqual(switched, [])
})

test('An explicitly connected replacement remains saved if session setup fails', async (t) => {
  let fail
  const { ctx, command, settingsPath } = setup(t, {}, { async switchAgent() {
    return { ready: new Promise((_, reject) => { fail = reject }) }
  } })
  upsertHarness(settingsPath, { id: 'local', label: 'Working ACP', command: 'working-acp', args: [] })
  setDefaultHarness(settingsPath, 'local')
  await command('add local --command replacement-acp')
  const result = await selectConfigured(ctx, 'local')
  assert.equal(result.harness.command, 'replacement-acp')
  assert.equal(selectedHarness(settingsPath).command, 'replacement-acp')
  fail(new Error('initialize failed'))
  await new Promise(setImmediate)
  assert.equal(selectedHarness(settingsPath).command, 'replacement-acp')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-setup-error')
})

test('Connect saves a prepared Harness before authentication or session readiness', async (t) => {
  let ready
  const { ctx, command, settingsPath } = setup(t, {}, { async switchAgent() {
    assert.equal(JSON.parse(readFileSync(settingsPath)).harnesses[0].command, 'new-acp',
      'Connect must persist configuration before launching the authentication flow')
    return { ready: new Promise((resolve) => { ready = resolve }) }
  } })
  await command('add new-agent --command new-acp')
  assert.equal(ready, undefined, 'configuration does not start a connection')
  const result = await selectConfigured(ctx, 'new-agent')
  assert.ok(result, 'Connect should return a handoff after saving its configuration')
  assert.equal(result.harness.command, 'new-acp')
  await command('')
  assert.ok(ctx.tuiOverlay.active().options.some(({ value }) => value === 'new-agent'),
    'the Harness is listed even while authentication is pending')
  assert.equal(selectedHarness(settingsPath), undefined, 'saving a connection does not claim session readiness')
  ready()
  await new Promise(setImmediate)
  assert.equal(selectedHarness(settingsPath).command, 'new-acp')
})

test('Connect saves a same-id replacement without starting session readiness', async (t) => {
  let ready
  const { command, settingsPath } = setup(t, {}, { async switchAgent() {
    return { ready: new Promise((resolve) => { ready = resolve }) }
  } })
  upsertHarness(settingsPath, { id: 'local', command: 'working-acp', args: [] })
  setDefaultHarness(settingsPath, 'local')
  await command('add local --label Replacement --command replacement-acp --arg --stdio')
  assert.deepEqual(selectedHarness(settingsPath), { id: 'local', label: 'Replacement', command: 'replacement-acp', args: ['--stdio'] })
  assert.equal(ready, undefined)
  assert.deepEqual(selectedHarness(settingsPath), { id: 'local', label: 'Replacement', command: 'replacement-acp', args: ['--stdio'] })
})

test('Host-owned transports still save prepared configuration without switching', async (t) => {
  const { ctx, command, settingsPath } = setup(t, { hostOwned: true }, {
    async switchAgent() { assert.fail('Host-owned transport cannot switch') },
  })
  await command('add local --command local-acp')
  assert.equal(JSON.parse(readFileSync(settingsPath)).harnesses[0].command, 'local-acp')
  assert.equal(selectedHarness(settingsPath), undefined)
  assert.equal(ctx.tuiOverlay.active().id, 'harness-saved')
})

test('Cancelling a later switch retains the explicitly configured binary recipe', async (t) => {
  const { ctx, command, submit, settingsPath } = setup(t, {
    registry: [{ id: 'binary-agent', label: 'Binary Agent', version: '1', distributions: [{
      type: 'binary', target: 'test-platform', command: 'binary-acp', args: [], env: {}, archive: 'https://example.test/agent.tar.gz',
    }] }],
    downloadFile: async (_url, destination) => writeFileSync(destination, 'archive bytes'),
    extractArchive(_archive, destination) { writeFileSync(path.join(destination, 'binary-acp'), '#!/bin/sh\n') },
  }, { async switchAgent() { assert.fail('Cancelled replacement cannot switch') } })
  ctx.acpSessionStatus.current = () => ({ session: { started: true } })
  const previous = upsertHarness(settingsPath, { id: 'binary-agent', label: 'Working binary', command: 'working-acp', args: [] })
  setDefaultHarness(settingsPath, 'binary-agent')
  await command('add binary-agent')
  const configured = selectedHarness(settingsPath)
  assert.notEqual(configured.command, previous.command)
  await selectConfigured(ctx, 'binary-agent')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-confirm')
  await submit('cancel')
  assert.deepEqual(selectedHarness(settingsPath), configured)
})

test('Disposing a pending Registry load closes its owned overlay', async (t) => {
  let finish
  const { ctx, command, dispose } = setup(t, { registry: undefined,
    fetchRegistry: () => new Promise((resolve) => { finish = resolve }) })
  const pending = command('add')
  await Promise.resolve()
  dispose()
  assert.equal(ctx.tuiOverlay.active(), null)
  finish([])
  await pending
  assert.equal(ctx.tuiOverlay.active(), null)
})

test('Disposing during process handoff cannot return a stale switch action', async (t) => {
  let finish
  const { command, settingsPath, dispose } = setup(t, {}, { switchAgent: () => new Promise((resolve) => { finish = resolve }) })
  upsertHarness(settingsPath, { id: 'next', command: 'next-acp', args: [] })
  const pending = command('next')
  dispose()
  finish({ ready: Promise.resolve({ sessionId: 'unused' }) })
  assert.equal(await pending, undefined)
  assert.equal(selectedHarness(settingsPath), undefined)
})

test('Disposing closes the Harness picker so its actions cannot run after unload', async (t) => {
  const { ctx, command, settingsPath, dispose } = setup(t)
  upsertHarness(settingsPath, { id: 'next', command: 'next-acp', args: [] })
  await command('')
  dispose()
  assert.equal(ctx.tuiOverlay.active(), null)
})

test('Late connection failure does not overwrite an open picker or reject unhandled', async (t) => {
  let fail
  const { ctx, command, settingsPath } = setup(t, {}, { async switchAgent() {
    return { ready: new Promise((_, reject) => { fail = reject }) }
  } })
  upsertHarness(settingsPath, { id: 'next', command: 'next-acp', args: [] })
  await command('next')
  await command('')
  fail(new Error('connection timed out'))
  await new Promise(setImmediate)
  assert.equal(ctx.tuiOverlay.active().id, 'harness')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'cancel', value: 'next' })
  await command('')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-setup-error')
})

test('A newer successful switch clears a deferred failure from the previous candidate', async (t) => {
  let fail
  const { ctx, command, submit, settingsPath } = setup(t, {}, { async switchAgent(agent) {
    return { ready: agent.command === 'bad-acp'
      ? new Promise((_, reject) => { fail = reject })
      : Promise.resolve({ sessionId: 'good-session' }) }
  } })
  upsertHarness(settingsPath, { id: 'bad', command: 'bad-acp', args: [] })
  upsertHarness(settingsPath, { id: 'good', command: 'good-acp', args: [] })
  await command('bad')
  await command('')
  fail(new Error('previous candidate failed'))
  await new Promise(setImmediate)
  await submit('good')
  await new Promise(setImmediate)
  assert.equal(selectedHarness(settingsPath).id, 'good')
  await command('')
  assert.equal(ctx.tuiOverlay.active().id, 'harness')
  assert.equal(ctx.tuiOverlay.active().value, 'good')
})

test('Closing the binary download panel keeps downloading and notifies without switching', async (t) => {
  let signal
  let finishDownload
  let started
  const downloading = new Promise((resolve) => { started = resolve })
  const switched = []
  const { ctx, command, submit, settingsPath, notifications } = setup(t, {
    registry: [{ id: 'binary-agent', label: 'Binary Agent', version: '1', distributions: [{
      type: 'binary', target: 'test-platform', command: 'binary-acp', args: [], env: {}, archive: 'https://example.test/agent.tar.gz',
    }] }],
    downloadFile: (_url, destination, opts) => new Promise((resolve, reject) => {
      signal = opts.signal
      const onAbort = () => reject(signal.reason)
      signal.addEventListener('abort', onAbort, { once: true })
      finishDownload = () => {
        signal.removeEventListener('abort', onAbort)
        writeFileSync(destination, 'archive bytes')
        resolve()
      }
      started()
    }),
    extractArchive(_archive, destination) { writeFileSync(path.join(destination, 'binary-acp'), '#!/bin/sh\n') },
  }, { async switchAgent(agent) { switched.push(agent) } })
  await command('add')
  await submit('binary-agent')
  const installing = submit('install')
  await downloading
  assert.equal(ctx.tuiOverlay.active().id, 'harness-installing')
  await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  await installing
  assert.equal(signal.aborted, false)
  assert.deepEqual(switched, [])
  assert.equal(existsSync(settingsPath), false)
  assert.equal(ctx.tuiOverlay.active(), null)
  finishDownload()
  await waitUntil(() => JSON.stringify(notifications.at(-1)).includes('Download complete'))
  assert.equal(ctx.tuiOverlay.active(), null, 'completion must not steal focus')
  assert.deepEqual(switched, [])
  await command('')
  const download = ctx.tuiOverlay.active().options.find(({ value }) => value === ':download:binary-agent')
  assert.ok(download, 'the completed download can be reopened from /harness')
  await submit(download.value)
  assert.match(ctx.tuiOverlay.active().title, /Download complete/)
  await submit()
  assert.equal(switched.length, 1)
})

async function waitUntil(check) {
  const deadline = Date.now() + 3000
  while (!check()) {
    assert.ok(Date.now() < deadline, 'state should settle')
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
}

function packageSetup(t) {
  let complete, fail, progress, signal
  let preparations = 0
  const switched = []
  const app = setup(t, {
    pathValue: path.dirname(process.execPath),
    registry: [{ id: 'package-agent', label: 'Package Agent', version: '1', distributions: [{
      type: 'npx', command: 'npx', args: ['test-package-agent@1'], env: {},
    }] }],
    preparePackage: (_entry, opts) => {
      preparations++
      signal = opts.signal
      progress = opts.onProgress
      return new Promise((resolve, reject) => { complete = resolve; fail = reject })
    },
  }, { async switchAgent(agent) { switched.push(agent) } })
  return { ...app, switched, complete: () => complete(), fail: (error) => fail(error),
    progress: (value) => progress(value), signal: () => signal, preparations: () => preparations }
}

test('Package download stays in its panel until ready and releases the command loop', async (t) => {
  const app = packageSetup(t)
  await app.command('add')
  await app.submit('package-agent')
  const pending = app.submit('configure')
  await waitUntil(() => app.ctx.tuiOverlay.active()?.id === 'harness-installing')
  assert.equal(await Promise.race([pending.then(() => 'returned'), new Promise((resolve) => setTimeout(() => resolve('blocked'), 50))]), 'returned')
  assert.deepEqual(app.switched, [])
  await app.submit()
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing', 'Enter cannot dismiss an unfinished download')
  app.progress({ phase: 'download', detail: 'Resolving packages' })
  assert.match(JSON.stringify(app.ctx.tuiOverlay.active()), /Resolving packages/)
  app.complete()
  await waitUntil(() => app.ctx.tuiOverlay.active()?.title.includes('Download complete'))
  assert.match(app.ctx.tuiOverlay.active().title, /Download complete/)
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].id, 'package-agent')
  assert.equal(selectedHarness(app.settingsPath), undefined, 'configuration does not change the default')
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  assert.equal(app.switched.length, 0, 'installation must not start initialize, session/new, or authentication')
  assert.equal(selectedHarness(app.settingsPath), undefined)
  await app.command('')
  assert.equal(app.ctx.tuiOverlay.active().options.some(({ value }) => value === ':download:package-agent'), false,
    'an acknowledged installation should not leave a duplicate completed job')
})

test('Adding a local command only configures it and never switches', async (t) => {
  const app = setup(t, {}, { async switchAgent() { assert.fail('configuration must not switch') } })
  assert.equal(await app.command('add local --command local-acp'), undefined)
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].command, 'local-acp')
  assert.equal(selectedHarness(app.settingsPath), undefined)
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-saved')
})

test('Background package downloads preserve unrelated panels and can be reopened without duplicate work', async (t) => {
  const app = packageSetup(t)
  await app.command('add')
  await app.submit('package-agent')
  await app.submit('configure')
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing')
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  assert.equal(app.signal().aborted, false)
  await app.command('add')
  await app.submit('package-agent')
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing')
  assert.equal(app.preparations(), 1)
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  app.ctx.tuiOverlay.openView({ id: 'unrelated', title: 'Other panel', nodes: [] })
  app.complete()
  await waitUntil(() => JSON.stringify(app.notifications.at(-1)).includes('Download complete'))
  assert.equal(app.ctx.tuiOverlay.active().id, 'unrelated')
  assert.deepEqual(app.switched, [])
})

test('Background download failure is visible and retryable without saving a default', async (t) => {
  const app = packageSetup(t)
  await app.command('add')
  await app.submit('package-agent')
  await app.submit('configure')
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing')
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  app.fail(new Error('network offline'))
  await waitUntil(() => JSON.stringify(app.notifications.at(-1)).includes('Download failed'))
  assert.equal(existsSync(app.settingsPath), false)
  await app.command('')
  await app.submit(':download:package-agent')
  assert.match(JSON.stringify(app.ctx.tuiOverlay.active()), /network offline/)
  await app.submit()
  assert.equal(app.preparations(), 2)
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing')
})

test('Unloading owns cancellation even after the download panel was closed', async (t) => {
  const app = packageSetup(t)
  await app.command('add')
  await app.submit('package-agent')
  await app.submit('configure')
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-installing')
  await app.ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'cancel' })
  app.dispose()
  assert.equal(app.signal().aborted, true)
  app.complete()
  await new Promise(setImmediate)
  assert.equal(app.ctx.tuiOverlay.active(), null)
  assert.deepEqual(app.notifications.at(-1).nodes, [])
  assert.equal(existsSync(app.settingsPath), false)
})

test('An in-flight Esc from the downloading panel remains valid when installation finishes', async (t) => {
  const app = packageSetup(t)
  await app.command('add')
  await app.submit('package-agent')
  await app.submit('configure')
  const event = { protocol: 0, id: app.ctx.tuiOverlay.active().id, event: 'cancel' }
  app.complete()
  await waitUntil(() => JSON.stringify(app.notifications.at(-1)).includes('Download complete'))
  await app.ctx.tuiOverlay.dispatch(event)
  assert.equal(app.ctx.tuiOverlay.active(), null)
  assert.deepEqual(app.switched, [])
})

test('Selecting a saved npx Harness switches directly without repeating Add preparation', async (t) => {
  const app = packageSetup(t)
  upsertHarness(app.settingsPath, { id: 'previous', command: 'working-acp', args: [] })
  upsertHarness(app.settingsPath, { id: 'saved-package', label: 'Saved Package', command: path.join(path.dirname(process.execPath), 'npx'), args: ['test-package-agent@1'] })
  setDefaultHarness(app.settingsPath, 'previous')
  const result = await selectConfigured(app.ctx, 'saved-package')
  assert.equal(result?.action, 'harness-switched')
  assert.equal(app.ctx.tuiOverlay.active(), null)
  assert.equal(app.switched.length, 1)
  assert.equal(selectedHarness(app.settingsPath).id, 'saved-package')
})

function parallelPackages(t, client) {
  const preparations = []
  const app = setup(t, {
    pathValue: path.dirname(process.execPath),
    registry: ['alpha', 'beta'].map((id) => ({ id, label: id, version: '1', distributions: [{
      type: 'npx', command: 'npx', args: [`${id}@1`], env: {},
    }] })),
    preparePackage: (entry, options) => new Promise((resolve, reject) => {
      preparations.push({ entry, signal: options.signal, progress: options.onProgress, resolve, reject })
    }),
  }, client)
  const hide = () => app.ctx.tuiOverlay.dispatch({ protocol: 0, id: app.ctx.tuiOverlay.active().id, event: 'cancel' })
  const start = async (id) => { await app.command('add'); await app.submit(id); await app.submit('configure') }
  return { ...app, preparations, hide, start }
}

test('Concurrent downloads announce the latest completion or failure, not the latest start', async (t) => {
  const app = parallelPackages(t)
  await app.start('alpha'); await app.hide()
  await app.start('beta'); await app.hide()
  app.preparations[0].reject(new Error('alpha failed'))
  await new Promise(setImmediate)
  assert.match(JSON.stringify(app.notifications.at(-1)), /Download failed.*alpha/)
  app.preparations[1].resolve()
  await new Promise(setImmediate)
  assert.match(JSON.stringify(app.notifications.at(-1)), /Download complete.*beta/)
  await app.command('')
  await app.submit(':download:beta'); await app.hide()
  assert.match(JSON.stringify(app.notifications.at(-1)), /Download failed.*alpha/,
    'acknowledging beta must leave the unseen alpha failure available')
})

test('Changing a package recipe keeps the old download running independently', async (t) => {
  const app = parallelPackages(t)
  await app.start('alpha'); await app.hide()
  const old = app.preparations[0]
  await app.command('add alpha --command npx --arg alpha@2')
  assert.equal(app.preparations.length, 2, 'the new recipe needs its own preparation')
  assert.deepEqual(app.preparations[1].entry.args, ['alpha@2'])
  assert.equal(old.signal.aborted, false, 'changing a recipe must not cancel its background download')
  old.resolve()
  await new Promise(setImmediate)
  assert.doesNotMatch(app.ctx.tuiOverlay.active().title, /Download complete/,
    'completion of the old task cannot change the new task panel')
  assert.equal(existsSync(app.settingsPath), false)
  await app.hide(); await app.command('')
  const tasks = app.ctx.tuiOverlay.active().options.filter(({ value }) => value.startsWith(':download:'))
  assert.equal(tasks.length, 2)
  assert.equal(new Set(tasks.map(({ value }) => value)).size, 2)
  assert.ok(tasks.some(({ description }) => description.includes('alpha@1')))
  assert.ok(tasks.some(({ description }) => description.includes('alpha@2')))
})

test('A completed package task cannot substitute its old recipe for a manual replacement', async (t) => {
  const switched = []
  const app = parallelPackages(t, { async switchAgent(entry) { switched.push(entry) } })
  await app.start('alpha')
  app.preparations[0].resolve()
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download complete'))
  await app.hide()
  await app.command('add alpha --command npx --arg alpha@2')
  assert.equal(app.preparations.length, 2)
  app.preparations[1].resolve()
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download complete'))
  await app.submit()
  assert.equal(switched.length, 1)
  assert.deepEqual(switched.at(-1).args, ['alpha@2'])
  assert.deepEqual(selectedHarness(app.settingsPath).args, ['alpha@2'])
  await app.command('alpha')
  assert.deepEqual(app.preparations.at(-1).entry.args, ['alpha@2'],
    'the saved recipe takes precedence over the older completed task')
})

test('A saved local replacement is not intercepted by a same-id background package task', async (t) => {
  const switched = []
  const app = parallelPackages(t, { async switchAgent(entry) { switched.push(entry) } })
  await app.start('alpha'); await app.hide()
  await app.command('add alpha --command local-acp')
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].command, 'local-acp')
  await selectConfigured(app.ctx, 'alpha')
  assert.equal(switched.length, 1)
  assert.equal(switched.at(-1).command, 'local-acp')
  assert.equal(app.ctx.tuiOverlay.active(), null)
  assert.equal(app.preparations[0].signal.aborted, false)
  app.preparations[0].resolve()
  await new Promise(setImmediate)
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].command, 'local-acp',
    'an older background installation cannot overwrite the newer explicit configuration')
})

test('Enter on download completion uses normal switching and commits default only after readiness', async (t) => {
  let ready
  const app = parallelPackages(t, { async switchAgent() {
    return { ready: new Promise((resolve) => { ready = resolve }) }
  } })
  await app.start('alpha')
  app.preparations[0].resolve()
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download complete'))
  assert.equal(ready, undefined, 'completion alone must not start the agent')
  assert.match(app.ctx.tuiOverlay.active().title, /enter switch/)
  app.ctx.acpSessionStatus.current = () => ({ session: { started: true } })
  await app.submit()
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-confirm', 'reuse normal started-session confirmation')
  assert.equal(ready, undefined)
  const result = await app.submit('switch')
  assert.equal(result.action, 'harness-switched')
  assert.equal(app.ctx.tuiOverlay.active(), null)
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].id, 'alpha')
  assert.equal(selectedHarness(app.settingsPath), undefined)
  assert.equal(typeof ready, 'function')
  ready({ sessionId: 'ready-session' })
  await new Promise(setImmediate)
  assert.equal(selectedHarness(app.settingsPath).id, 'alpha')
  await app.command('')
  assert.equal(app.ctx.tuiOverlay.active().options.some(({ value }) => value.startsWith(':download:')), false)
})

test('Retrying one same-id task preserves its sibling and removes only the connected recipe', async (t) => {
  const app = parallelPackages(t)
  await app.start('alpha'); await app.hide()
  await app.command('add alpha --command npx --arg alpha@2')
  app.preparations[1].reject(new Error('replacement failed'))
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download failed'))
  await app.submit()
  assert.equal(app.preparations.length, 3)
  assert.equal(app.preparations[0].signal.aborted, false)
  app.preparations[0].resolve()
  await new Promise(setImmediate)
  assert.doesNotMatch(app.ctx.tuiOverlay.active().title, /Download complete/)
  app.preparations[2].resolve()
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download complete'))
  await app.submit()
  await selectConfigured(app.ctx, 'alpha')
  assert.deepEqual(selectedHarness(app.settingsPath).args, ['alpha@2'])
  await app.command('')
  const tasks = app.ctx.tuiOverlay.active().options.filter(({ value }) => value.startsWith(':download:'))
  assert.equal(tasks.length, 1)
  assert.match(tasks[0].description, /alpha@1/)
})

test('A later ACP failure retains the configured recipe and retries without downloading', async (t) => {
  const handoffs = []
  const app = parallelPackages(t, { async switchAgent() {
    return { ready: new Promise((resolve, reject) => { handoffs.push({ resolve, reject }) }) }
  } })
  await app.start('alpha')
  app.preparations[0].resolve()
  await waitUntil(() => app.ctx.tuiOverlay.active().title.includes('Download complete'))
  await app.submit()
  handoffs[0].reject(new Error('ACP connection failed'))
  await new Promise(setImmediate)
  assert.equal(app.ctx.tuiOverlay.active().id, 'harness-setup-error')
  assert.equal(JSON.parse(readFileSync(app.settingsPath)).harnesses[0].id, 'alpha',
    'a setup error does not undo the saved connection')
  assert.equal(selectedHarness(app.settingsPath), undefined)
  await app.hide(); await app.command('')
  assert.ok(app.ctx.tuiOverlay.active().options.some(({ value }) => value === 'alpha'))
  await app.submit('alpha')
  handoffs[1].resolve({ sessionId: 'recovered' })
  await new Promise(setImmediate)
  assert.equal(selectedHarness(app.settingsPath).id, 'alpha')
  assert.equal(app.preparations.length, 1, 'retrying ACP does not redownload a ready package')
  await app.command('')
  assert.equal(app.ctx.tuiOverlay.active().options.some(({ value }) => value.startsWith(':download:')), false)
})

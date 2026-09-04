import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { selectedHarness, setDefaultHarness, upsertHarness } from '../npm/lib/harnesses.js'

const harnessView = await import('../npm/lib/harness-view.js').catch(() => ({}))

function makeCtx(acpClient, session = {}) {
  const currentSession = {
    sessionId: undefined,
    bound: false,
    started: false,
    ...session,
  }
  const ctx = {
    effect(fn) { return fn() },
    get() {},
    on() { return () => {} },
    acpSessionStatus: {
      current() {
        return { session: currentSession }
      },
    },
    ...(acpClient === undefined ? {} : { acpClient }),
  }
  installTuiCommands(ctx)
  installTuiOverlay(ctx)
  return ctx
}

test('/harness opens a native picker over configured and discovered entries', async () => {
  assert.equal(typeof harnessView.apply, 'function')
  if (typeof harnessView.apply !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local',
      label: 'Local ACP',
      command: 'local-acp',
      args: ['--stdio'],
    })
    setDefaultHarness(settingsPath, 'local')
    const ctx = makeCtx()
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
        source: 'builtin',
      }],
    })

    assert.deepEqual(ctx.tuiCommands.list(), [{
      name: 'harness',
      description: 'Switch or add a Harness; switching starts a new session',
      input: {
        hint: '[id] | find [query] | add <id> --command <cmd>',
        options: [
          { value: 'local', label: 'Local ACP', description: 'configured · local-acp --stdio' },
          { value: 'builtin-dsh', label: 'Bundled DeepSeek Harness', description: 'builtin · /pkg/dsh-acp' },
        ],
      },
    }])

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness',
      title: 'Switch Harness · starts a new session',
      value: 'local',
      options: [
        { value: 'local', label: 'Local ACP', description: 'configured · local-acp --stdio' },
        { value: 'builtin-dsh', label: 'Bundled DeepSeek Harness', description: 'builtin · /pkg/dsh-acp' },
      ],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness add saves a custom ACP command and switches to it immediately', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-add-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, { settingsPath, pathValue: '', defaults: [] })

    const result = await ctx.tuiCommands.dispatch({
      protocol: 0,
      name: 'harness',
      args: 'add local --label "Local ACP" --command local-acp --arg --stdio',
    })

    assert.deepEqual(switched, [{ command: 'local-acp', args: ['--stdio'] }])
    assert.deepEqual(ctx.tuiCommands.list()[0].input.options, [{
      value: 'local',
      label: 'Local ACP',
      description: 'configured · local-acp --stdio',
    }])
    assert.deepEqual(selectedHarness(settingsPath), {
      id: 'local',
      label: 'Local ACP',
      command: 'local-acp',
      args: ['--stdio'],
    })
    assert.deepEqual(result, {
      action: 'harness-switched',
      harness: {
        id: 'local',
        label: 'Local ACP',
        command: 'local-acp',
        args: ['--stdio'],
      },
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness add without arguments opens guided setup in the native overlay', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-add-help-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const ctx = makeCtx()
    harnessView.apply(ctx, { settingsPath, pathValue: '', defaults: [] })
    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'add' })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'view',
      id: 'harness-add-help',
      title: 'Add a Harness',
      nodes: [{
        id: 'instructions',
        kind: 'markdown',
        text: 'Use /harness find to discover installed ACP commands, or /harness add <id> --command <cmd> [--label <label>] [--arg <arg>] to save and switch to a custom ACP Harness.',
      }],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find opens discovered ACP candidates inside the TUI', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-view-'))
  const settingsPath = path.join(root, 'settings.json')
  const bin = path.join(root, 'bin')
  try {
    const { mkdirSync, chmodSync, writeFileSync } = await import('node:fs')
    mkdirSync(bin)
    const acp = path.join(bin, 'local-acp')
    writeFileSync(acp, '#!/bin/sh\n')
    chmodSync(acp, 0o755)
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, { settingsPath, pathValue: bin, defaults: [] })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find' })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Find ACP Harnesses',
      value: 'path-local-acp',
      options: [{
        value: 'path-local-acp',
        label: 'local-acp',
        description: `path · ${acp}`,
      }],
    })
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'path-local-acp',
    })
    assert.deepEqual(switched, [{ command: acp, args: [] }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness marks the forced product Harness until this process switches', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-default-'))
  const settingsPath = path.join(root, 'settings.json')
  const productDefault = {
    id: 'product-default',
    label: 'Product Default',
    command: 'product-acp',
    args: [],
    source: 'builtin',
  }
  try {
    upsertHarness(settingsPath, {
      id: 'last-used', label: 'Last Used', command: 'last-used-acp', args: [],
    })
    setDefaultHarness(settingsPath, 'last-used')
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, {
      settingsPath,
      forcedHarness: productDefault,
      defaults: [productDefault],
      pathValue: '',
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    assert.equal(ctx.tuiOverlay.active()?.value, 'product-default')

    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness',
      event: 'submit',
      value: 'last-used',
    })
    assert.deepEqual(switched, [{ command: 'last-used-acp', args: [] }])

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    assert.equal(ctx.tuiOverlay.active()?.value, 'last-used')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('submitting the TUI harness picker without an active session immediately replaces the agent', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-submit-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: [],
    })
    setDefaultHarness(settingsPath, 'local')
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
        source: 'builtin',
      }],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    const result = await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness',
      event: 'submit',
      value: 'builtin-dsh',
    })

    assert.deepEqual(switched, [{ command: '/pkg/dsh-acp', args: [] }])
    assert.deepEqual(selectedHarness(settingsPath), {
      id: 'builtin-dsh',
      label: 'Bundled DeepSeek Harness',
      command: '/pkg/dsh-acp',
      args: [],
    })
    assert.equal(ctx.tuiOverlay.active(), null)
    assert.deepEqual(result, {
      action: 'harness-switched',
      harness: {
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
      },
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('submitting the TUI harness picker from an unused bound session switches immediately', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-empty-bound-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: [],
    })
    setDefaultHarness(settingsPath, 'local')
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    }, { sessionId: 'session-1', bound: true, started: false })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
        source: 'builtin',
      }],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    const result = await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness',
      event: 'submit',
      value: 'builtin-dsh',
    })

    assert.deepEqual(switched, [{ command: '/pkg/dsh-acp', args: [] }])
    assert.equal(ctx.tuiOverlay.active(), null)
    assert.deepEqual(result, {
      action: 'harness-switched',
      harness: {
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
      },
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('submitting the TUI harness picker after the first prompt requires confirmation', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-confirm-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: [],
    })
    setDefaultHarness(settingsPath, 'local')
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async switchAgent(agent) {
        switched.push(agent)
      },
    }, { sessionId: 'session-1', bound: true, started: true })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
        source: 'builtin',
      }],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    const first = await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness',
      event: 'submit',
      value: 'builtin-dsh',
    })

    assert.equal(first, undefined)
    assert.deepEqual(switched, [])
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-confirm',
      title: 'Switch Harness? · starts a new session',
      value: 'switch',
      options: [
        {
          value: 'switch',
          label: 'Switch to Bundled DeepSeek Harness',
          description: 'Current session stays available in /session',
        },
        {
          value: 'cancel',
          label: 'Stay in current session',
          description: 'Keep using the current Harness',
        },
      ],
    })

    const result = await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-confirm',
      event: 'submit',
      value: 'switch',
    })

    assert.deepEqual(switched, [{ command: '/pkg/dsh-acp', args: [] }])
    assert.deepEqual(result, {
      action: 'harness-switched',
      harness: {
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
      },
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness <id> saves directly while profile mode remains explicitly Host-owned', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-profile-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const ctx = makeCtx()
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      hostOwned: true,
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: [],
        source: 'builtin',
      }],
    })

    await ctx.tuiCommands.dispatch({
      protocol: 0,
      name: 'harness',
      args: 'builtin-dsh',
    })
    assert.equal(selectedHarness(settingsPath)?.id, 'builtin-dsh')
    assert.equal(
      ctx.tuiOverlay.active().nodes[0].text,
      'Bundled DeepSeek Harness is saved for a new standalone session. The current dsh profile and session remain Host-owned.',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

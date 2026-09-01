import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { activateHarness, selectedHarness, upsertHarness } from '../npm/lib/harnesses.js'

const harnessView = await import('../npm/lib/harness-view.js').catch(() => ({}))

function makeCtx(acpClient) {
  const ctx = {
    effect(fn) { return fn() },
    get() {},
    on() { return () => {} },
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
    activateHarness(settingsPath, 'local')
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
      description: 'Switch Harness now and start a new session',
      input: {
        hint: '[id]',
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

test('submitting the TUI harness picker immediately replaces the agent and starts a new session', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-submit-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: [],
    })
    activateHarness(settingsPath, 'local')
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

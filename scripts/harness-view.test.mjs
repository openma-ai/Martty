import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { activateHarness, selectedHarness, upsertHarness } from '../npm/lib/harnesses.js'

const harnessView = await import('../npm/lib/harness-view.js').catch(() => ({}))

function makeCtx() {
  const ctx = {
    effect(fn) { return fn() },
    get() {},
    on() { return () => {} },
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
      description: 'Switch harness in a new session on the next standalone launch',
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
      title: 'Harness · next standalone session',
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

test('submitting the TUI harness picker persists selection and explains the restart boundary', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-submit-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: [],
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

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness',
      event: 'submit',
      value: 'builtin-dsh',
    })

    assert.deepEqual(selectedHarness(settingsPath), {
      id: 'builtin-dsh',
      label: 'Bundled DeepSeek Harness',
      command: '/pkg/dsh-acp',
      args: [],
    })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'view',
      id: 'harness-saved',
      title: 'Harness saved',
      nodes: [{
        id: 'notice',
        kind: 'notice',
        level: 'info',
        text: 'Bundled DeepSeek Harness will start in a new session after restarting standalone Martty.',
      }],
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

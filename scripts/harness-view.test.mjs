import assert from 'node:assert/strict'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { selectedHarness, setDefaultHarness, upsertHarness } from '../npm/lib/harnesses.js'

const harnessView = await import('../npm/lib/harness-view.js').catch(() => ({}))

const addHarnessOption = {
  value: ':add', label: '+ Add Harness…', description: 'Browse the ACP Registry and local programs',
}
const browseActions = [
  { value: ':refresh', label: 'Refresh Registry', description: 'Recheck the catalog and local programs' },
  { value: ':manual', label: 'Manual configuration…', description: 'Use an ACP command not listed here' },
]

async function waitForCatalog(ctx) {
  const deadline = Date.now() + 3000
  while (ctx.tuiOverlay.active()?.title.includes('checking Registry')) {
    assert.ok(Date.now() < deadline, 'background catalog scan should settle')
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
}

async function selectConfigured(ctx, id) {
  const active = ctx.tuiOverlay.active()
  if (active) await ctx.tuiOverlay.dispatch({ protocol: 0, id: active.id, event: 'cancel' })
  await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
  return ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness', event: 'submit', value: id })
}

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

test('/harness selects the default without touching the current session; Enter uses new-session', async t => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-default-session-'))
  t.after(() => rmSync(root, {recursive:true,force:true}))
  const settingsPath = path.join(root, 'settings.json')
  const entry = {id:'beta',label:'Beta',command:'beta-acp',args:[]}
  upsertHarness(settingsPath, entry)
  const defaults = []
  const ctx = makeCtx({
    command:'alpha-acp',args:[],
    setDefaultAgent(spec) { defaults.push(spec) },
    switchAgent() { assert.fail('Changing the default must not replace an ACP process') },
  }, {sessionId:'alpha-session',bound:true,started:true})
  harnessView.apply(ctx, {settingsPath,pathValue:'',defaults:[]})
  await ctx.tuiCommands.dispatch({protocol:0,name:'harness',args:'beta'})
  assert.equal(selectedHarness(settingsPath).id, 'beta')
  assert.deepEqual(defaults, [{command:'beta-acp',args:[]}])
  assert.equal(ctx.acpSessionStatus.current().session.sessionId, 'alpha-session')
  assert.equal(ctx.tuiOverlay.active().id, 'harness-saved')
  const result = await ctx.tuiOverlay.dispatch({protocol:0,id:'harness-saved',event:'submit'})
  assert.deepEqual(result, {action:'new-session'})
})

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
      description: 'Choose the default Harness for new sessions',
      input: {
        hint: '[id] [--new] | add | remove [id] | find [query]',
        options: [
          { value: 'local', label: 'Local ACP (default)', description: 'configured · local-acp --stdio' },
          { value: 'builtin-dsh', label: 'Bundled DeepSeek Harness', description: 'builtin · /pkg/dsh-acp' },
          { ...addHarnessOption, value: 'add' },
        ],
      },
    }])

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: '' })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness',
      title: 'Default Harness · for new sessions',
      value: 'local',
      options: [
        { value: 'local', label: 'Local ACP (default)', description: 'configured · local-acp --stdio', deletable: true },
        { value: 'builtin-dsh', label: 'Bundled DeepSeek Harness', description: 'builtin · /pkg/dsh-acp' },
        addHarnessOption,
      ],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness add saves a custom command; only a later selection switches', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-add-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async setDefaultAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, { settingsPath, pathValue: '', defaults: [] })

    const configured = await ctx.tuiCommands.dispatch({
      protocol: 0,
      name: 'harness',
      args: 'add local --label "Local ACP" --command local-acp --arg --stdio',
    })

    assert.equal(configured, undefined)
    assert.deepEqual(switched, [])
    assert.equal(selectedHarness(settingsPath), undefined)
    const result = await selectConfigured(ctx, 'local')
    assert.deepEqual(switched, [{ command: 'local-acp', args: ['--stdio'] }])
    assert.deepEqual(ctx.tuiCommands.list()[0].input.options, [{
      value: 'local',
      label: 'Local ACP (default)',
      description: 'configured · local-acp --stdio',
    }, { ...addHarnessOption, value: 'add' }])
    assert.deepEqual(selectedHarness(settingsPath), {
      id: 'local',
      label: 'Local ACP',
      command: 'local-acp',
      args: ['--stdio'],
    })
    assert.deepEqual(result, {
      action: 'harness-selected',
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

test('/harness add uses a registry npx fallback when the local command is missing', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-registry-add-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async setDefaultAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        commands: [{ command: 'demo-acp', args: [] }],
        install: { command: 'npx', args: ['demo-harness-acp'] },
      }],
      preparePackage: async () => {},
      defaults: [],
    })

    await ctx.tuiCommands.dispatch({
      protocol: 0,
      name: 'harness',
      args: 'add demo',
    })
    const deadline = Date.now() + 3000
    while (!ctx.tuiOverlay.active()?.title.includes('Download complete')) {
      assert.ok(Date.now() < deadline, 'package preparation should complete')
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
    assert.deepEqual(switched, [])
    const result = await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-installing', event: 'submit' })
    assert.deepEqual(switched, [{ command: 'npx', args: ['demo-harness-acp'] }])
    assert.deepEqual(selectedHarness(settingsPath), {
      id: 'demo',
      label: 'Demo Harness',
      command: 'npx',
      args: ['demo-harness-acp'],
    })
    assert.deepEqual(result, { action: 'new-session' })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness add without arguments opens searchable Registry browsing in the native overlay', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-add-help-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const ctx = makeCtx()
    harnessView.apply(ctx, { settingsPath, pathValue: '', registry: [], defaults: [] })
    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'add' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Add Harness',
      value: ':refresh',
      options: browseActions,
      searchable: true,
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
      async setDefaultAgent(agent) {
        switched.push(agent)
      },
    })
    harnessView.apply(ctx, { settingsPath, pathValue: bin, registry: [], defaults: [] })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Add Harness',
      value: 'path-local-acp',
      options: [{
        value: 'path-local-acp',
        label: 'local-acp',
        description: `found locally · configure · ${acp}`,
        group: 'Installed / configured',
      }, ...browseActions],
      searchable: true,
    })
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'path-local-acp',
    })
    assert.deepEqual(switched, [])
    await selectConfigured(ctx, 'path-local-acp')
    assert.deepEqual(switched, [{ command: acp, args: [] }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find shows an npx install step for a registry Harness that is not installed', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-install-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const ctx = makeCtx()
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        commands: [{ command: 'demo-acp', args: [] }],
        install: { command: 'npx', args: ['demo-harness-acp'] },
      }],
      defaults: [],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find demo' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Add Harness',
      value: 'demo',
      options: [{
        value: 'demo',
        label: 'Demo Harness',
        description: 'not installed · install npx demo-harness-acp',
        group: 'Not downloaded',
      }, ...browseActions],
      searchable: true,
    })

    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'demo',
    })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'view',
      id: 'harness-find-install',
      title: 'Demo Harness is not installed',
      nodes: [{
        id: 'instructions',
        kind: 'markdown',
        text: 'Configure with /harness add demo; this saves the registry fallback (`npx demo-harness-acp`) as the launch command.',
      }],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find explains a missing package runner without saving or switching', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-runner-missing-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async setDefaultAgent(agent) { switched.push(agent) },
    })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'node-agent',
        label: 'Node Agent',
        version: '1.0.0',
        distributions: [{
          type: 'npx', command: 'npx', args: ['node-agent@1.0.0'], env: {},
        }],
      }],
      defaults: [],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active().options, [{
      value: 'node-agent',
      label: 'Node Agent',
      description: 'needs npx · install Node.js/npm',
      group: 'Not downloaded',
    }, ...browseActions])
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'node-agent',
    })

    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-runner-missing',
      title: 'Node Agent needs Node.js/npm',
      value: ':recheck',
      options: [
        { value: ':recheck', label: 'Recheck installation', description: 'Install Node.js/npm: https://nodejs.org/en/download' },
        { value: ':manual', label: 'Use an existing ACP command…', description: 'The program may be installed outside PATH' },
        { value: ':back', label: 'Back to Harnesses', description: 'Choose another distribution' },
      ],
    })
    assert.deepEqual(switched, [])
    assert.equal(selectedHarness(settingsPath), undefined)
    await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-runner-missing', event: 'submit', value: ':recheck' })
    assert.equal(ctx.tuiOverlay.active().id, 'harness-find')
    assert.equal(ctx.tuiOverlay.active().options[0].description, 'needs npx · install Node.js/npm')
    assert.deepEqual(switched, [])
    assert.equal(selectedHarness(settingsPath), undefined)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find gives legacy binary Harnesses installation guidance and a recheck action', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-binary-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const ctx = makeCtx()
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'brew-demo',
        label: 'Brew Demo Harness',
        commands: [{ command: 'brew-demo-acp', args: [] }],
        install: { command: 'brew', args: ['install', 'brew-demo-acp'] },
      }],
      defaults: [],
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find brew-demo' })
    await waitForCatalog(ctx)
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'brew-demo',
    })
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'view',
      id: 'harness-find-install',
      title: 'Brew Demo Harness is not installed',
      nodes: [{
        id: 'instructions',
        kind: 'markdown',
        text: 'Install: `brew install brew-demo-acp`\nThen return here and press Enter to check again.',
      }],
    })
    await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-find-install', event: 'submit' })
    assert.equal(ctx.tuiOverlay.active().id, 'harness-find')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find confirms managed binary installation before switching', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-install-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async setDefaultAgent(agent) { switched.push(agent) },
    })
    harnessView.apply(ctx, {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'amp-acp',
        label: 'Amp',
        version: '0.9.0',
        description: 'ACP wrapper',
        distributions: [{
          type: 'binary',
          target: 'darwin-aarch64',
          command: './amp-acp',
          args: ['serve'],
          env: {},
          archive: 'https://example.test/amp-acp.tar.gz',
        }],
      }],
      defaults: [],
      extractArchive(_archivePath, destination) {
        const executable = path.join(destination, 'amp-acp')
        writeFileSync(executable, '#!/bin/sh\n')
        chmodSync(executable, 0o755)
      },
      downloadFile: async (_url, destination) => writeFileSync(destination, ''),
    })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find amp' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Add Harness',
      value: 'amp-acp',
      options: [{
        value: 'amp-acp',
        label: 'Amp',
        description: `install in Martty · ${path.join(root, 'bin', 'amp-acp', '0.9.0', 'darwin-aarch64')}`,
        group: 'Not downloaded',
      }, ...browseActions],
      searchable: true,
    })
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-find',
      event: 'submit',
      value: 'amp-acp',
    })
    assert.equal(ctx.tuiOverlay.active().kind, 'select')
    assert.equal(ctx.tuiOverlay.active().id, 'harness-install-confirm')
    assert.deepEqual(switched, [])
    assert.equal(selectedHarness(settingsPath), undefined)
    await ctx.tuiOverlay.dispatch({
      protocol: 0,
      id: 'harness-install-confirm',
      event: 'submit',
      value: 'install',
    })
    assert.deepEqual(switched, [], 'download alone cannot switch the current session')
    const deadline = Date.now() + 3000
    while (!ctx.tuiOverlay.active()?.title.includes('Download complete')) {
      assert.ok(Date.now() < deadline, 'download should complete')
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
    await ctx.tuiOverlay.dispatch({ protocol: 0, id: ctx.tuiOverlay.active().id, event: 'submit' })
    assert.deepEqual(switched, [{
      command: path.join(root, 'bin', 'amp-acp', '0.9.0', 'darwin-aarch64', 'amp-acp'),
      args: ['serve'],
      env: {},
    }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('/harness find switches a configured candidate without reconfiguring an empty session', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-configured-view-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    upsertHarness(settingsPath, {
      id: 'local', label: 'Local ACP', command: 'local-acp', args: ['--stdio'],
    })
    const switched = []
    const ctx = makeCtx({
      kind: 'spawn',
      async setDefaultAgent(agent) { switched.push(agent) },
    }, { bound: true, sessionId: 'empty-session', started: false })
    harnessView.apply(ctx, { settingsPath, pathValue: '', registry: [], defaults: [] })

    await ctx.tuiCommands.dispatch({ protocol: 0, name: 'harness', args: 'find' })
    await waitForCatalog(ctx)
    assert.deepEqual(ctx.tuiOverlay.active(), {
      kind: 'select',
      id: 'harness-find',
      title: 'Add Harness',
      value: 'local',
      options: [{
        value: 'local',
        label: 'Local ACP',
        description: 'configured · local-acp --stdio',
        group: 'Installed / configured',
      }, ...browseActions],
      searchable: true,
    })
    const result = await ctx.tuiOverlay.dispatch({ protocol: 0, id: 'harness-find', event: 'submit', value: 'local' })
    assert.equal(result?.action, 'harness-selected')
    assert.deepEqual(switched, [{ command: 'local-acp', args: ['--stdio'] }])
    assert.equal(selectedHarness(settingsPath)?.id, 'local')
    assert.equal(ctx.tuiOverlay.active().id, 'harness-saved')
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
      'Bundled DeepSeek Harness is saved for the next standalone session. The current profile owns its Harness.',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

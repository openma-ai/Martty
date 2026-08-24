import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'
import { state, fakeChild } from './plugin-runner-harness.mjs'

const repoRoot = path.resolve(import.meta.dirname, '..')
const packageJsonPath = path.join(repoRoot, 'npm', 'package.json')
const runnerPath = path.join(repoRoot, 'npm', 'lib', 'index.js')
const bootPath = path.join(repoRoot, 'npm', 'lib', 'boot.js')
const harnessUrl = pathToFileURL(path.join(repoRoot, 'scripts', 'plugin-runner-harness.mjs')).href

const stubSources = {
  'node:child_process': `export { spawnTui as spawn } from ${JSON.stringify(harnessUrl)}`,
}

registerHooks({
  resolve(specifier, context, nextResolve) {
    const fromPluginLib = context.parentURL?.includes('/npm/lib/index.js')
      || context.parentURL?.includes('/npm/lib/spawn-tui.js')
    if (fromPluginLib && specifier in stubSources) {
      return {
        url: `data:text/javascript,${encodeURIComponent(stubSources[specifier])}`,
        shortCircuit: true,
      }
    }
    return nextResolve(specifier, context)
  },
})

const runner = await import(pathToFileURL(runnerPath).href)
const {
  bootClient,
  migrateLegacyUiSettings,
  parseClientPluginsEnv,
  uiSettingsPath,
} = await import(pathToFileURL(bootPath).href)
const { installTuiTheme } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-theme.js')).href
)
const { installTuiSlots } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-slots.js')).href
)
const { installTuiCommands } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-commands.js')).href
)
const { installTuiOverlay } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-overlay.js')).href
)
const { installTuiQueue } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-queue.js')).href
)
const { installTuiAgents } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-agents.js')).href
)
const { installAcpSessionConfig } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'acp-session-config.js')).href
)
const { installAcpClientEvents } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'acp-client-events.js')).href
)
const { installAcpSessionPlan } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'acp-session-plan.js')).href
)
const { createTuiPluginStore } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'tui-plugin-store.js')).href
)
const { selectNativeBinary } = await import(
  pathToFileURL(path.join(repoRoot, 'npm', 'lib', 'spawn-tui.js')).href
)

test('a source checkout prefers its current debug painter over a stale packaged binary', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-native-select-'))
  try {
    const debug = path.join(root, 'target', 'debug', 'martty')
    const packaged = path.join(root, 'npm', 'vendor', 'darwin-arm64', 'martty')
    mkdirSync(path.dirname(debug), { recursive: true })
    mkdirSync(path.dirname(packaged), { recursive: true })
    writeFileSync(debug, 'current-debug')
    writeFileSync(packaged, 'stale-packaged')

    assert.equal(selectNativeBinary({ devBin: debug, packagedBin: packaged }), debug)
    assert.equal(
      selectNativeBinary({ envBin: '/missing/override', devBin: debug, packagedBin: packaged }),
      debug,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Martty settings live under the branded home with explicit environment precedence', () => {
  assert.equal(
    uiSettingsPath([], { MARTTY_HOME: '/opt/martty', DSH_HOME: '/opt/dsh' }, '/Users/test'),
    path.join('/opt/martty', 'settings.json'),
  )
  assert.equal(
    uiSettingsPath([], { DSH_HOME: '/opt/dsh' }, '/Users/test'),
    path.join('/opt/dsh', '.martty', 'settings.json'),
  )
  assert.equal(
    uiSettingsPath([], {}, '/Users/test'),
    path.join('/Users/test', '.martty', 'settings.json'),
  )
  assert.equal(
    uiSettingsPath(['--session-root', '/tmp/isolated'], {}, '/Users/test'),
    path.join('/tmp/isolated', 'settings.json'),
  )
})

test('legacy UI settings copy into Martty home without overwriting either side', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-settings-migration-'))
  try {
    const legacy = path.join(root, 'legacy', 'dsh-tui-settings.json')
    const current = path.join(root, '.martty', 'settings.json')
    mkdirSync(path.dirname(legacy), { recursive: true })
    writeFileSync(legacy, '{"uiPreset":"deepseek","theme":"ember"}\n')

    assert.equal(migrateLegacyUiSettings(current, [legacy]), true)
    assert.deepEqual(JSON.parse(readFileSync(current, 'utf8')), {
      uiPreset: 'deepseek',
      theme: 'ember',
    })
    assert.equal(existsSync(legacy), true, 'migration must preserve legacy settings')

    writeFileSync(current, '{"theme":"custom"}\n')
    assert.equal(migrateLegacyUiSettings(current, [legacy]), false)
    assert.equal(JSON.parse(readFileSync(current, 'utf8')).theme, 'custom')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('the published plugin is ESM so Cordis can import() it in parallel', () => {
  const pkg = JSON.parse(readFileSync(packageJsonPath, 'utf8'))
  assert.equal(pkg.type, 'module')
  const source = readFileSync(runnerPath, 'utf8')
  assert.match(source, /^import /m)
  assert.match(source, /export function apply/)
  assert.equal(runner.inject.includes('acpClient'), true)
  assert.doesNotMatch(source, /module\.exports/)
  assert.doesNotMatch(source, /from '@deepseek-ai\/dsh-/)
  assert.doesNotMatch(source, /from '@openma\/deepseek-harness-acp/)
})

test('the bundle carries ACP as a dependency and its Creator overlay internally', () => {
  const pkg = JSON.parse(readFileSync(packageJsonPath, 'utf8'))
  assert.deepEqual(pkg.dsh, { bundle: { patch: './cordis.patch.yml' } })
  for (const field of ['dependencies', 'peerDependencies', 'optionalDependencies']) {
    const names = Object.keys(pkg[field] ?? {})
    const forbidden = names.filter(
      (name) => name.startsWith('@deepseek-ai/dsh-'),
    )
    assert.deepEqual(forbidden, [], field)
    const otherDeepseek = names.filter(
      (name) => name.startsWith('@deepseek-ai/') && name !== '@deepseek-ai/cordis',
    )
    assert.deepEqual(otherDeepseek, [], field)
  }
  assert.equal(pkg.dependencies['@deepseek-ai/cordis'] !== undefined, true)
  assert.equal(pkg.dependencies['@openma/deepseek-harness-acp'] !== undefined, true)
  assert.equal(pkg.dependencies['@openma/deepseek-harness-tui-creator'], undefined)
  assert.equal(pkg.exports['./creator-overlay'], './lib/creator-overlay.js')
  assert.equal(pkg.exports['./agents'], './lib/tui-agents.js')
  assert.equal(pkg.exports['./agents-view'], './lib/agents-view.js')
  assert.equal(pkg.files.includes('creator'), true)
  assert.equal(pkg.files.includes('skills/tui-plugin-development/SKILL.md'), true)
})

test('standalone boot mounts the Cordis Client runner before the shell', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    const ctx = await bootClient({
      stream: { stdin: fakeStream(), stdout: fakeStream() },
      extraArgs: [],
    })
    const clientRunner = ctx.get('tuiCordisClientRunner')
    assert.equal(typeof clientRunner?.bindTransport, 'function')
    assert.equal(typeof clientRunner?.onHost, 'function')
    assert.equal(typeof ctx.get('tuiSlots')?.register, 'function')
    assert.equal(typeof ctx.get('tuiCommands')?.register, 'function')
    assert.equal(typeof ctx.get('tuiOverlay')?.openSlider, 'function')
    assert.equal(typeof ctx.get('tuiOverlay')?.openView, 'function')
    assert.equal(typeof ctx.get('acpSessionPlan')?.current, 'function')
    assert.equal(typeof ctx.get('acpSessionStats')?.current, 'function')
    assert.equal(typeof ctx.get('tuiQueue')?.current, 'function')
    assert.equal(typeof ctx.get('tuiAgents')?.current, 'function')
    assert.equal(typeof ctx.get('tuiAgents')?.select, 'function')
    assert.equal(
      ctx.get('tuiCommands')?.list().some((command) => command.name === 'plan-view'),
      true,
    )
    assert.equal(
      ctx.get('tuiCommands')?.list().some((command) => command.name === 'ui'),
      true,
      'the classic DeepSeek logo is a default Client Plugin command',
    )
    assert.deepEqual(
      ctx.get('tuiSlots')?.list().find((slot) => slot.name === 'conversation.input.dock')?.occupants,
      [{ id: 'plan-view', order: 0 }, { id: 'queue-view', order: -10 }],
    )
    assert.deepEqual(
      ctx.get('tuiSlots')?.list().find((slot) => slot.name === 'conversation.composer.dock')?.occupants,
      [{ id: 'stats', order: 0 }],
    )
    assert.deepEqual(
      ctx.get('tuiSlots')?.list().find((slot) => slot.name === 'conversation.navigation.dock')?.occupants,
      [{ id: 'agents-view', order: 0 }],
    )
    assert.equal(
      ctx.get('tuiCommands')?.list().some((command) => command.name === 'agents'),
      true,
    )
  } finally {
    restore()
  }
})

test('Client boot rehydrates Creator-authored UI Plugins from the durable user root', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const home = mkdtempSync(path.join(tmpdir(), 'dsh-tui-client-artifacts-'))
  let ctx
  try {
    const artifactRoot = path.join(home, '.tui-plugins')
    createTuiPluginStore({ root: artifactRoot }).save({
      id: 'focused-ui',
      kind: 'ui-preset',
      name: 'Focused UI',
      purpose: 'Focused startup layout',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiPresets'],
        apply(ctx) {
          return ctx.tuiPresets.register({ id: 'focused', label: 'Focused' }, () => () => {})
        },
      }`,
    })

    ctx = await bootClient({
      stream: { stdin: fakeStream(), stdout: fakeStream() },
      extraArgs: [],
      settingsPath: path.join(home, 'dsh-tui-settings.json'),
      artifactRoot,
    })

    assert.equal(typeof ctx.get('tuiLocalPlugins')?.list, 'function')
    assert.ok(ctx.get('tuiPresets').list().some(({ id }) => id === 'focused'))
    assert.deepEqual(ctx.get('tuiLocalPlugins').list(), [{
      artifactId: 'focused-ui',
      pluginId: 'tui-local:focused-ui',
      kind: 'ui',
      loaded: true,
    }])
  } finally {
    await ctx?.dispose?.()
    runner.resetShellForTests()
    restore()
    rmSync(home, { recursive: true, force: true })
  }
})

test('Client boot mounts installed package entries received from the Host registry', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const home = mkdtempSync(path.join(tmpdir(), 'dsh-tui-package-entries-'))
  let ctx
  try {
    const entry = path.join(home, 'installed-ui.mjs')
    writeFileSync(entry, `
      export const inject = ['tuiPresets']
      export function apply(ctx) {
        return ctx.tuiPresets.register(
          { id: 'installed', label: 'Installed package' },
          () => () => {},
        )
      }
    `)
    const packagePlugins = [{
      id: 'installed-ui',
      kind: 'ui',
      entry: pathToFileURL(entry).href,
    }]
    assert.equal(typeof parseClientPluginsEnv, 'function')
    if (typeof parseClientPluginsEnv !== 'function') return
    assert.deepEqual(parseClientPluginsEnv(JSON.stringify(packagePlugins)), packagePlugins)

    ctx = await bootClient({
      stream: { stdin: fakeStream(), stdout: fakeStream() },
      extraArgs: [],
      settingsPath: path.join(home, 'dsh-tui-settings.json'),
      artifactRoot: path.join(home, '.tui-plugins'),
      packagePlugins,
    })

    assert.ok(ctx.get('tuiPresets').list().some(({ id }) => id === 'installed'))
    assert.deepEqual(ctx.get('tuiLocalPlugins').list(), [{
      artifactId: 'installed-ui',
      pluginId: 'tui-package:installed-ui',
      kind: 'ui',
      loaded: true,
    }])

    assert.equal(parseClientPluginsEnv(JSON.stringify([{
      id: 'legacy-ui',
      kind: 'ui-preset',
      entry: pathToFileURL(entry).href,
    }]))[0].kind, 'ui')
  } finally {
    await ctx?.dispose?.()
    runner.resetShellForTests()
    restore()
    rmSync(home, { recursive: true, force: true })
  }
})

test('Client-process boot keeps ACP on stdio and maps inherited TTY fds to the painter', async () => {
  runner.resetShellForTests()
  state.spawnCalls.length = 0
  const restore = ensureTestNative()
  try {
    await bootClient({
      stream: { stdin: fakeStream(), stdout: fakeStream() },
      extraArgs: [],
      tty: { stdin: 3, stdout: 4 },
    })
    assert.deepEqual(state.spawnCalls.at(-1).options.stdio, [
      3, 4, 'inherit', 'pipe', 'pipe',
    ])
  } finally {
    runner.resetShellForTests()
    restore()
  }
})

test('extra-fd EPIPE after the TUI exits is not an unhandled error', {
  skip: process.platform === 'win32' ? 'Windows uses the TCP plugin transport' : false,
}, async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    await runner.apply(makeCtx())
    assert.ok(fakeChild.stdio[3].listeners.error.length > 0)
    assert.ok(fakeChild.stdio[4].listeners.error.length > 0)
    const epike = Object.assign(new Error('write EPIPE'), { code: 'EPIPE', syscall: 'write' })
    for (const fn of fakeChild.stdio[3].listeners.error) fn(epike)
    for (const fn of fakeChild.stdio[4].listeners.error) fn(epike)
  } finally {
    restore()
  }
})

test('Windows plugin transport uses authenticated loopback TCP instead of extra fds', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const previous = process.env.DSH_TUI_FORCE_TCP
  process.env.DSH_TUI_FORCE_TCP = '1'
  state.spawnCalls.length = 0
  try {
    await runner.apply(makeCtx())
    const call = state.spawnCalls.at(-1)
    assert.equal(call.args[0], '--attach-tcp')
    assert.match(call.args[1], /^127\.0\.0\.1:\d+$/)
    assert.deepEqual(call.options.stdio, ['inherit', 'inherit', 'inherit'])
    assert.match(call.options.env.DSH_TUI_ATTACH_TOKEN, /^[a-f0-9]{64}$/)
  } finally {
    if (previous === undefined) delete process.env.DSH_TUI_FORCE_TCP
    else process.env.DSH_TUI_FORCE_TCP = previous
    restore()
  }
})

test('Windows plugin transport reports a child launch failure before handshake timeout', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const previous = process.env.DSH_TUI_FORCE_TCP
  process.env.DSH_TUI_FORCE_TCP = '1'
  state.failSpawn = true
  try {
    await assert.rejects(runner.apply(makeCtx()), /simulated launch failure/)
  } finally {
    state.failSpawn = false
    if (previous === undefined) delete process.env.DSH_TUI_FORCE_TCP
    else process.env.DSH_TUI_FORCE_TCP = previous
    restore()
  }
})

function fakeStream() {
  const listeners = { error: [] }
  return {
    listeners,
    on(event, fn) {
      ;(listeners[event] ??= []).push(fn)
      return this
    },
    write() {
      return true
    },
  }
}

function makeCtx({ cordisClientRunner } = {}) {
  const defaultClientRunner = {
    bindTransport() {},
    onHost() {},
    sync() {},
    selectTheme() {},
  }
  const ctx = {
    acpClient: {
      stdin: fakeStream(),
      stdout: fakeStream(),
    },
    get(name) {
      if (name === 'acpClient') return this.acpClient
      if (name === 'tuiCordisClientRunner') return this.tuiCordisClientRunner
      if (name === 'tuiSlots') return this.tuiSlots
      if (name === 'tuiCommands') return this.tuiCommands
      if (name === 'tuiOverlay') return this.tuiOverlay
      if (name === 'tuiQueue') return this.tuiQueue
      if (name === 'tuiAgents') return this.tuiAgents
      if (name === 'acpSessionConfig') return this.acpSessionConfig
      if (name === 'acpSessionPlan') return this.acpSessionPlan
      if (name === 'acpClientEvents') return this.acpClientEvents
      return undefined
    },
    on() {
      return () => {}
    },
    effect(fn) {
      return fn()
    },
  }
  ctx.tuiCordisClientRunner = cordisClientRunner ?? defaultClientRunner
  installTuiTheme(ctx)
  installTuiSlots(ctx)
  installTuiCommands(ctx)
  installTuiOverlay(ctx)
  installTuiQueue(ctx)
  installTuiAgents(ctx)
  installAcpClientEvents(ctx)
  installAcpSessionConfig(ctx)
  installAcpSessionPlan(ctx)
  return ctx
}

function ensureTestNative() {
  const key = `${process.platform}-${process.arch}`
  const exe = process.platform === 'win32' ? 'martty.exe' : 'martty'
  const dir = path.join(repoRoot, 'npm', 'vendor', key)
  const bin = path.join(dir, exe)
  if (existsSync(bin)) return () => {}
  mkdirSync(dir, { recursive: true })
  writeFileSync(bin, '')
  return () => rmSync(bin, { force: true })
}

test('shell consumes tuiTheme and register notifies _dsh/cordis/tui/theme/update toward the painter', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    const ctx = makeCtx()
    await runner.apply(ctx)
    assert.equal(typeof ctx.tuiTheme?.register, 'function')

    const writes = []
    fakeChild.stdio[3].write = (chunk) => {
      writes.push(String(chunk))
      return true
    }
    const palette = JSON.parse(readFileSync(path.join(repoRoot, 'docs/fixtures/demo-skin.v0.json'), 'utf8'))
    ctx.tuiTheme.register(structuredClone(palette))
    const note = writes.map((line) => JSON.parse(line)).find((entry) => entry.method === '_dsh/cordis/tui/theme/update')
    assert.ok(note, 'register emits _dsh/cordis/tui/theme/update')
    assert.equal(note.params.protocol, 0)
    assert.equal(note.params.activate, false)
    assert.equal(note.params.palette.id, 'ember')
  } finally {
    restore()
  }
})

test('shell republishes Client commands after the agent enables the Cordis plane', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    const ctx = makeCtx()
    ctx.tuiCommands.register(
      { name: 'ui', description: 'Switch UI preset' },
      () => {},
    )
    await runner.apply(ctx)

    const writes = []
    fakeChild.stdio[3].write = (chunk) => {
      writes.push(String(chunk))
      return true
    }
    for (const listener of fakeChild.stdio[4].listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify({
        jsonrpc: '2.0', id: 1, method: 'initialize', params: {},
      })}\n`))
    }
    for (const listener of ctx.acpClient.stdout.listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        result: {
          agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
        },
      })}\n`))
    }

    const updates = writes
      .map((line) => JSON.parse(line))
      .filter((entry) => entry.method === '_dsh/cordis/tui/commands/update')
    assert.deepEqual(updates.at(-1)?.params.commands, [{
      name: 'ui',
      description: 'Switch UI preset',
    }])
  } finally {
    restore()
  }
})

test('shell routes /theme selection through the dynamic Client Plugin runner', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    const seen = []
    const ctx = makeCtx({
      cordisClientRunner: {
        bindTransport() {},
        onHost() {},
        selectTheme(params) {
          seen.push(params)
          return { ok: true }
        },
      },
    })
    await runner.apply(ctx)

    for (const listener of fakeChild.stdio[4].listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify({
        jsonrpc: '2.0',
        id: 'theme-select-1',
        method: '_dsh/cordis/tui/theme/selected',
        params: { protocol: 0, agentId: 'agent-1', id: 'ember' },
      })}\n`))
    }
    await new Promise((resolve) => setTimeout(resolve, 0))

    assert.deepEqual(seen, [{ protocol: 0, agentId: 'agent-1', id: 'ember' }])
  } finally {
    restore()
  }
})

test('shell binds chrome.right snapshots toward the native painter', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  try {
    const ctx = makeCtx()
    await runner.apply(ctx)
    const writes = []
    fakeChild.stdio[3].write = (chunk) => {
      writes.push(String(chunk))
      return true
    }

    ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'test' },
      [{ id: 'hello', kind: 'markdown', text: 'hello' }],
    )

    const note = writes.map((line) => JSON.parse(line)).find((entry) => entry.method === '_dsh/cordis/tui/slots/update')
    assert.ok(note, 'register emits _dsh/cordis/tui/slots/update')
    assert.equal(note.params.slot, 'chrome.right')
    assert.deepEqual(note.params.nodes, [
      { id: 'test:hello', kind: 'markdown', text: 'hello' },
    ])
  } finally {
    restore()
  }
})

test('shell binds the ACP extension transport to the sibling Cordis Client runner', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  let requestAgent
  const clientRunner = {
    bindTransport(request) {
      requestAgent = request
    },
    onHost() {},
    selectTheme() {},
  }
  try {
    await runner.apply(makeCtx({ cordisClientRunner: clientRunner }))
    assert.equal(typeof requestAgent, 'function')
  } finally {
    restore()
  }
})

test('shell routes Host Client-run requests to the sibling Cordis Client runner', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const seen = []
  const clientRunner = {
    bindTransport() {},
    sync() {},
    onHost(message) {
      seen.push(message)
    },
    selectTheme() {},
  }
  try {
    const ctx = makeCtx({ cordisClientRunner: clientRunner })
    await runner.apply(ctx)
    for (const listener of fakeChild.stdio[4].listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify({
        jsonrpc: '2.0', id: 1, method: 'initialize', params: {},
      })}\n`))
    }
    for (const listener of ctx.acpClient.stdout.listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        result: {
          protocolVersion: 1,
          agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
        },
      })}\n`))
    }
    const request = {
      jsonrpc: '2.0',
      method: '_dsh/cordis/run/request',
      params: { requestId: 'run-1', pluginId: 'dyn-1', packageId: 'pkg-1' },
    }
    for (const listener of ctx.acpClient.stdout.listeners.data ?? []) {
      listener(Buffer.from(`${JSON.stringify(request)}\n`))
    }
    assert.deepEqual(seen, [request])
  } finally {
    restore()
  }
})

test('shell dispatches Cordis TUI requests to lifecycle-owned commands and overlays', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const invoked = []
  const previews = []
  try {
    const ctx = makeCtx()
    ctx.tuiCommands.register(
      { name: 'threshold', description: 'adjust threshold' },
      (args) => {
        invoked.push(args)
        return { opened: true }
      },
    )
    ctx.tuiOverlay.openSlider(
      { id: 'threshold', title: 'Threshold', min: 0, max: 100, step: 5, value: 40 },
      { onChange: (value) => previews.push(value) },
    )
    await runner.apply(ctx)
    const writes = []
    fakeChild.stdio[3].write = (chunk) => {
      writes.push(String(chunk))
      return true
    }
    const incoming = [
      {
        jsonrpc: '2.0',
        id: 23,
        method: '_dsh/cordis/tui/commands/invoke',
        params: { protocol: 0, name: 'threshold', args: '55' },
      },
      {
        jsonrpc: '2.0',
        method: '_dsh/cordis/tui/overlay/event',
        params: { protocol: 0, id: 'threshold', event: 'change', value: 55 },
      },
    ]
    for (const message of incoming) {
      for (const listener of fakeChild.stdio[4].listeners.data ?? []) {
        listener(Buffer.from(`${JSON.stringify(message)}\n`))
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 0))

    assert.deepEqual(invoked, ['55'])
    assert.deepEqual(previews, [55])
    const response = writes
      .flatMap((line) => line.trim().split('\n').filter(Boolean))
      .map((line) => JSON.parse(line))
      .find((message) => message.id === 23)
    assert.deepEqual(response, { jsonrpc: '2.0', id: 23, result: { opened: true } })
  } finally {
    restore()
  }
})

test('hot-reloaded shell modules keep exactly one TTY painter', async () => {
  const restore = ensureTestNative()
  const href = pathToFileURL(runnerPath).href
  const nonce = `${process.pid}-${Date.now()}`
  const first = await import(`${href}?hmr=${nonce}-a`)
  const second = await import(`${href}?hmr=${nonce}-b`)
  first.resetShellForTests()
  state.spawnCalls.length = 0

  try {
    await Promise.all([first.apply(makeCtx()), second.apply(makeCtx())])
    assert.equal(
      state.spawnCalls.length,
      1,
      'module reloads must share one process-wide TTY owner',
    )
  } finally {
    first.resetShellForTests()
    restore()
  }
})

test('test shell reset retracts its process-exit listener', async () => {
  runner.resetShellForTests()
  const restore = ensureTestNative()
  const before = process.listenerCount('exit')
  try {
    await runner.apply(makeCtx())
    assert.equal(process.listenerCount('exit'), before + 1)
    runner.resetShellForTests()
    assert.equal(process.listenerCount('exit'), before)
  } finally {
    runner.resetShellForTests()
    restore()
  }
})

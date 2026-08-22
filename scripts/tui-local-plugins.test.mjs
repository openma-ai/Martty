import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

import { createTuiPluginStore } from '../npm/lib/tui-plugin-store.js'
import { installTuiCommands } from '../npm/lib/tui-commands.js'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'
import { installTuiPresets } from '../npm/lib/tui-presets.js'
import { installTuiSlots } from '../npm/lib/tui-slots.js'
import { installTuiTheme, TOKEN_NAMES } from '../npm/lib/tui-theme.js'

const localModule = await import('../npm/lib/tui-local-plugins.js').catch(() => ({}))

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

function services(ctx, options = {}) {
  const tuiCommands = installTuiCommands(ctx)
  const tuiOverlay = installTuiOverlay(ctx)
  const tuiTheme = installTuiTheme(ctx, { settingsPath: options.settingsPath })
  const tuiSlots = installTuiSlots(ctx)
  const tuiPresets = installTuiPresets({ tuiCommands, tuiOverlay })
  return { tuiCommands, tuiOverlay, tuiTheme, tuiSlots, tuiPresets }
}

function palette(id, label) {
  return {
    id,
    label,
    dark: Object.fromEntries(TOKEN_NAMES.map((token) => [token, '#111111'])),
    light: Object.fromEntries(TOKEN_NAMES.map((token) => [token, '#EEEEEE'])),
  }
}

test('a new Client process discovers and mounts saved UI Plugin artifacts', async () => {
  assert.equal(typeof localModule.installTuiLocalPlugins, 'function')
  if (typeof localModule.installTuiLocalPlugins !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-local-plugins-'))
  try {
    const store = createTuiPluginStore({ root })
    store.save({
      id: 'focused-ui',
      kind: 'ui-preset',
      name: 'Focused UI',
      purpose: 'A focused welcome layout',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiPresets'],
        apply(ctx) {
          return ctx.tuiPresets.register(
            { id: 'focused', label: 'Focused' },
            () => () => {},
          )
        },
      }`,
    })

    const ctx = makeCtx()
    const runtime = services(ctx)
    runtime.tuiPresets.register({ id: 'default', label: 'Martty' }, () => () => {})
    const local = localModule.installTuiLocalPlugins(ctx, { store, ...runtime })

    await local.discover()

    assert.deepEqual(runtime.tuiPresets.list(), [
      { id: 'default', label: 'Martty' },
      { id: 'focused', label: 'Focused' },
    ])
    assert.deepEqual(local.list(), [{
      artifactId: 'focused-ui',
      pluginId: 'tui-local:focused-ui',
      kind: 'ui',
      loaded: true,
    }])

    await local.dispose()
    assert.deepEqual(runtime.tuiPresets.list(), [
      { id: 'default', label: 'Martty' },
      { id: 'focused', label: 'Focused' },
    ])
    assert.equal(runtime.tuiPresets.catalog()[1].status, 'stopped')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('saved Theme Plugins are catalogued on discovery and load only when selected', async () => {
  assert.equal(typeof localModule.installTuiLocalPlugins, 'function')
  if (typeof localModule.installTuiLocalPlugins !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-local-plugins-'))
  try {
    const store = createTuiPluginStore({ root })
    const paper = palette('paper-lantern', 'Paper Lantern')
    store.save({
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      source: { pluginId: 'skin-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiTheme'],
        apply(ctx) { return ctx.tuiTheme.register(${JSON.stringify(paper)}) },
      }`,
    })

    const ctx = makeCtx()
    const runtime = services(ctx)
    const local = localModule.installTuiLocalPlugins(ctx, { store, ...runtime })
    await local.discover()

    assert.deepEqual(runtime.tuiTheme.list().map(({ id }) => id), ['default', 'paper-lantern'])
    assert.equal(runtime.tuiTheme.active(), 'default')
    assert.equal(runtime.tuiTheme.isLoaded('paper-lantern'), false)
    assert.equal(runtime.tuiTheme.owner('paper-lantern'), 'tui-local:paper-lantern')
    assert.equal(local.has('tui-local:paper-lantern'), true)
    assert.equal(local.isLoaded('tui-local:paper-lantern'), false)

    await local.start('tui-local:paper-lantern')
    assert.equal(runtime.tuiTheme.active(), 'paper-lantern')
    assert.equal(runtime.tuiTheme.isLoaded('paper-lantern'), true)
    assert.equal(local.isLoaded('tui-local:paper-lantern'), true)

    await local.stop('tui-local:paper-lantern')
    assert.equal(runtime.tuiTheme.active(), 'default')
    assert.equal(runtime.tuiTheme.isLoaded('paper-lantern'), false)
    assert.equal(local.isLoaded('tui-local:paper-lantern'), false)
    await local.dispose()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('one broken artifact does not block healthy startup recovery', async () => {
  assert.equal(typeof localModule.installTuiLocalPlugins, 'function')
  if (typeof localModule.installTuiLocalPlugins !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-local-plugins-'))
  try {
    const store = createTuiPluginStore({ root })
    store.save({
      id: 'healthy-ui',
      kind: 'ui-preset',
      name: 'Healthy UI',
      purpose: 'Still loads',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiPresets'],
        apply(ctx) {
          return ctx.tuiPresets.register({ id: 'healthy', label: 'Healthy' }, () => () => {})
        },
      }`,
    })
    store.save({
      id: 'broken-ui',
      kind: 'ui-preset',
      name: 'Broken UI',
      purpose: 'Fails to parse',
      source: { pluginId: 'ui-2', packageId: 'dyn-2' },
      clientCode: 'return {',
    })

    const ctx = makeCtx()
    const runtime = services(ctx)
    const local = localModule.installTuiLocalPlugins(ctx, { store, ...runtime })
    await local.discover()

    assert.ok(runtime.tuiPresets.list().some(({ id }) => id === 'healthy'))
    assert.match(local.diagnostics().find(({ artifactId }) => artifactId === 'broken-ui').message, /parse|syntax/i)
    await local.dispose()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('discovery restarts the locally persisted preferred Theme Plugin', async () => {
  assert.equal(typeof localModule.installTuiLocalPlugins, 'function')
  if (typeof localModule.installTuiLocalPlugins !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-local-plugins-'))
  const settingsPath = path.join(root, 'dsh-tui-settings.json')
  try {
    const store = createTuiPluginStore({ root: path.join(root, 'artifacts') })
    const paper = palette('paper-lantern', 'Paper Lantern')
    store.save({
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      source: { pluginId: 'skin-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiTheme'],
        apply(ctx) { return ctx.tuiTheme.register(${JSON.stringify(paper)}) },
      }`,
    })
    writeFileSync(settingsPath, JSON.stringify({ theme: 'paper-lantern' }))

    const ctx = makeCtx()
    const runtime = services(ctx, { settingsPath })
    const local = localModule.installTuiLocalPlugins(ctx, { store, ...runtime })
    await local.discover()

    assert.equal(runtime.tuiTheme.active(), 'paper-lantern')
    assert.equal(local.isLoaded('tui-local:paper-lantern'), true)
    await local.dispose()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('installed package Client modules merge ahead of same-id Creator artifacts', async () => {
  assert.equal(typeof localModule.installTuiLocalPlugins, 'function')
  if (typeof localModule.installTuiLocalPlugins !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-plugins-'))
  try {
    const entry = path.join(root, 'installed-ui.mjs')
    writeFileSync(entry, `
      export const inject = ['tuiPresets']
      export function apply(ctx) {
        return ctx.tuiPresets.register(
          { id: 'installed', label: 'Installed package' },
          () => () => {},
        )
      }
    `)
    const store = createTuiPluginStore({ root: path.join(root, 'artifacts') })
    store.save({
      id: 'shared-ui',
      kind: 'ui-preset',
      name: 'Shadowed Creator UI',
      purpose: 'Must not replace the installed package',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: `return {
        inject: ['tuiPresets'],
        apply(ctx) {
          return ctx.tuiPresets.register({ id: 'local-copy', label: 'Local copy' }, () => () => {})
        },
      }`,
    })

    const ctx = makeCtx()
    const runtime = services(ctx)
    const local = localModule.installTuiLocalPlugins(ctx, {
      store,
      packages: [{
        id: 'shared-ui',
        kind: 'ui-preset',
        entry: pathToFileURL(entry).href,
      }],
      ...runtime,
    })
    await local.discover()

    assert.ok(runtime.tuiPresets.list().some(({ id }) => id === 'installed'))
    assert.equal(runtime.tuiPresets.list().some(({ id }) => id === 'local-copy'), false)
    assert.deepEqual(local.list(), [{
      artifactId: 'shared-ui',
      pluginId: 'tui-package:shared-ui',
      kind: 'ui-preset',
      loaded: true,
    }])
    assert.match(
      local.diagnostics().find(({ artifactId }) => artifactId === 'shared-ui').message,
      /installed package|shadow/i,
    )
    await local.dispose()
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

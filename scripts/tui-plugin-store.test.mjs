import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const storeModule = await import('../npm/lib/tui-plugin-store.js').catch(() => ({}))

test('Martty home owns Creator plugins with explicit environment precedence', () => {
  assert.equal(
    storeModule.tuiPluginRoot({ MARTTY_HOME: '/opt/martty', DSH_HOME: '/opt/dsh' }),
    path.join('/opt/martty', 'plugins'),
  )
  assert.equal(
    storeModule.tuiPluginRoot({ DSH_HOME: '/opt/dsh' }),
    path.join('/opt/dsh', '.oma', 'plugins'),
  )
})

test('legacy Creator artifacts copy into Martty home without deleting the source', () => {
  const home = mkdtempSync(path.join(tmpdir(), 'martty-plugin-migration-'))
  try {
    const legacyRoot = path.join(home, '.tui-plugins')
    const root = path.join(home, '.martty', 'plugins')
    const input = {
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      source: { pluginId: 'skin-1', packageId: 'dyn-1' },
      clientCode: 'return { apply() {} }',
    }
    const legacy = storeModule.createTuiPluginStore({ root: legacyRoot })
    const legacyPath = legacy.save(input).path

    const migrated = storeModule.createTuiPluginStore({ root, legacyRoot })

    assert.equal(migrated.resolve(input.id).path, path.join(root, input.id, 'plugin.json'))
    assert.equal(existsSync(legacyPath), true, 'migration must preserve legacy data')
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
})

test('Creator TUI artifacts persist under one user root and are discovered from disk', () => {
  assert.equal(typeof storeModule.createTuiPluginStore, 'function')
  if (typeof storeModule.createTuiPluginStore !== 'function') return

  const home = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const root = path.join(home, '.tui-plugins')
    const store = storeModule.createTuiPluginStore({ root })
    const saved = store.save({
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      source: { pluginId: 'skin-1', packageId: 'dyn-1' },
      clientCode: 'return { apply() {} }',
    })

    assert.equal(saved.id, 'paper-lantern')
    assert.equal(saved.path, path.join(root, 'paper-lantern', 'plugin.json'))
    assert.deepEqual(store.list(), [{
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      path: saved.path,
      broken: undefined,
    }])
    assert.deepEqual(store.resolve('paper-lantern'), {
      schemaVersion: 0,
      id: 'paper-lantern',
      kind: 'theme',
      name: 'Paper Lantern',
      purpose: 'Warm terminal palette',
      source: { pluginId: 'skin-1', packageId: 'dyn-1' },
      code: { client: 'return { apply() {} }' },
      path: saved.path,
    })

    const manifest = JSON.parse(readFileSync(saved.path, 'utf8'))
    assert.equal(manifest.code.client, 'return { apply() {} }')

    writeFileSync(saved.path, JSON.stringify({
      ...manifest,
      name: 'Edited on disk',
    }))
    assert.equal(store.list()[0].name, 'Edited on disk', 'discovery is not memoized')
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
})

test('new UI Plugins persist as ui without Preset terminology', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const store = storeModule.createTuiPluginStore({ root })
    const saved = store.save({
      id: 'focused-ui',
      kind: 'ui',
      name: 'Focused UI',
      purpose: 'A focused layout',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: 'return { apply() {} }',
    })

    assert.equal(store.resolve('focused-ui').kind, 'ui')
    assert.equal(JSON.parse(readFileSync(saved.path, 'utf8')).kind, 'ui')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('legacy ui-preset manifests load dynamically as UI Plugins', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const dir = path.join(root, 'legacy-ui')
    mkdirSync(dir, { recursive: true })
    writeFileSync(path.join(dir, 'plugin.json'), JSON.stringify({
      schemaVersion: 0,
      id: 'legacy-ui',
      kind: 'ui-preset',
      name: 'Legacy UI',
      purpose: 'Compatibility fixture',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      code: { client: 'return { apply() {} }' },
    }))

    const store = storeModule.createTuiPluginStore({ root })
    assert.equal(store.list()[0].kind, 'ui')
    assert.equal(store.resolve('legacy-ui').kind, 'ui')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('saving an existing artifact requires explicit replacement', () => {
  assert.equal(typeof storeModule.createTuiPluginStore, 'function')
  if (typeof storeModule.createTuiPluginStore !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const store = storeModule.createTuiPluginStore({ root })
    const first = {
      id: 'focused-ui',
      kind: 'ui',
      name: 'Focused UI',
      purpose: 'A focused layout',
      source: { pluginId: 'ui-1', packageId: 'dyn-1' },
      clientCode: 'return { apply() {} }',
    }
    store.save(first)
    assert.throws(() => store.save({ ...first, clientCode: 'return {}' }), /already exists/i)

    store.save({
      ...first,
      source: { pluginId: 'ui-1', packageId: 'dyn-2' },
      clientCode: 'return { apply() { return 2 } }',
    }, { replace: true })
    assert.equal(store.resolve('focused-ui').source.packageId, 'dyn-2')
    assert.match(store.resolve('focused-ui').code.client, /return 2/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('only valid user artifact ids can be saved or removed', () => {
  assert.equal(typeof storeModule.createTuiPluginStore, 'function')
  if (typeof storeModule.createTuiPluginStore !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const store = storeModule.createTuiPluginStore({ root })
    assert.throws(() => store.save({
      id: '../escape',
      kind: 'theme',
      name: 'Escape',
      purpose: 'Bad id',
      source: { pluginId: 'x', packageId: 'y' },
      clientCode: 'return {}',
    }), /id/i)

    store.save({
      id: 'safe-theme',
      kind: 'theme',
      name: 'Safe theme',
      purpose: 'Safe id',
      source: { pluginId: 'x', packageId: 'y' },
      clientCode: 'return {}',
    })
    assert.equal(store.remove('safe-theme'), true)
    assert.equal(store.remove('safe-theme'), false)
    assert.throws(() => store.remove('../escape'), /id/i)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('malformed artifact directories remain visible as broken rows', () => {
  assert.equal(typeof storeModule.createTuiPluginStore, 'function')
  if (typeof storeModule.createTuiPluginStore !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-tui-artifacts-'))
  try {
    const dir = path.join(root, 'broken-theme')
    storeModule.createTuiPluginStore({ root }).save({
      id: 'broken-theme',
      kind: 'theme',
      name: 'Broken theme',
      purpose: 'Will be corrupted',
      source: { pluginId: 'x', packageId: 'y' },
      clientCode: 'return {}',
    })
    writeFileSync(path.join(dir, 'plugin.json'), '{')

    const [row] = storeModule.createTuiPluginStore({ root }).list()
    assert.equal(row.id, 'broken-theme')
    assert.match(row.broken, /JSON|parse|invalid/i)
    assert.throws(() => storeModule.createTuiPluginStore({ root }).resolve('broken-theme'), /broken/i)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('legacy ~/.martty migrates into the shared ~/.oma home once', () => {
  const home = mkdtempSync(path.join(tmpdir(), 'martty-oma-migration-'))
  try {
    const legacy = path.join(home, '.martty')
    mkdirSync(path.join(legacy, 'sessions'), { recursive: true })
    writeFileSync(path.join(legacy, 'settings.json'), '{"defaultHarness":"pi-acp"}')
    writeFileSync(path.join(legacy, 'sessions', 'one.jsonl'), '{}')

    const resolved = storeModule.marttyHome({}, home)
    assert.equal(resolved, path.join(home, '.oma'))
    assert.equal(
      readFileSync(path.join(resolved, 'settings.json'), 'utf8'),
      '{"defaultHarness":"pi-acp"}',
      'settings carry over to the shared home',
    )
    assert.ok(existsSync(path.join(resolved, 'sessions', 'one.jsonl')))
    assert.ok(existsSync(path.join(legacy, 'settings.json')), 'the legacy dir is left in place')

    // Later launches use the shared home directly — no re-migration.
    assert.equal(storeModule.marttyHome({}, home), resolved)

    // An existing ~/.oma always wins; nothing is overwritten by legacy data.
    rmSync(resolved, { recursive: true, force: true })
    mkdirSync(resolved, { recursive: true })
    assert.equal(storeModule.marttyHome({}, home), resolved)
    assert.ok(!existsSync(path.join(resolved, 'settings.json')))

    // $DSH_HOME resolves .oma the same way (and migrates .martty inside it).
    const dsh = mkdtempSync(path.join(tmpdir(), 'martty-oma-dsh-'))
    mkdirSync(path.join(dsh, '.martty'), { recursive: true })
    writeFileSync(path.join(dsh, '.martty', 'settings.json'), '{}')
    assert.equal(storeModule.marttyHome({ DSH_HOME: dsh }, home), path.join(dsh, '.oma'))
    assert.ok(existsSync(path.join(dsh, '.oma', 'settings.json')))
    rmSync(dsh, { recursive: true, force: true })

    // Explicit MARTTY_HOME / OMA_HOME always win and never migrate.
    assert.equal(storeModule.marttyHome({ MARTTY_HOME: '/opt/martty' }, home), '/opt/martty')
    assert.equal(storeModule.marttyHome({ OMA_HOME: '/opt/oma' }, home), '/opt/oma')
    // An empty or non-string override is treated as unset, not as the home.
    assert.equal(
      storeModule.marttyHome({ MARTTY_HOME: '', OMA_HOME: '', DSH_HOME: dsh }, home),
      path.join(dsh, '.oma'),
    )
    assert.equal(
      storeModule.marttyHome({ MARTTY_HOME: 7, OMA_HOME: false, DSH_HOME: dsh }, home),
      path.join(dsh, '.oma'),
    )
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
})

test('a failed .martty migration keeps using the legacy home', () => {
  const home = mkdtempSync(path.join(tmpdir(), 'martty-oma-migration-fail-'))
  try {
    const legacy = path.join(home, '.martty')
    mkdirSync(legacy, { recursive: true })
    const unreadable = path.join(legacy, 'locked.json')
    writeFileSync(unreadable, '{}')
    chmodSync(unreadable, 0o000)

    const resolved = storeModule.marttyHome({}, home)
    chmodSync(unreadable, 0o600)
    if (resolved === path.join(home, '.oma')) {
      // Root and other permission-bypassing environments migrate anyway —
      // the legacy fallback only needs to hold where the copy can fail.
      assert.equal(process.getuid?.(), 0)
      return
    }
    assert.equal(resolved, legacy, 'migration failure must fall back to the legacy dir')
    assert.ok(!existsSync(path.join(home, '.oma')), 'no partial shared home left behind')
  } finally {
    chmodSync(path.join(home, '.martty', 'locked.json'), 0o600)
    rmSync(home, { recursive: true, force: true })
  }
})

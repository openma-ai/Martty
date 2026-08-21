import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const repoRoot = path.resolve(import.meta.dirname, '..')
const emberPath = path.join(repoRoot, 'docs/fixtures/demo-skin.v0.json')
const npmEmberPath = path.join(repoRoot, 'npm/lib/palettes/ember.json')
const themeUrl = pathToFileURL(path.join(repoRoot, 'npm/lib/tui-theme.js')).href
const npmRequire = createRequire(path.join(repoRoot, 'npm/package.json'))
const cordisUrl = pathToFileURL(npmRequire.resolve('@deepseek-ai/cordis')).href

function loadEmber() {
  return JSON.parse(readFileSync(emberPath, 'utf8'))
}

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

const themePlugin = await import(themeUrl)
const { installTuiTheme } = themePlugin

test('theme registry is a real Cordis service plugin', async () => {
  const { Context } = await import(cordisUrl)
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  assert.equal(typeof ctx.tuiTheme.register, 'function')
  assert.deepEqual(themePlugin.inject, [])
})

test('valid ember fixture registers and disposer removes it', () => {
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  const ember = loadEmber()
  const dispose = ctx.tuiTheme.register(ember)
  assert.equal(typeof dispose, 'function')
  assert.throws(() => ctx.tuiTheme.register(structuredClone(ember)), /already registered|duplicate/i)
  dispose()
  assert.doesNotThrow(() => theme.register(structuredClone(ember)))
})

test('theme registration updates an optional background without replacing its palette', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const liang = {
    ...loadEmber(),
    id: 'liang',
    label: 'Liang',
    background: {
      source: { kind: 'file', path: '/opt/skins/liang/stage-00.png' },
      fit: 'cover',
      anchor: { x: 0.75, y: 0.5 },
      opacity: 0.42,
    },
  }

  const registration = theme.register(liang, { activate: true })
  assert.equal(typeof registration, 'function', 'legacy disposer call remains supported')
  assert.equal(typeof registration.update, 'function')
  assert.equal(registration.dispose, registration)

  registration.update({
    background: {
      source: { kind: 'file', path: '/opt/skins/liang/stage-01.png' },
      fit: 'cover',
      anchor: { x: 0.75, y: 0.5 },
      opacity: 0.42,
    },
  })

  assert.equal(theme.active(), 'liang')
  assert.deepEqual(sent.at(-1), {
    method: '_dsh/cordis/tui/theme/update',
    params: {
      protocol: 0,
      palette: {
        ...liang,
        background: {
          ...liang.background,
          source: { kind: 'file', path: '/opt/skins/liang/stage-01.png' },
        },
      },
      activate: false,
      loaded: true,
    },
  })

  registration()
  assert.equal(theme.active(), 'default')
})

test('disposing a registration removes its palette from the native catalog', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const dispose = theme.register(loadEmber())

  dispose()

  assert.deepEqual(sent.at(-1), {
    method: '_dsh/cordis/tui/theme/remove',
    params: { protocol: 0, id: 'ember' },
  })
})

test('missing token, extra token, bad hex, missing fields, and duplicate id throw', () => {
  const ctx = makeCtx()
  installTuiTheme(ctx)

  const missing = loadEmber()
  delete missing.dark.bg
  assert.throws(() => ctx.tuiTheme.register(missing), /missing|bg/i)

  const extra = loadEmber()
  extra.dark.neon = '#FF00FF'
  assert.throws(() => ctx.tuiTheme.register(extra), /unknown|extra|neon/i)

  const badHex = loadEmber()
  badHex.light.brand = '#fff'
  assert.throws(() => ctx.tuiTheme.register(badHex), /hex|#fff|brand/i)

  const noId = loadEmber()
  delete noId.id
  assert.throws(() => ctx.tuiTheme.register(noId), /id/i)

  const noLabel = loadEmber()
  delete noLabel.label
  assert.throws(() => ctx.tuiTheme.register(noLabel), /label/i)

  const first = loadEmber()
  ctx.tuiTheme.register(first)
  assert.throws(() => ctx.tuiTheme.register(structuredClone(first)), /already registered|duplicate/i)
})

test('register grows the catalog and does not cover', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const ember = loadEmber()
  ctx.tuiTheme.register(ember)
  assert.equal(theme.active(), 'default')
  assert.deepEqual(theme.list().map((p) => p.id), ['default', 'ember'])
  assert.equal(sent.length, 1)
  assert.equal(sent[0].method, '_dsh/cordis/tui/theme/update')
  assert.equal(sent[0].params.protocol, 0)
  assert.equal(sent[0].params.activate, false)
  assert.deepEqual(sent[0].params.palette, ember)
})

test('activate covers; disposing the active pack returns to default', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const ember = loadEmber()
  const moss = structuredClone(ember)
  moss.id = 'moss'
  moss.label = 'Moss'
  const disposeEmber = ctx.tuiTheme.register(ember)
  const disposeMoss = ctx.tuiTheme.register(moss)
  assert.equal(theme.active(), 'default')
  theme.activate('ember')
  assert.equal(theme.active(), 'ember')
  theme.activate('moss')
  assert.equal(theme.active(), 'moss')
  disposeMoss()
  assert.equal(theme.active(), 'default')
  const lastActivation = sent.filter((item) => item.params.activate === true).at(-1)
  assert.equal(lastActivation.params.palette.id, 'default')
  disposeEmber()
  assert.equal(theme.active(), 'default')
})

test('disposing an activated registration restores the actual previous palette', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx, {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const ember = loadEmber()
  const moss = structuredClone(ember)
  moss.id = 'moss'
  moss.label = 'Moss'

  ctx.tuiTheme.register(ember)
  theme.activate('ember')
  const disposeMoss = ctx.tuiTheme.register(moss, { activate: true })

  assert.equal(theme.active(), 'moss')
  disposeMoss()
  assert.equal(theme.active(), 'ember')
  const lastActivation = sent.filter((item) => item.params.activate === true).at(-1)
  assert.equal(lastActivation.params.palette.id, 'ember')
})

test('a later explicit choice of the same palette supersedes an activation lease', () => {
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  const ember = loadEmber()
  const moss = structuredClone(ember)
  moss.id = 'moss'
  moss.label = 'Moss'
  ctx.tuiTheme.register(ember)
  theme.activate('ember')

  const disposeMoss = ctx.tuiTheme.register(moss, { activate: true })
  theme.activate('ember')
  theme.activate('moss')
  disposeMoss()

  assert.equal(theme.active(), 'default')
})

test('theme subscribers observe active palette changes until disposed', () => {
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  const ember = loadEmber()
  const seen = []
  ctx.tuiTheme.register(ember)

  const dispose = theme.subscribe((id) => seen.push(id))
  theme.activate('ember')
  theme.activate('ember')
  theme.activate('default')
  dispose()
  theme.activate('ember')

  assert.deepEqual(seen, ['ember', 'default'])
})

test('register before notify is bound is flushed after bind', () => {
  const sent = []
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  const ember = loadEmber()
  ctx.tuiTheme.register(ember)
  assert.equal(sent.length, 0)
  theme.bindNotify((method, params) => {
    sent.push({ method, params })
  })
  assert.equal(sent.length, 1)
  assert.equal(sent[0].method, '_dsh/cordis/tui/theme/update')
  assert.equal(sent[0].params.activate, false)
  assert.deepEqual(sent[0].params.palette, ember)
})

test('npm ember.json matches docs/fixtures/demo-skin.v0.json', () => {
  const docs = JSON.parse(readFileSync(emberPath, 'utf8'))
  const npm = JSON.parse(readFileSync(npmEmberPath, 'utf8'))
  assert.deepEqual(npm, docs)
})

test('ember plugin injects tuiTheme and does not cover on apply', async () => {
  const ember = await import(pathToFileURL(path.join(repoRoot, 'npm/lib/ember.js')).href)
  assert.deepEqual(ember.inject, ['tuiTheme'])
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  ember.apply(ctx)
  assert.equal(theme.active(), 'default')
  assert.ok(theme.list().some((p) => p.id === 'ember'))
})

test('disposing a static palette fiber unregisters its palette', async () => {
  const { Context } = await import(cordisUrl)
  const ember = await import(pathToFileURL(path.join(repoRoot, 'npm/lib/ember.js')).href)
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  const fiber = ctx.plugin(ember)
  await fiber
  assert.ok(ctx.tuiTheme.list().some((palette) => palette.id === 'ember'))

  await fiber.dispose()
  assert.deepEqual(ctx.tuiTheme.list(), [{ id: 'default', label: 'Default' }])
})

test('list includes builtin default before any register', () => {
  const theme = installTuiTheme(makeCtx())
  assert.deepEqual(theme.list(), [{ id: 'default', label: 'Default' }])
  assert.equal(theme.active(), 'default')
})

test('explicit theme selection persists separately from temporary Plugin preview', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-theme-settings-'))
  const settingsPath = path.join(root, 'dsh-tui-settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({ language: 'zh', uiPreset: 'deepseek' }))
    const theme = installTuiTheme(makeCtx(), { settingsPath })
    const ember = loadEmber()
    theme.register(ember)

    const preview = theme.registerOwned('preview-plugin', {
      ...ember,
      id: 'preview',
      label: 'Preview',
    })
    assert.equal(theme.active(), 'preview')
    assert.equal(JSON.parse(readFileSync(settingsPath, 'utf8')).theme, undefined)
    preview()

    theme.activate('ember')
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')), {
      language: 'zh',
      uiPreset: 'deepseek',
      theme: 'ember',
    })

    const restored = installTuiTheme(makeCtx(), { settingsPath })
    assert.equal(restored.preferred(), 'ember')
    assert.equal(restored.active(), 'default', 'the saved Plugin is not active before it is loaded')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('exportInspectTokens lists every closed token as #RRGGBB', () => {
  const theme = installTuiTheme(makeCtx())
  const tokens = theme.exportInspectTokens()
  assert.equal(tokens.length, 18)
  assert.equal(tokens[0].valueType, '#RRGGBB')
  assert.equal(tokens[0].requiresLightAndDark, true)
  assert.ok(tokens.every((token) => typeof token.name === 'string' && token.name.length > 0))
})

const GALLERY = [
  ['one', 'One'],
  ['ayu', 'Ayu'],
  ['catppuccin', 'Catppuccin'],
  ['github', 'GitHub'],
  ['kanagawa', 'Kanagawa'],
  ['everforest', 'Everforest'],
  ['gruvbox', 'Gruvbox'],
  ['iceberg', 'Iceberg'],
  ['night-owl', 'Night Owl'],
  ['one-half', 'One Half'],
  ['seoul256', 'Seoul256'],
  ['solarized', 'Solarized'],
]

test('npm gallery palettes match docs/fixtures v0.json', () => {
  for (const [id] of GALLERY) {
    const npm = JSON.parse(
      readFileSync(path.join(repoRoot, 'npm/lib/palettes', `${id}.json`), 'utf8'),
    )
    const docs = JSON.parse(readFileSync(path.join(repoRoot, 'docs/fixtures', `${id}.v0.json`), 'utf8'))
    assert.deepEqual(npm, docs, `${id} npm vs fixture`)
  }
})

test('gallery plugins register without covering and activate round-trips', async () => {
  const plugins = {}
  for (const [id] of GALLERY) {
    plugins[id] = await import(pathToFileURL(path.join(repoRoot, `npm/lib/${id}.js`)).href)
    assert.deepEqual(plugins[id].inject, ['tuiTheme'], `${id} inject`)
  }
  const ctx = makeCtx()
  const theme = installTuiTheme(ctx)
  for (const [id] of GALLERY) plugins[id].apply(ctx)
  assert.equal(theme.active(), 'default')
  const ids = theme.list().map((p) => p.id)
  for (const [id] of GALLERY) {
    assert.ok(ids.includes(id), `catalog ${ids.join(', ')}`)
    theme.activate(id)
    assert.equal(theme.active(), id)
  }
  theme.activate('default')
  assert.equal(theme.active(), 'default')
})

test('disposing static palette fibers unregisters their palettes', async () => {
  const { Context } = await import(cordisUrl)
  const plugins = {}
  for (const [id] of GALLERY) {
    plugins[id] = await import(pathToFileURL(path.join(repoRoot, `npm/lib/${id}.js`)).href)
  }
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  const fibers = {}
  for (const [id] of GALLERY) {
    fibers[id] = ctx.plugin(plugins[id])
    await fibers[id]
    assert.ok(
      ctx.tuiTheme.list().some((palette) => palette.id === id),
      `${id} registered`,
    )
  }
  for (const [id] of GALLERY) {
    await fibers[id].dispose()
    assert.ok(
      !ctx.tuiTheme.list().some((palette) => palette.id === id),
      `${id} unregistered`,
    )
  }
  assert.deepEqual(ctx.tuiTheme.list(), [{ id: 'default', label: 'Default' }])
})

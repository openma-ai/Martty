import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const registryModule = await import('../npm/lib/tui-client-plugin-registry.js').catch(() => ({}))
const cordisModule = await import('../npm/node_modules/@deepseek-ai/cordis/lib/index.js')

test('installed packages register serializable Client entries on the Host tree', async () => {
  assert.equal(typeof registryModule.apply, 'function')
  if (typeof registryModule.apply !== 'function') return

  const { Context } = cordisModule
  const ctx = new Context()
  await ctx.plugin(registryModule)
  const entry = path.resolve('/opt/example/paper-lantern.js')
  const registration = ctx.plugin({
    name: 'paper-lantern-host-registration',
    inject: ['tuiClientPlugins'],
    apply(pluginCtx) {
      return pluginCtx.tuiClientPlugins.register({
        id: 'paper-lantern',
        kind: 'theme',
        entry,
      })
    },
  })
  await registration

  assert.deepEqual(ctx.tuiClientPlugins.list(), [{
    id: 'paper-lantern',
    kind: 'theme',
    entry: pathToFileURL(entry).href,
  }])
  const snapshot = ctx.tuiClientPlugins.list()
  snapshot[0].id = 'mutated'
  assert.equal(ctx.tuiClientPlugins.list()[0].id, 'paper-lantern')

  await registration.dispose()
  assert.deepEqual(ctx.tuiClientPlugins.list(), [])
})

test('installed package registry rejects duplicate ids and non-file Client entries', async () => {
  assert.equal(typeof registryModule.apply, 'function')
  if (typeof registryModule.apply !== 'function') return

  const { Context } = cordisModule
  const ctx = new Context()
  await ctx.plugin(registryModule)
  const release = ctx.tuiClientPlugins.register({
    id: 'focused-ui',
    kind: 'ui-preset',
    entry: '/opt/example/focused-ui.js',
  })
  assert.throws(() => ctx.tuiClientPlugins.register({
    id: 'focused-ui',
    kind: 'ui-preset',
    entry: '/opt/example/other.js',
  }), /already registered|duplicate/i)
  assert.throws(() => ctx.tuiClientPlugins.register({
    id: 'remote-ui',
    kind: 'ui-preset',
    entry: 'https://example.com/plugin.js',
  }), /file|absolute|entry/i)
  release()
})

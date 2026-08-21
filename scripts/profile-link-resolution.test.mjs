import assert from 'node:assert/strict'
import { createRequire, registerHooks } from 'node:module'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'

const harnessUrl = pathToFileURL(
  path.join(import.meta.dirname, 'profile-runner-harness.mjs'),
).href

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (
      specifier === 'node:child_process'
      && context.parentURL?.endsWith('/npm/lib/runner.js')
    ) {
      return {
        url: `data:text/javascript,${encodeURIComponent(`
          export { spawnClient as spawn } from ${JSON.stringify(harnessUrl)}
        `)}`,
        shortCircuit: true,
      }
    }
    return nextResolve(specifier, context)
  },
})

const harness = await import(harnessUrl)
const packageLib = path.join(import.meta.dirname, '../npm/lib')
const requireFromTui = createRequire(path.join(packageLib, '../package.json'))
const ownAcpPlugin = requireFromTui.resolve('@openma/deepseek-harness-acp/plugin')
const ownAcpBridge = requireFromTui.resolve('@openma/deepseek-harness-acp/bridge')

test('Host entries resolve ACP from the TUI dependency graph through the profile loader', async () => {
  const imports = []
  const embedded = {
    name: 'embedded-acp',
    inject: [],
    apply() {},
  }
  const loader = {
    async import(specifier) {
      imports.push(specifier)
      if (specifier === ownAcpPlugin) return embedded
      if (specifier === ownAcpBridge) {
        return { nodeAcpStream: harness.nodeAcpStream }
      }
      throw new Error(`unexpected loader import ${specifier}`)
    },
    unwrapExports(exports) {
      return exports
    },
  }

  const acpHost = await import(
    pathToFileURL(path.join(packageLib, 'acp-host.js')).href
  )
  const mounted = []
  const wrapperResult = await acpHost.apply({
    loader,
    plugin(plugin, config) {
      mounted.push({ plugin, config })
      return Symbol('cordis-fiber')
    },
  }, { permissionMode: 'workspace-write' })

  assert.equal(wrapperResult, undefined)
  assert.deepEqual(acpHost.inject, ['loader'])
  assert.deepEqual(mounted, [{
    plugin: embedded,
    config: { permissionMode: 'workspace-write' },
  }])

  const runner = await import(
    pathToFileURL(path.join(packageLib, 'runner.js')).href
  )
  harness.reset()
  const connection = Promise.resolve()
  connection.dispose = () => {}
  await runner.apply({
    loader,
    acpServer: { connect: () => connection },
    cmdlineArgs: { get: () => [] },
    appExit() {},
    tuiClientPlugins: { list: () => [] },
    effect() {},
  })

  assert.deepEqual(runner.inject, [
    'loader', 'acpServer', 'cmdlineArgs', 'appExit', 'tuiClientPlugins',
  ])
  assert.deepEqual(imports, [
    ownAcpPlugin,
    ownAcpBridge,
  ])
})

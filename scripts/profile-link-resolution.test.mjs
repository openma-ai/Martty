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
const ownAgentPresets = requireFromTui.resolve('@deepseek-ai/dsh-agent-presets')
const ownCordisRunner = requireFromTui.resolve('@deepseek-ai/dsh-cordis-host-runner')
const ownAcpPluginUrl = pathToFileURL(ownAcpPlugin).href
const ownAcpBridgeUrl = pathToFileURL(ownAcpBridge).href
const ownAgentPresetsUrl = pathToFileURL(ownAgentPresets).href
const ownCordisRunnerUrl = pathToFileURL(ownCordisRunner).href

test('Host entries pass resolved ACP modules to the profile loader as file URLs', async () => {
  const imports = []
  const embedded = {
    name: 'embedded-acp',
    inject: [],
    apply() {},
  }
  const services = new Map()
  const agentPresets = {
    name: 'agent-presets',
    apply() {
      services.set('agentPresets', {})
    },
  }
  const dynamicCordisRunner = {
    name: 'dynamic-cordis-runner',
    apply() {
      services.set('dynamicCordisRunner', {})
    },
  }
  const loader = {
    async import(specifier) {
      imports.push(specifier)
      if (specifier === ownAgentPresetsUrl) return agentPresets
      if (specifier === ownCordisRunnerUrl) return dynamicCordisRunner
      if (specifier === ownAcpPluginUrl) return embedded
      if (specifier === ownAcpBridgeUrl) {
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
    baseUrl: import.meta.url,
    loader,
    get(service) {
      return services.get(service)
    },
    async plugin(plugin, config) {
      mounted.push({ plugin, config })
      await plugin.apply?.(this, config)
      return Symbol('cordis-fiber')
    },
  }, { permissionMode: 'workspace-write' })

  assert.equal(wrapperResult, undefined)
  assert.deepEqual(acpHost.inject, ['loader'])
  assert.equal(mounted.length, 3)
  assert.equal(mounted[0].plugin, agentPresets)
  assert.equal(mounted[0].config.default, 'standard')
  assert.match(mounted[0].config.roots[0].path, /agent-presets$/)
  assert.equal(mounted[0].config.roots[0].trust, 'system')
  assert.deepEqual(mounted[1], { plugin: dynamicCordisRunner, config: undefined })
  assert.deepEqual(mounted[2], {
    plugin: embedded,
    config: { permissionMode: 'workspace-write' },
  })

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
    ownAgentPresetsUrl,
    ownCordisRunnerUrl,
    ownAcpPluginUrl,
    ownAcpBridgeUrl,
  ])
})

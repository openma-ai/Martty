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
const ownAcpPluginUrl = pathToFileURL(ownAcpPlugin).href
const ownAcpBridgeUrl = pathToFileURL(ownAcpBridge).href
const ownAcpPluginExports = await import(ownAcpPluginUrl)

test('Host entries pass resolved ACP modules to the profile loader as file URLs', async () => {
  const imports = []
  const loader = {
    async import(specifier) {
      imports.push(specifier)
      if (specifier === ownAcpBridgeUrl) {
        return { nodeAcpStream: harness.nodeAcpStream }
      }
      throw new Error(`unexpected loader import ${specifier}`)
    },
    unwrapExports(exports) {
      return exports.default ?? exports
    },
  }

  const acpHost = await import(
    pathToFileURL(path.join(packageLib, 'acp-host.js')).href
  )
  const mounted = []
  const wrapperResult = await acpHost.apply({
    baseUrl: import.meta.url,
    loader,
    async plugin(plugin, config) {
      mounted.push({ plugin, config })
      return Symbol('cordis-fiber')
    },
  }, { permissionMode: 'workspace-write' })

  assert.equal(wrapperResult, undefined)
  assert.deepEqual(acpHost.inject, ['loader', 'userQuestions', 'permissionPresets'])
  assert.deepEqual(mounted, [{
    plugin: loader.unwrapExports(ownAcpPluginExports),
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
    ownAcpBridgeUrl,
  ])
})

test('ACP Host adapts the legacy user-question provider to the scoped waterfall', async () => {
  const listeners = []
  const effects = []
  const userQuestions = {}
  const acpHost = await import(
    pathToFileURL(path.join(packageLib, 'acp-host.js')).href
  )
  const ctx = {
    userQuestions,
    get(name) {
      return name === 'userQuestions' ? userQuestions : undefined
    },
    on(name, listener) {
      listeners.push({ name, listener })
      return () => listeners.splice(
        listeners.findIndex((entry) => entry.listener === listener),
        1,
      )
    },
    effect(callback) {
      effects.push(callback())
    },
  }

  assert.equal(acpHost.installUserQuestionsCompatibility(ctx), true)
  const unregister = userQuestions.registerProvider({
    ask: async (request) => ({ answer: request.question }),
  })
  assert.equal(listeners[0].name, 'user-questions/request')
  assert.deepEqual(
    await listeners[0].listener({ question: 'continue?' }),
    { answer: 'continue?' },
  )

  unregister()
  assert.equal(listeners.length, 0)
  await Promise.all(effects.map((dispose) => dispose()))
  assert.equal(userQuestions.registerProvider, undefined)
})

test('ACP Host restores the immutable Session.events snapshot removed by dsh 0.1.2', async () => {
  const acpHost = await import(
    pathToFileURL(path.join(packageLib, 'acp-host.js')).href
  )
  class CurrentSession {
    snapshots = Object.freeze([{ type: 'plan/mode', data: { active: true } }])

    snapshotEvents() {
      return this.snapshots
    }
  }

  assert.equal(acpHost.installSessionEventsCompatibility(CurrentSession), true)
  const session = new CurrentSession()
  assert.equal(session.events, session.snapshots)
  assert.equal(acpHost.installSessionEventsCompatibility(CurrentSession), false)

  class ReferencedSession {
    seq = 2

    constructor() {
      // A Cordis reference can expose the Host prototype while omitting a
      // newer method from the materialized remote object.
      this.snapshotEvents = undefined
    }

    snapshotEvents() {
      return Object.freeze([])
    }

    eventAt(seq) {
      return [{ type: 'turn/start' }, { type: 'plan/mode' }][seq]
    }
  }
  assert.equal(acpHost.installSessionEventsCompatibility(ReferencedSession), true)
  assert.equal(
    typeof Object.getOwnPropertyDescriptor(ReferencedSession.prototype, 'events')?.get,
    'function',
  )
  assert.deepEqual(
    new ReferencedSession().events.map(({ type }) => type),
    ['turn/start', 'plan/mode'],
  )

  class LegacySession {
    get events() {
      return Object.freeze([])
    }
  }
  assert.equal(acpHost.installSessionEventsCompatibility(LegacySession), false)
})

test('ACP Host lets legacy ACP read dsh 0.1.2 permission state from events', async () => {
  const acpHost = await import(
    pathToFileURL(path.join(packageLib, 'acp-host.js')).href
  )
  const session = { snapshotEvents() { return [] } }
  const calls = []
  const permissionPresets = {
    current(target) {
      calls.push(target)
      return 'native-session'
    },
    derive(state) {
      calls.push(state)
      return `${state.preset}:${state.sandbox}:${state.approval}:${state.seeded}`
    },
  }

  assert.equal(acpHost.installPermissionPresetsCompatibility(permissionPresets), true)
  assert.equal(permissionPresets.current(session), 'native-session')
  assert.equal(permissionPresets.current([
    { type: 'permission/preset', data: { preset: 'strict' } },
    { type: 'sandbox/mode', data: { mode: 'workspace-write' } },
    { type: 'approval/policy', data: { policy: 'ask' } },
    { type: 'session/end-seed', data: {} },
  ]), 'strict:workspace-write:ask:true')
  assert.deepEqual(calls, [session, {
    preset: 'strict',
    sandbox: 'workspace-write',
    approval: 'ask',
    seeded: true,
  }])
  assert.equal(acpHost.installPermissionPresetsCompatibility(permissionPresets), false)
})

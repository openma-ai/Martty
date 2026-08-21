import assert from 'node:assert/strict'
import { registerHooks } from 'node:module'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'

const profileHarnessUrl = pathToFileURL(
  path.join(import.meta.dirname, 'profile-runner-harness.mjs'),
).href

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (
      specifier === 'node:child_process'
      && context.parentURL?.includes('/npm/lib/runner.js')
    ) {
      return {
        url: `data:text/javascript,${encodeURIComponent(`
          export { spawnClient as spawn } from ${JSON.stringify(profileHarnessUrl)}
        `)}`,
        shortCircuit: true,
      }
    }
    if (
      specifier === '@openma/deepseek-harness-acp/server'
      && context.parentURL?.endsWith('/npm/lib/runner.js')
    ) {
      return {
        url: `data:text/javascript,${encodeURIComponent(`
          export {
            serverName as name,
            serverInject as inject,
            applyServer as apply
          } from ${JSON.stringify(profileHarnessUrl)}
        `)}`,
        shortCircuit: true,
      }
    }
    if (
      specifier === '@openma/deepseek-harness-acp/bridge'
      && context.parentURL?.endsWith('/npm/lib/runner.js')
    ) {
      return {
        url: `data:text/javascript,${encodeURIComponent(`
          export { nodeAcpStream } from ${JSON.stringify(profileHarnessUrl)}
        `)}`,
        shortCircuit: true,
      }
    }
    if (specifier === './boot.js' && context.parentURL?.endsWith('/npm/lib/runner.js')) {
      return {
        url: `data:text/javascript,${encodeURIComponent(`
          export { bootClient } from ${JSON.stringify(profileHarnessUrl)}
        `)}`,
        shortCircuit: true,
      }
    }
    return nextResolve(specifier, context)
  },
})

const runner = await import(
  pathToFileURL(path.join(import.meta.dirname, '../npm/lib/runner.js')).href
)
const profileHarness = await import(profileHarnessUrl)
const { restoreHostTty, shouldQuitOnPainterExit } = await import(
  pathToFileURL(path.join(import.meta.dirname, '../npm/lib/index.js')).href
)

test('profile runner waits for the Host ACP server and launcher lifecycle', () => {
  assert.deepEqual(runner.inject, [
    'loader', 'acpServer', 'cmdlineArgs', 'appExit', 'tuiClientPlugins',
  ])
})

test('profile runner serializes installed package entries into the Client process', async () => {
  runner.resetProfileRunnerForTests?.()
  profileHarness.reset()
  const entries = [{
    id: 'paper-lantern',
    kind: 'theme',
    entry: 'file:///opt/example/paper-lantern.js',
  }]
  const ctx = {
    loader: {
      async import() {
        return { nodeAcpStream: profileHarness.nodeAcpStream }
      },
    },
    acpServer: profileHarness.makeServer('host-plugin'),
    cmdlineArgs: { get: () => [] },
    appExit() {},
    tuiClientPlugins: { list: () => entries.map((entry) => ({ ...entry })) },
    effect() {},
  }

  await runner.apply(ctx)

  const spawned = profileHarness.state.clientSpawns[0]
  assert.deepEqual(
    JSON.parse(spawned.options.env.DSH_TUI_CLIENT_PLUGINS_V0),
    entries,
  )
})

test('profile runner connects Host ACP to a separate TUI Client process', async () => {
  runner.resetProfileRunnerForTests?.()
  profileHarness.reset()
  const effects = []
  const exitCodes = []
  const ctx = {
    loader: {
      async import(specifier) {
        assert.notEqual(specifier, '@openma/deepseek-harness-acp/bridge')
        assert.match(specifier, /^data:text\/javascript,/)
        return { nodeAcpStream: profileHarness.nodeAcpStream }
      },
    },
    acpServer: profileHarness.makeServer('host-plugin'),
    cmdlineArgs: { get: () => ['--workspace', '/tmp/eval'] },
    tuiClientPlugins: { list: () => [] },
    appExit() {
      throw new Error('scoped appExit accessor must not own process shutdown')
    },
    get(name) {
      if (name === 'appExit') return (code) => exitCodes.push(code)
      return undefined
    },
    effect(callback) {
      effects.push(callback())
    },
  }

  await runner.apply(ctx)

  assert.equal(profileHarness.state.serverApplies, 0)
  assert.equal(profileHarness.state.connections.length, 1)
  assert.equal(profileHarness.state.bootOptions.length, 0)
  assert.equal(profileHarness.state.clientSpawns.length, 1)
  const spawned = profileHarness.state.clientSpawns[0]
  assert.equal(spawned.command, process.execPath)
  assert.deepEqual(spawned.args.slice(1), [
    '--workspace', '/tmp/eval',
  ])
  assert.deepEqual(spawned.options.stdio, [
    'pipe', 'pipe', 'inherit', process.stdin, process.stdout,
  ])
  assert.equal(profileHarness.state.nodeStreams[0].input, spawned.child.stdout)
  assert.equal(profileHarness.state.nodeStreams[0].output, spawned.child.stdin)
  assert.equal(effects.length, 1)
  assert.deepEqual(exitCodes, [])
  await Promise.all(effects.map((dispose) => dispose()))
})

test('independent runner fibers own independent Client processes', async () => {
  await runner.resetProfileRunnerForTests?.()
  profileHarness.reset()
  const firstEffects = []
  const secondEffects = []
  const makeCtx = (owner, effects) => ({
    loader: {
      async import() {
        return { nodeAcpStream: profileHarness.nodeAcpStream }
      },
    },
    acpServer: profileHarness.makeServer(owner),
    cmdlineArgs: { get: () => [] },
    tuiClientPlugins: { list: () => [] },
    appExit() {},
    effect(callback) {
      effects.push(callback())
    },
  })

  await runner.apply(makeCtx('first-host', firstEffects))
  await runner.apply(makeCtx('second-host', secondEffects))

  assert.equal(profileHarness.state.clientSpawns.length, 2)
  assert.equal(profileHarness.state.connections.length, 2)
  await Promise.all(firstEffects.map((dispose) => dispose()))
  assert.equal(profileHarness.state.clientSpawns[0].child.killedWith, 'SIGTERM')
  assert.equal(profileHarness.state.clientSpawns[1].child.killedWith, undefined)
  await Promise.all(secondEffects.map((dispose) => dispose()))
})

test('a failed ACP connection retracts the Client process it already spawned', async () => {
  runner.resetProfileRunnerForTests?.()
  profileHarness.reset()
  const expected = new Error('connection rejected')
  const ctx = {
    loader: {
      async import() {
        return { nodeAcpStream: profileHarness.nodeAcpStream }
      },
    },
    acpServer: {
      connect() {
        throw expected
      },
    },
    cmdlineArgs: { get: () => [] },
    tuiClientPlugins: { list: () => [] },
    appExit() {},
    effect() {},
  }

  await assert.rejects(runner.apply(ctx), expected)

  assert.equal(profileHarness.state.clientSpawns.length, 1)
  assert.equal(profileHarness.state.clientSpawns[0].child.killedWith, 'SIGTERM')
})

test('Client exit requests app shutdown before ACP cleanup settles', async () => {
  await runner.resetProfileRunnerForTests?.()
  profileHarness.reset()
  const exitCodes = []
  let finishDispose
  const disposing = new Promise((resolve) => {
    finishDispose = resolve
  })
  const connection = {
    then(resolve) {
      resolve()
    },
    dispose() {
      return disposing
    },
  }
  const ctx = {
    loader: {
      async import() {
        return { nodeAcpStream: profileHarness.nodeAcpStream }
      },
    },
    acpServer: { connect: () => connection },
    cmdlineArgs: { get: () => [] },
    tuiClientPlugins: { list: () => [] },
    appExit() {
      throw new Error('scoped appExit accessor must not own process shutdown')
    },
    get(name) {
      if (name === 'appExit') return (code) => exitCodes.push(code)
      return undefined
    },
    effect() {},
  }

  await runner.apply(ctx)
  const child = profileHarness.state.clientSpawns[0].child
  child.exitCode = 0
  child.emit('exit', 0, null)
  await new Promise((resolve) => setImmediate(resolve))

  try {
    assert.deepEqual(exitCodes, [0])
  } finally {
    finishDispose()
    await runner.resetProfileRunnerForTests?.()
  }
})

test('painter SIGTERM (fiber dispose) is not a process quit', () => {
  assert.equal(shouldQuitOnPainterExit(null), true)
  assert.equal(shouldQuitOnPainterExit('SIGINT'), true)
  assert.equal(shouldQuitOnPainterExit('SIGTERM'), false)
})

test('host TTY restore is a no-op without a TTY', () => {
  assert.doesNotThrow(() => restoreHostTty())
})

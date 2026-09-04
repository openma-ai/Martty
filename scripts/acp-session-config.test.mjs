import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import test from 'node:test'

import * as acpClient from '../npm/lib/acp-client.js'
import { applyClientHalf } from '../npm/lib/client-run.js'
import { CORDIS_METHODS } from '../npm/lib/cordis-protocol.js'
import { muxAcpAndCompositor } from '../npm/lib/mux.js'

function makeCtx() {
  return {
    effect(setup) {
      return setup()
    },
  }
}

const initialOptions = [
  {
    type: 'select',
    id: 'effort',
    name: 'Reasoning effort',
    category: 'thought_level',
    currentValue: 'high',
    options: [
      { value: 'off', name: 'Off' },
      { value: 'high', name: 'High' },
      { value: 'max', name: 'Max' },
    ],
  },
  {
    type: 'boolean',
    id: 'telemetry',
    name: 'Telemetry',
    currentValue: true,
  },
]

test('ACP Session config service folds setup responses and config_option_update snapshots', () => {
  assert.equal(typeof acpClient.installAcpSessionConfig, 'function')
  if (typeof acpClient.installAcpSessionConfig !== 'function') return
  const service = acpClient.installAcpSessionConfig(makeCtx())

  service.observeClient({ jsonrpc: '2.0', id: 41, method: 'session/new', params: { cwd: '/tmp' } })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 41,
    result: { sessionId: 's-1', configOptions: initialOptions },
  })

  assert.equal(service.current('effort'), 'high')
  assert.equal(service.current('telemetry'), true)
  assert.deepEqual(service.list(), initialOptions)

  const listed = service.list()
  listed[0].currentValue = 'mutated-outside'
  assert.equal(service.current('effort'), 'high', 'callers cannot mutate the live snapshot')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: {
        sessionUpdate: 'config_option_update',
        configOptions: [
          { ...initialOptions[0], currentValue: 'max' },
          initialOptions[1],
        ],
      },
    },
  })
  assert.equal(service.current('effort'), 'max')
})

test('ACP Session config resolves standard semantic categories from each live snapshot', () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 61, method: 'session/new', params: {} })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 61,
    result: {
      sessionId: 's-category',
      configOptions: [{
        ...initialOptions[0],
        id: 'agent-specific-effort-a',
      }],
    },
  })

  const first = service.byCategory('thought_level')
  assert.deepEqual(first.map((option) => option.id), ['agent-specific-effort-a'])
  first[0].id = 'mutated-outside'
  assert.equal(service.byCategory('thought_level')[0].id, 'agent-specific-effort-a')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-category',
      update: {
        sessionUpdate: 'config_option_update',
        configOptions: [{
          ...initialOptions[0],
          id: 'agent-specific-effort-b',
        }],
      },
    },
  })
  assert.deepEqual(
    service.byCategory('thought_level').map((option) => option.id),
    ['agent-specific-effort-b'],
  )
  assert.deepEqual(service.byCategory('model'), [])
  assert.throws(() => service.byCategory(''), /non-empty string/)
})

test('ACP Session config keeps tab snapshots isolated and writes the selected Session', async () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 71, method: 'session/new', params: {} })
  service.observeAgent({
    jsonrpc: '2.0', id: 71,
    result: { sessionId: 's-1', configOptions: initialOptions },
  })
  service.observeClient({
    jsonrpc: '2.0', id: 72, method: 'session/resume', params: { sessionId: 's-2' },
  })
  service.observeAgent({
    jsonrpc: '2.0', id: 72,
    result: {
      sessionId: 's-2',
      configOptions: [{ ...initialOptions[0], currentValue: 'max' }],
    },
  })

  service.selectSession('s-1')
  service.observeAgent({
    jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: 's-2',
      update: {
        sessionUpdate: 'config_option_update',
        configOptions: [{ ...initialOptions[0], currentValue: 'off' }],
      },
    },
  })
  assert.equal(service.current('effort'), 'high', 'background updates stay cached off-screen')

  const writes = []
  service.bindTransport(async (_method, params) => {
    writes.push(params)
    return {
      sessionId: params.sessionId,
      configOptions: [{ ...initialOptions[0], currentValue: params.value }],
    }
  })
  await service.set('effort', 'off')
  assert.equal(writes[0].sessionId, 's-1')

  service.selectSession('s-2')
  assert.equal(service.current('effort'), 'off')
})

test('config transactions remain pinned to their originating Session across a tab switch', async () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  for (const [id, sessionId, currentValue] of [[81, 's-1', 'high'], [82, 's-2', 'max']]) {
    service.observeClient({ jsonrpc: '2.0', id, method: 'session/load', params: { sessionId } })
    service.observeAgent({
      jsonrpc: '2.0', id,
      result: { sessionId, configOptions: [{ ...initialOptions[0], currentValue }] },
    })
  }
  const writes = []
  service.bindTransport(async (_method, params) => {
    writes.push(params)
    return {
      sessionId: params.sessionId,
      configOptions: [{ ...initialOptions[0], currentValue: params.value }],
    }
  })

  service.selectSession('s-1')
  const transaction = service.transaction({ id: 'effort' })
  service.selectSession('s-2')
  await transaction.preview('off')
  await transaction.rollback()

  assert.deepEqual(writes.map(({ sessionId, value }) => ({ sessionId, value })), [
    { sessionId: 's-1', value: 'off' },
    { sessionId: 's-1', value: 'high' },
  ])
  assert.equal(service.current('effort'), 'max')
})

test('dynamic Client resolves Session config by standard category through its restricted context', async () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 62, method: 'session/new', params: {} })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 62,
    result: {
      sessionId: 's-dynamic-category',
      configOptions: [{
        ...initialOptions[0],
        id: 'agent-owned-thought-level',
      }],
    },
  })

  globalThis.__dshDynamicCategory = undefined
  try {
    await applyClientHalf(
      `return {
        inject: ['acpSessionConfig'],
        apply(ctx) {
          globalThis.__dshDynamicCategory = ctx.acpSessionConfig
            .byCategory('thought_level')
            .map((option) => option.id)
        },
      }`,
      { acpSessionConfig: service },
    )

    assert.deepEqual(globalThis.__dshDynamicCategory, ['agent-owned-thought-level'])
  } finally {
    delete globalThis.__dshDynamicCategory
  }
})

test('dynamic config transaction deduplicates previews and rolls back with its Plugin run', async () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 63, method: 'session/new', params: {} })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 63,
    result: {
      sessionId: 's-preview-rollback',
      configOptions: [{ ...initialOptions[0], id: 'agent-preview-depth' }],
    },
  })
  const writes = []
  service.bindTransport(async (_method, params) => {
    writes.push(params.value)
    return {
      sessionId: params.sessionId,
      configOptions: [{
        ...initialOptions[0],
        id: params.configId,
        currentValue: params.value,
      }],
    }
  })

  const applied = await applyClientHalf(
    `return {
      inject: ['acpSessionConfig'],
      async apply(ctx) {
        const transaction = ctx.acpSessionConfig.transaction({ category: 'thought_level' })
        await transaction.preview('max')
        await transaction.preview('max')
      },
    }`,
    { acpSessionConfig: service },
  )

  assert.deepEqual(writes, ['max'])
  assert.equal(service.current('agent-preview-depth'), 'max')
  applied.dispose()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(writes, ['max', 'high'])
  assert.equal(service.current('agent-preview-depth'), 'high')
})

test('committed dynamic config transaction keeps its final value when the Plugin run disposes', async () => {
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 64, method: 'session/new', params: {} })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 64,
    result: {
      sessionId: 's-preview-commit',
      configOptions: [{ ...initialOptions[0], id: 'agent-commit-depth' }],
    },
  })
  const writes = []
  service.bindTransport(async (_method, params) => {
    writes.push(params.value)
    return {
      sessionId: params.sessionId,
      configOptions: [{
        ...initialOptions[0],
        id: params.configId,
        currentValue: params.value,
      }],
    }
  })

  const applied = await applyClientHalf(
    `return {
      inject: ['acpSessionConfig'],
      async apply(ctx) {
        const transaction = ctx.acpSessionConfig.transaction({ id: 'agent-commit-depth' })
        await transaction.preview('max')
        await transaction.commit()
      },
    }`,
    { acpSessionConfig: service },
  )

  applied.dispose()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(writes, ['max'])
  assert.equal(service.current('agent-commit-depth'), 'max')
})

test('ACP Session config set stays on the Rust-owned standard request path and folds response-only state', async () => {
  assert.equal(typeof acpClient.installAcpSessionConfig, 'function')
  if (typeof acpClient.installAcpSessionConfig !== 'function') return
  const service = acpClient.installAcpSessionConfig(makeCtx())
  service.observeClient({ jsonrpc: '2.0', id: 7, method: 'session/load', params: { sessionId: 's-7' } })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 7,
    result: { configOptions: initialOptions },
  })

  const requests = []
  service.bindTransport(async (method, params) => {
    requests.push({ method, params })
    return {
      sessionId: 's-7',
      configOptions: [
        { ...initialOptions[0], currentValue: 'off' },
        initialOptions[1],
      ],
    }
  })

  const options = await service.set('effort', 'off')

  assert.deepEqual(requests, [{
    method: CORDIS_METHODS.sessionConfigSet,
    params: { protocol: 0, sessionId: 's-7', configId: 'effort', value: 'off' },
  }])
  assert.equal(service.current('effort'), 'off')
  assert.deepEqual(options, service.list())

  await assert.rejects(service.set('telemetry', 'yes'), /boolean value/)
  await service.set('telemetry', false)
  assert.equal(requests.at(-1).params.value, false)
})

test('dynamic Client subscriptions are disposed with their Plugin run', async () => {
  assert.equal(typeof acpClient.installAcpSessionConfig, 'function')
  if (typeof acpClient.installAcpSessionConfig !== 'function') return
  const service = acpClient.installAcpSessionConfig(makeCtx())
  const seen = []
  globalThis.__dshConfigSeen = seen
  try {
    const applied = await applyClientHalf(
      `return {
        inject: ['acpSessionConfig'],
        apply(ctx) {
          ctx.acpSessionConfig.subscribe((snapshot) => {
            globalThis.__dshConfigSeen.push(snapshot.options[0].currentValue)
          })
        },
      }`,
      { acpSessionConfig: service },
    )
    service.observeClient({ jsonrpc: '2.0', id: 9, method: 'session/new', params: {} })
    service.observeAgent({
      jsonrpc: '2.0', id: 9, result: { sessionId: 's-9', configOptions: initialOptions },
    })
    assert.deepEqual(seen, ['high'])

    applied.dispose()
    service.observeAgent({
      jsonrpc: '2.0',
      method: 'session/update',
      params: {
        sessionId: 's-9',
        update: {
          sessionUpdate: 'config_option_update',
          configOptions: [{ ...initialOptions[0], currentValue: 'max' }],
        },
      },
    })
    assert.deepEqual(seen, ['high'])
  } finally {
    delete globalThis.__dshConfigSeen
  }
})

test('mux routes Client-host config requests to Rust and observes only standard ACP traffic', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const toAgent = []
  const toTui = []
  const observed = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))
  tuiOut.on('data', (chunk) => toTui.push(String(chunk)))

  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
    onAcp(direction, message) {
      observed.push({ direction, method: message.method, id: message.id })
    },
  })

  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'session/new', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, result: { sessionId: 's-1' } })}\n`)
  const pending = mux.requestTui(CORDIS_METHODS.sessionConfigSet, {
    protocol: 0, sessionId: 's-1', configId: 'effort', value: 'max',
  })
  await new Promise((resolve) => setTimeout(resolve, 10))

  const internal = toTui
    .flatMap((line) => line.trim().split('\n').filter(Boolean))
    .map((line) => JSON.parse(line))
    .find((message) => message.method === CORDIS_METHODS.sessionConfigSet)
  assert.ok(internal)
  assert.equal(toAgent.some((line) => line.includes(CORDIS_METHODS.sessionConfigSet)), false)
  tuiIn.write(`${JSON.stringify({
    jsonrpc: '2.0', id: internal.id, result: { sessionId: 's-1', configOptions: initialOptions },
  })}\n`)

  assert.deepEqual(await pending, { sessionId: 's-1', configOptions: initialOptions })
  assert.deepEqual(observed, [
    { direction: 'client', method: 'session/new', id: 1 },
    { direction: 'agent', method: undefined, id: 1 },
  ])
})

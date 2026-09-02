import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import test from 'node:test'
import { isCompositorMessage, muxAcpAndCompositor } from '../npm/lib/mux.js'

test('compositor methods stay off the agent stream', () => {
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/theme/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/theme/selected' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/theme/remove' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/slots/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/commands/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/overlay/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/commands/invoke' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/overlay/event' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/queue/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/agents/update' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/agents/select' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/tui/agents/navigate' }), true)
  assert.equal(isCompositorMessage({ method: '_dsh/cordis/plugins/list' }), false)
  assert.equal(isCompositorMessage({ method: 'tui/palette' }), false)
  assert.equal(isCompositorMessage({ method: 'session/prompt' }), false)
  assert.equal(isCompositorMessage({ method: 'initialize' }), false)
})

test('mux forwards ACP both ways and injects palette only toward the TUI', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const toAgent = []
  const toTui = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))
  tuiOut.on('data', (chunk) => toTui.push(String(chunk)))

  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
  })

  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 's' } })}\n`)
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    result: {
      protocolVersion: 1,
      agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
    },
  })}\n`)
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 2, method: '_dsh/cordis/plugins/list', params: { agentId: 's' } })}\n`)
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', method: '_dsh/cordis/tui/slots/update', params: {} })}\n`)
  mux.notifyTui('_dsh/cordis/tui/theme/update', { protocol: 0, activate: true })

  await new Promise((resolve) => setTimeout(resolve, 20))

  assert.equal(toAgent.some((line) => line.includes('"initialize"')), true)
  assert.equal(toAgent.some((line) => line.includes('_dsh/cordis/plugins/list')), true)
  assert.equal(toAgent.some((line) => line.includes('_dsh/cordis/tui/slots/update')), false)
  assert.equal(toAgent.some((line) => line.includes('_dsh/cordis/tui/theme/update')), false)
  assert.equal(toTui.some((line) => line.includes('session/update')), true)
  assert.equal(toTui.some((line) => line.includes('_dsh/cordis/tui/theme/update')), true)
})

test('mux routes TUI command and overlay events to the compositor owner', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const toAgent = []
  const toTui = []
  const compositor = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))
  tuiOut.on('data', (chunk) => toTui.push(String(chunk)))

  muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
    onCompositor(message) {
      compositor.push(message)
      return { handled: true }
    },
  })
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 17, method: '_dsh/cordis/tui/commands/invoke', params: { name: 'threshold' } })}\n`)
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', method: '_dsh/cordis/tui/overlay/event', params: { id: 'threshold', event: 'change', value: 45 } })}\n`)

  await new Promise((resolve) => setTimeout(resolve, 20))

  assert.deepEqual(compositor.map((message) => message.method), [
    '_dsh/cordis/tui/commands/invoke',
    '_dsh/cordis/tui/overlay/event',
  ])
  assert.equal(toAgent.some((line) => line.includes('_dsh/cordis/tui/commands/invoke')), false)
  assert.equal(toAgent.some((line) => line.includes('_dsh/cordis/tui/overlay/event')), false)
  assert.deepEqual(
    toTui.flatMap((line) => line.trim().split('\n').filter(Boolean)).map((line) => JSON.parse(line)),
    [{ jsonrpc: '2.0', id: 17, result: { handled: true } }],
  )
})

test('mux enables host Cordis requests only after the agent advertises the protocol', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const toAgent = []
  const ready = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))

  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
    onCordisReady(capability) {
      ready.push(capability)
    },
  })

  const early = assert.rejects(
    mux.requestAgent('_dsh/cordis/inspect/sync', { providers: [] }),
    /agent has not advertised _dsh\/cordis/,
  )
  await new Promise((resolve) => setTimeout(resolve, 10))
  const leaked = toAgent
    .flatMap((line) => line.trim().split('\n').filter(Boolean))
    .map((line) => JSON.parse(line))
    .find((message) => message.method === '_dsh/cordis/inspect/sync')
  if (leaked !== undefined) {
    agentOut.write(`${JSON.stringify({
      jsonrpc: '2.0',
      id: leaked.id,
      error: { code: -32601, message: 'unexpected early request' },
    })}\n`)
  }
  await early
  assert.equal(leaked, undefined)

  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    result: {
      protocolVersion: 1,
      agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
    },
  })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))

  assert.deepEqual(ready, [{ protocol: 0 }])
  const request = mux.requestAgent('_dsh/cordis/inspect/sync', { providers: [] })
  await new Promise((resolve) => setTimeout(resolve, 10))
  const sent = toAgent
    .flatMap((line) => line.trim().split('\n').filter(Boolean))
    .map((line) => JSON.parse(line))
    .find((message) => message.method === '_dsh/cordis/inspect/sync')
  assert.ok(sent)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', id: sent.id, result: { ok: true } })}\n`)
  assert.deepEqual(await request, { ok: true })
})

test('mux forgets the previous Agent capabilities when its Harness is replaced', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
  })

  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    result: {
      protocolVersion: 1,
      agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
    },
  })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))

  mux.resetAgent()
  await assert.rejects(
    mux.requestAgent('_dsh/cordis/inspect/sync', { providers: [] }),
    /agent has not advertised _dsh\/cordis/,
  )
})

test('mux keeps Cordis disabled when the initialize response omits the capability', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const ready = []
  const toAgent = []
  const toTui = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))
  tuiOut.on('data', (chunk) => toTui.push(String(chunk)))

  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
    onCordisReady(capability) {
      ready.push(capability)
    },
  })
  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'initialize', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 2,
    result: { protocolVersion: 1, agentCapabilities: {} },
  })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))

  assert.deepEqual(ready, [])
  const request = assert.rejects(
    mux.requestAgent('_dsh/cordis/plugins/list', { agentId: 'agent-1' }),
    /agent has not advertised _dsh\/cordis/,
  )
  await new Promise((resolve) => setTimeout(resolve, 10))
  const leaked = toAgent
    .flatMap((line) => line.trim().split('\n').filter(Boolean))
    .map((line) => JSON.parse(line))
    .find((message) => message.method === '_dsh/cordis/plugins/list')
  if (leaked !== undefined) {
    agentOut.write(`${JSON.stringify({
      jsonrpc: '2.0',
      id: leaked.id,
      error: { code: -32601, message: 'unexpected unsupported request' },
    })}\n`)
  }
  await request
  assert.equal(leaked, undefined)

  tuiIn.write(`${JSON.stringify({
    jsonrpc: '2.0', id: 3, method: '_dsh/cordis/plugins/list', params: { agentId: 'agent-1' },
  })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))
  assert.equal(toAgent.some((line) => line.includes('"id":3')), false)
  assert.deepEqual(
    toTui
      .flatMap((line) => line.trim().split('\n').filter(Boolean))
      .map((line) => JSON.parse(line))
      .find((message) => message.id === 3),
    {
      jsonrpc: '2.0',
      id: 3,
      error: { code: -32601, message: 'agent has not advertised _dsh/cordis' },
    },
  )
})

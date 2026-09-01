import assert from 'node:assert/strict'
import test from 'node:test'

import { installAcpSessionStatus } from '../npm/lib/acp-session-status.js'

const PROGRESS_UPDATE_TYPES = [
  'user_message_chunk',
  'agent_message_chunk',
  'agent_thought_chunk',
  'tool_call',
  'tool_call_update',
  'plan',
  'plan_update',
  'plan_removed',
  'usage_update',
]

function makeCtx() {
  return {
    effect(setup) {
      return setup()
    },
  }
}

function makeSessionConfig() {
  const listeners = new Set()
  return {
    options: [
      {
        type: 'select',
        id: 'model',
        name: 'Model',
        currentValue: 'deepseek-v4-flash',
        options: [],
      },
      {
        type: 'select',
        id: 'effort',
        name: 'Reasoning effort',
        currentValue: 'high',
        options: [],
      },
    ],
    list() {
      return this.options
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    emit() {
      for (const listener of [...listeners]) listener({
        sessionId: undefined,
        options: this.options,
      })
    },
  }
}

test('the status service folds connection, server, auth, and session facts', () => {
  const sessionConfig = makeSessionConfig()
  const events = { register: () => () => {} }
  const service = installAcpSessionStatus(makeCtx(), { events, sessionConfig })
  const current = service.current()
  assert.equal(current.state, 'idle')
  assert.equal(current.connection, 'connecting')
  assert.equal(current.session.bound, false)
  // Seed folded from the already-advertised config options.
  assert.equal(current.model, 'deepseek-v4-flash')
  assert.equal(current.effort, 'high')

  service.observeClient({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })
  service.observeAgent({
    jsonrpc: '2.0',
    id: 1,
    result: {
      agentInfo: { name: 'dsh-acp' },
      authMethods: [{ id: 'agent', name: 'Agent' }],
    },
  })
  assert.equal(service.current().connection, 'attached')
  assert.equal(service.current().server, 'dsh-acp')
  assert.equal(service.current().auth.method, 'Agent')
  assert.equal(service.current().auth.status, undefined)

  service.observeClient({ jsonrpc: '2.0', id: 2, method: 'session/new', params: {} })
  service.observeAgent({ jsonrpc: '2.0', id: 2, result: { sessionId: 's-1' } })
  assert.deepEqual(service.current().session, { sessionId: 's-1', bound: true })

  // authRequired errors anywhere flip the authenticate state.
  service.observeAgent({
    jsonrpc: '2.0',
    id: 9,
    error: { code: -32000, message: 'auth required' },
  })
  assert.equal(service.current().auth.status, 'needs sign-in')

  service.observeClient({ jsonrpc: '2.0', id: 3, method: 'authenticate', params: {} })
  assert.equal(service.current().auth.status, 'signing in')
  service.observeAgent({ jsonrpc: '2.0', id: 3, result: {} })
  assert.equal(service.current().auth.status, 'configured')
})

test('the status service keeps the session.status run-state extension', () => {
  const sessionConfig = makeSessionConfig()
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig,
  })

  service.observeClient({
    jsonrpc: '2.0', id: 1, method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')
  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'hi' } },
    },
  })
  assert.equal(service.current().state, 'running')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'idle' },
  })
  assert.equal(service.current().state, 'running', 'idle waits for the prompt response')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'running' },
  })
  assert.equal(service.current().state, 'running')

  service.observeAgent({ jsonrpc: '2.0', id: 1, result: { stopReason: 'end_turn' } })
  assert.equal(service.current().state, 'idle')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'running' },
  })
  assert.equal(service.current().state, 'running')
  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'idle' },
  })
  assert.equal(service.current().state, 'idle')
})

test('prompt responses and errors settle standard ACP run state', () => {
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig: makeSessionConfig(),
  })

  service.observeClient({
    jsonrpc: '2.0', id: 'prompt-1', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 'another-session',
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'no' } },
    },
  })
  assert.equal(service.current().state, 'starting', 'another session is unrelated')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'available_commands_update', availableCommands: [] },
    },
  })
  assert.equal(service.current().state, 'starting', 'administrative updates are not progress')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-1',
      update: { sessionUpdate: 'plan', entries: [] },
    },
  })
  assert.equal(service.current().state, 'running', 'the first related progress update starts the run')

  service.observeAgent({ jsonrpc: '2.0', id: 'prompt-1', result: { stopReason: 'end_turn' } })
  assert.equal(service.current().state, 'idle')

  service.observeClient({
    jsonrpc: '2.0', id: 'prompt-2', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')
  service.observeAgent({
    jsonrpc: '2.0', id: 'prompt-2', error: { code: -32603, message: 'agent failed' },
  })
  assert.equal(service.current().state, 'idle')

  service.observeClient({
    jsonrpc: '2.0', id: 'prompt-auth', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  service.observeAgent({
    jsonrpc: '2.0', id: 'prompt-auth', error: { code: -32000, message: 'auth required' },
  })
  assert.equal(service.current().state, 'idle')
  assert.equal(service.current().auth.status, 'needs sign-in')
})

test('standard prompt progress updates promote starting to running', () => {
  for (const sessionUpdate of PROGRESS_UPDATE_TYPES) {
    const service = installAcpSessionStatus(makeCtx(), {
      events: { register: () => () => {} },
      sessionConfig: makeSessionConfig(),
    })
    service.observeClient({
      jsonrpc: '2.0',
      id: `prompt-${sessionUpdate}`,
      method: 'session/prompt',
      params: { sessionId: 's-1' },
    })
    service.observeAgent({
      jsonrpc: '2.0',
      method: 'session/update',
      params: { sessionId: 's-1', update: { sessionUpdate } },
    })
    assert.equal(service.current().state, 'running', sessionUpdate)
  }
})

test('cancel and concurrent steer prompts stay active until every response settles', () => {
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig: makeSessionConfig(),
  })

  service.observeClient({
    jsonrpc: '2.0', id: 7, method: 'session/prompt', params: { sessionId: 's-1' },
  })
  service.observeClient({
    jsonrpc: '2.0', id: '7', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')

  service.observeClient({
    jsonrpc: '2.0', method: 'session/cancel', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting', 'cancel is a notification, not completion')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: { sessionId: 's-1', update: { sessionUpdate: 'agent_thought_chunk' } },
  })
  assert.equal(service.current().state, 'running', 'post-cancel progress remains observable')

  service.observeAgent({ jsonrpc: '2.0', id: '7', result: { stopReason: 'cancelled' } })
  assert.equal(service.current().state, 'running', 'the original prompt is still pending')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'idle' },
  })
  assert.equal(service.current().state, 'running', 'the extension cannot hide a pending prompt')

  service.observeAgent({ jsonrpc: '2.0', id: 999, result: { stopReason: 'end_turn' } })
  assert.equal(service.current().state, 'running', 'an unrelated response cannot settle the run')

  service.observeAgent({ jsonrpc: '2.0', id: 7, result: { stopReason: 'cancelled' } })
  assert.equal(service.current().state, 'idle')
})

test('only response envelopes settle prompt request ids', () => {
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig: makeSessionConfig(),
  })

  service.observeClient({
    jsonrpc: '2.0', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'idle', 'a notification has no response to await')

  service.observeClient({
    jsonrpc: '2.0', id: 0, method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')

  service.observeAgent({
    jsonrpc: '2.0', id: 0, method: 'session/request_permission', params: {},
  })
  service.observeAgent({ jsonrpc: '2.0', id: 0 })
  assert.equal(service.current().state, 'starting', 'agent requests and malformed envelopes do not settle')

  service.observeAgent({ jsonrpc: '2.0', id: 0, result: 'non-object result' })
  assert.equal(service.current().state, 'idle', 'the result payload shape does not change the boundary')

  service.observeClient({
    jsonrpc: '2.0', id: null, method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().state, 'starting')
  service.observeAgent({ jsonrpc: '2.0', id: null, result: null })
  assert.equal(service.current().state, 'idle')
})

test('the status service folds session.event mode facts', () => {
  const sessionConfig = makeSessionConfig()
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig,
  })

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.event',
    params: {
      sessionId: 's-1',
      event: { type: 'permission/preset', data: { preset: 'workspace-write' } },
    },
  })
  assert.equal(service.current().permission, 'workspace-write')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.event',
    params: {
      sessionId: 's-1',
      event: { type: 'sandbox/mode', data: { mode: 'read-only' } },
    },
  })
  assert.equal(service.current().permission, 'workspace-write', 'preset wins over sandbox')

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.event',
    params: {
      sessionId: 's-1',
      event: { type: 'plan/mode', data: { active: true } },
    },
  })
  assert.equal(service.current().plan, true)

  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.event',
    params: {
      sessionId: 's-1',
      event: { type: 'agent-preset/selected', data: { agentPreset: 'code' } },
    },
  })
  assert.equal(service.current().agent, 'code')

  // Snapshots are copies: mutation cannot reach the live value.
  const snapshot = service.current()
  snapshot.plan = false
  assert.equal(service.current().plan, true)
})

test('the status service re-reports subscriptions', () => {
  const sessionConfig = makeSessionConfig()
  const snapshots = []
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig,
  })
  service.subscribe((snapshot) => snapshots.push(snapshot))
  service.observeAgent({
    jsonrpc: '2.0',
    method: 'session.status',
    params: { sessionId: 's-1', status: 'running' },
  })
  assert.equal(snapshots.length, 1)
  assert.equal(snapshots[0].state, 'running')
})

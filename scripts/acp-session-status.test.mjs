import assert from 'node:assert/strict'
import test from 'node:test'

import { installAcpSessionStatus } from '../npm/lib/acp-session-status.js'

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

test('the status service folds run state and session.event mode facts', () => {
  const sessionConfig = makeSessionConfig()
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig,
  })

  service.observeClient({ jsonrpc: '2.0', id: 1, method: 'session/prompt', params: {} })
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
  assert.equal(service.current().state, 'idle')

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

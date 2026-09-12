import assert from 'node:assert/strict'
import test from 'node:test'

import { installAcpSessionStatus } from '../npm/lib/acp-session-status.js'
import { installAcpSessionConfig } from '../npm/lib/acp-session-config.js'
import { installAcpSessionStats } from '../npm/lib/acp-session-stats.js'
import { installAcpSessionPlan } from '../npm/lib/acp-session-plan.js'

test('each tab publishes its own Harness identity after a different Harness creates a session', () => {
  const status = installAcpSessionStatus(makeCtx())
  status.observeClient({id:'init',method:'initialize'})
  status.observeAgent({id:'init',result:{agentInfo:{name:'alpha'},authMethods:[]}})
  for (const [id, name] of [['a','alpha'],['b','beta']]) {
    status.observeClient({id,method:'session/new'})
    status.observeAgent({id,result:{sessionId:id,_meta:{marttyConnection:{id:name,agentInfo:{name},authMethods:[]}}}})
    if (id === 'a') status.selectSession('a')
  }
  assert.equal(status.current().server, 'alpha')
  status.selectSession('b')
  assert.equal(status.current().server, 'beta')
  status.selectSession('a')
  assert.equal(status.current().server, 'alpha')
})

test('a new initialize clears every old Agent projection and ignores late request responses', () => {
  const config = installAcpSessionConfig(makeCtx())
  const status = installAcpSessionStatus(makeCtx(), { sessionConfig: config })
  const stats = installAcpSessionStats(makeCtx())
  const plan = installAcpSessionPlan(makeCtx())
  const services = [config, status, stats, plan]
  const client = (message) => services.forEach((service) => service.observeClient(message))
  const agent = (message) => services.forEach((service) => service.observeAgent(message))
  client({ id: 'init-a', method: 'initialize' })
  agent({ id: 'init-a', result: { agentInfo: { name: 'old-agent' }, authMethods: [{ id: 'key', name: 'API Key' }] } })
  client({ id: 'new-a', method: 'session/new' })
  agent({ id: 'new-a', result: { sessionId: 'a', configOptions: [{ id: 'model', type: 'select', name: 'Model', category: 'model', currentValue: 'old-model', options: [] }] } })
  client({ id: 'auth-a', method: 'authenticate', params: { methodId: 'key' } })
  client({ id: 'prompt-a', method: 'session/prompt', params: { sessionId: 'a' } })
  client({ id: 'config-a', method: 'session/set_config_option', params: { sessionId: 'a' } })
  agent({ method: 'session/update', params: { sessionId: 'a', update: { sessionUpdate: 'plan', entries: [{ content: 'Old work', priority: 'medium', status: 'pending' }] } } })
  assert.equal(status.current().model, 'old-model')
  assert.equal(stats.current().stats.turns, 1)
  assert.ok(plan.list().length)
  client({ id: 'init-b', method: 'initialize' })
  assert.equal(status.current().model, undefined)
  assert.equal(status.current().server, undefined)
  assert.equal(status.current().auth.method, undefined)
  assert.equal(status.current().session.bound, false)
  assert.equal(stats.current().stats.turns, 0)
  assert.deepEqual(plan.list(), [])
  assert.deepEqual(config.list(), [])
  agent({ id: 'auth-a', result: {} })
  agent({ id: 'config-a', result: { configOptions: [{ id: 'model', currentValue: 'stale' }] } })
  agent({ id: 'prompt-a', error: { code: -32000, message: 'old auth failure' } })
  agent({ id: 'init-b', result: { agentInfo: { name: 'new-agent' }, authMethods: [] } })
  client({ id: 'new-b', method: 'session/new' })
  agent({ id: 'new-b', result: { sessionId: 'b' } })
  assert.equal(status.current().server, 'new-agent')
  assert.equal(status.current().model, undefined)
  assert.equal(status.current().auth.status, undefined)
  assert.deepEqual(config.list(), [])
})

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

test('Failed session setup reports failure and clears session credentials until a successful retry', () => {
  const status = installAcpSessionStatus(makeCtx())
  status.observeClient({ id: 'init', method: 'initialize' })
  status.observeAgent({ id: 'init', result: { agentInfo: { name: 'Cline' }, authMethods: [] } })
  status.observeClient({ id: 'new', method: 'session/new' })
  status.observeAgent({ id: 'new', error: { code: -32603, message: 'missing executable' } })
  assert.equal(status.current().connection, 'failed')
  assert.equal(status.current().error, 'missing executable')
  assert.equal(status.current().session.bound, false)
  assert.equal(status.current().auth.status, undefined)
  status.observeClient({ id: 'retry', method: 'session/new' })
  status.observeAgent({ id: 'retry', result: { sessionId: 'recovered' } })
  assert.equal(status.current().connection, 'attached')
  assert.equal(status.current().error, undefined)
})

test('status reads model and effort by ACP category before legacy option ids', () => {
  const config = installAcpSessionConfig(makeCtx())
  const status = installAcpSessionStatus(makeCtx(), { sessionConfig: config })
  config.observeClient({ id: 'new', method: 'session/new' })
  config.observeAgent({ id: 'new', result: { sessionId: 'current', configOptions: [
    { id: 'effort', currentValue: 'legacy' },
    { id: 'reasoning_effort', category: 'thought_level', currentValue: 'ultra' },
    { id: 'agent_model', category: 'model', currentValue: 'current-model' },
  ] } })
  assert.equal(status.current().model, 'current-model')
  assert.equal(status.current().effort, 'ultra')
})

test('initialize failure is not an attached connection', () => {
  const status = installAcpSessionStatus(makeCtx())
  status.observeClient({ id: 'init', method: 'initialize' })
  status.observeAgent({ id: 'init', error: { code: -32603, message: 'Agent failed to initialize' } })
  assert.equal(status.current().connection, 'failed')
  assert.equal(status.current().server, undefined)
})

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
  assert.deepEqual(current.session, {
    sessionId: undefined,
    bound: false,
    started: false,
  })
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
  assert.equal(service.current().auth.method, undefined, 'advertised methods are not active credentials')
  assert.equal(service.current().auth.status, undefined)

  service.observeClient({ jsonrpc: '2.0', id: 2, method: 'session/new', params: {} })
  service.observeAgent({ jsonrpc: '2.0', id: 2, result: { sessionId: 's-1' } })
  assert.equal(service.current().auth.method, undefined, 'session/new does not report a credential source')
  assert.deepEqual(service.current().session, {
    sessionId: 's-1',
    bound: true,
    started: false,
  })

  service.observeClient({
    jsonrpc: '2.0', id: 'first-prompt', method: 'session/prompt', params: { sessionId: 's-1' },
  })
  assert.equal(service.current().session.started, true)
  service.observeAgent({
    jsonrpc: '2.0', id: 'first-prompt', result: { stopReason: 'end_turn' },
  })

  service.observeClient({
    jsonrpc: '2.0', id: 'load', method: 'session/load', params: { sessionId: 'saved-session' },
  })
  service.observeAgent({ jsonrpc: '2.0', id: 'load', result: {} })
  assert.deepEqual(service.current().session, {
    sessionId: 'saved-session',
    bound: true,
    started: true,
  })

  // Auth failures on current tracked requests change authentication state.
  service.observeClient({ id: 9, method: 'session/set_mode', params: { sessionId: 'saved-session' } })
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

test('authenticate rejection is a failed login with the selected method and agent reason', () => {
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} }, sessionConfig: makeSessionConfig(),
  })
  service.observeClient({ id: 1, method: 'initialize' })
  service.observeAgent({ id: 1, result: { authMethods: [
    { id: 'first', name: 'First method' }, { id: 'oauth-personal', name: 'Google' },
  ] } })
  service.observeClient({ id: 2, method: 'authenticate', params: { methodId: 'oauth-personal' } })
  assert.equal(service.current().auth.method, 'Google')
  assert.equal(service.current().auth.status, 'signing in')
  const reason = 'Onboarding failed: account is not eligible in your location'
  service.observeAgent({ id: 2, error: { code: -32000, message: reason } })
  assert.equal(service.current().auth.status, 'sign-in failed')
  assert.equal(service.current().auth.message, reason)
  service.observeClient({ id: 3, method: 'authenticate', params: { methodId: 'oauth-personal' } })
  assert.equal(service.current().auth.message, undefined, 'retry clears the previous failure')
  service.observeAgent({ id: 3, result: {} })
  assert.equal(service.current().auth.status, 'configured')
  service.observeClient({ id: 4, method: 'authenticate', params: { methodId: 'oauth-personal' } })
  service.observeAgent({ id: 4, error: { code: -32000, message: reason } })
  service.observeClient({ id: 5, method: 'session/new', params: {} })
  service.observeAgent({ id: 5, result: { sessionId: 'ready-session' } })
  assert.equal(service.current().auth.status, 'configured', 'a successful session clears a stale auth failure')
  assert.equal(service.current().auth.message, undefined)
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

test('background Session status and mode facts do not contaminate the selected tab', () => {
  const service = installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig: makeSessionConfig(),
  })
  service.selectSession('s-2')
  service.observeAgent({
    jsonrpc: '2.0', method: 'session.status',
    params: { sessionId: 's-1', status: 'running' },
  })
  service.observeAgent({
    jsonrpc: '2.0', method: 'session.event', params: {
      sessionId: 's-1',
      event: { type: 'permission/preset', data: { preset: 'danger-full-access' } },
    },
  })
  assert.equal(service.current().state, 'idle')
  assert.equal(service.current().permission, undefined)
  assert.equal(service.current().session.sessionId, 's-2')

  service.selectSession('s-1')
  assert.equal(service.current().state, 'running')
  assert.equal(service.current().permission, 'danger-full-access')
})

test('authentication updates only the owning Harness across tabs', () => {
  const status = installAcpSessionStatus(makeCtx())
  status.observeClient({id:'init',method:'initialize'})
  status.observeAgent({id:'init',result:{agentInfo:{name:'alpha'},authMethods:[]}})
  for (const id of ['a','b']) {
    status.observeClient({id,method:'session/new'})
    status.observeAgent({id,result:{sessionId:id,_meta:{marttyConnection:{id,agentInfo:{name:id},authMethods:[{id:`${id}:login`,name:`Login ${id}`}]}}}})
  }
  status.selectSession('a')
  status.observeClient({id:'auth',method:'authenticate',params:{methodId:'b:login'}})
  assert.equal(status.current().auth.status,'configured')
  status.selectSession('b')
  assert.equal(status.current().auth.status,'signing in')
  assert.equal(status.current().auth.method,'Login b')
  status.observeAgent({id:'auth',error:{code:-32000,message:'Beta refused sign in'}})
  assert.equal(status.current().auth.status,'sign-in failed')
  status.selectSession('a')
  assert.equal(status.current().auth.status,'configured')
  status.observeClient({id:'prompt',method:'session/prompt',params:{sessionId:'a'}})
  status.observeAgent({id:'prompt',error:{code:-32000,message:'Alpha expired'}})
  assert.equal(status.current().auth.status,'needs sign-in')
  status.selectSession('b')
  assert.equal(status.current().auth.status,'sign-in failed')
})

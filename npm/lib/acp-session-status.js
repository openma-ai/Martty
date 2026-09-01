/**
 * Standard-ACP-backed run-state projection for the Client tree.
 *
 * Folds the non-statistics facts `/status` needs — connection, server,
 * authenticate state, session binding, model, effort, permission, plan,
 * agent preset, and the running/idle state — from messages the mux already
 * observes. Statistics stay in `acpSessionStats`; this service never
 * counts tokens or timings, so there is exactly one stats source.
 */

import { Service } from '@deepseek-ai/cordis'

export const name = 'acp-session-status'
export const inject = ['acpClientEvents', 'acpSessionConfig']

const AUTH_REQUIRED_CODE = -32000
const SETUP_METHODS = new Set(['session/new', 'session/load'])
const RUNNING_UPDATE_TYPES = new Set([
  'user_message_chunk',
  'agent_message_chunk',
  'agent_thought_chunk',
  'tool_call',
  'tool_call_update',
  'plan',
  'plan_update',
  'plan_removed',
  'usage_update',
])

class AcpSessionStatusService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpSessionStatus')
    this.core = core
  }

  current() { return this.core.current() }
  subscribe(listener) { return this.core.subscribe(this.ctx, listener) }
  observeClient(message) { return this.core.observeClient(message) }
  observeAgent(message) { return this.core.observeAgent(message) }
}

function zero() {
  return {
    state: 'idle',
    connection: 'connecting',
    server: undefined,
    auth: { status: undefined, method: undefined },
    session: { sessionId: undefined, bound: false },
    model: undefined,
    effort: undefined,
    permission: undefined,
    plan: undefined,
    agent: undefined,
  }
}

export function installAcpSessionStatus(ctx, options = {}) {
  const events = options.events ?? ctx.acpClientEvents ?? ctx.get?.('acpClientEvents')
  const sessionConfig = options.sessionConfig
    ?? ctx.acpSessionConfig ?? ctx.get?.('acpSessionConfig')

  const listeners = new Set()
  let value = zero()
  let initializeId
  let pendingAuthenticate = new Set()
  const pendingSetup = new Map()
  const pendingPrompts = new Map()
  let permissionPreset
  let sandboxMode

  function current() { return structuredClone(value) }

  function publish() {
    const snapshot = current()
    for (const listener of [...listeners]) listener(snapshot)
  }

  function subscribe(effectCtx, listener) {
    if (typeof listener !== 'function') {
      throw new Error('acpSessionStatus.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'acpSessionStatus.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observeClient(message) {
    if (!object(message) || typeof message.method !== 'string') return
    if (message.method === 'initialize' && message.id !== undefined) {
      initializeId = message.id
      return
    }
    if (message.method === 'authenticate' && message.id !== undefined) {
      pendingAuthenticate.add(message.id)
      value.auth.status = 'signing in'
      publish()
      return
    }
    if (SETUP_METHODS.has(message.method) && message.id !== undefined) {
      pendingSetup.set(
        message.id,
        message.method === 'session/load'
          ? readString(message.params, 'sessionId', 'session_id')
          : undefined,
      )
      return
    }
    if (message.method === 'session/prompt' && message.id !== undefined) {
      pendingPrompts.set(
        message.id,
        readString(message.params, 'sessionId', 'session_id'),
      )
      if (value.state === 'idle') {
        value.state = 'starting'
        publish()
      }
    }
  }

  function observeAgent(message) {
    if (!object(message)) return
    if (message.id !== undefined && message.id === initializeId) {
      initializeId = undefined
      value.connection = 'attached'
      const result = object(message.result) ? message.result : undefined
      value.server = readString(result?.agentInfo, 'name')
      const methods = Array.isArray(result?.authMethods) ? result.authMethods : []
      const method = methods.find((candidate) => object(candidate)
        && typeof candidate.id === 'string' && candidate.id.length > 0)
      if (method !== undefined && value.auth.status === undefined) {
        value.auth.method = readString(method, 'name', 'label') ?? method.id
      }
      publish()
      return
    }
    if (message.id !== undefined && pendingAuthenticate.has(message.id)) {
      pendingAuthenticate.delete(message.id)
      if (message.error === undefined) {
        value.auth.status = 'configured'
      } else if (isAuthRequired(message.error)) {
        value.auth.status = 'needs sign-in'
      } else {
        value.auth.status = undefined
      }
      publish()
      return
    }
    if (message.id !== undefined && pendingSetup.has(message.id)) {
      const requested = pendingSetup.get(message.id)
      pendingSetup.delete(message.id)
      if (message.error !== undefined) {
        if (isAuthRequired(message.error)) value.auth.status = 'needs sign-in'
        value.session = { sessionId: requested, bound: false }
      } else {
        const sessionId = readString(message.result, 'sessionId', 'session_id') ?? requested
        value.session = { sessionId, bound: sessionId !== undefined }
      }
      publish()
      return
    }
    if (response(message) && pendingPrompts.has(message.id)) {
      pendingPrompts.delete(message.id)
      let changed = false
      if (message.error !== undefined && isAuthRequired(message.error)
        && value.auth.status !== 'needs sign-in') {
        value.auth.status = 'needs sign-in'
        changed = true
      }
      if (pendingPrompts.size === 0 && value.state !== 'idle') {
        value.state = 'idle'
        changed = true
      }
      if (changed) publish()
      return
    }
    if (message.error !== undefined && isAuthRequired(message.error)) {
      value.auth.status = 'needs sign-in'
      publish()
      return
    }

    if (message.method === 'session.status' && object(message.params)) {
      const status = readString(message.params, 'status')
      if (status === 'running' || (status === 'idle' && pendingPrompts.size === 0)) {
        value.state = status
        publish()
      }
      return
    }
    if (message.method === 'session.event' && object(message.params)) {
      const event = object(message.params.event) ? message.params.event : undefined
      const type = readString(event, 'type')
      const data = object(event?.data) ? event.data : undefined
      if (type === 'permission/preset') {
        const preset = readString(data, 'preset')
        if (preset !== undefined) {
          permissionPreset = preset
          value.permission = permissionPreset ?? sandboxMode
          publish()
        }
      } else if (type === 'sandbox/mode') {
        const mode = readString(data, 'mode')
        if (mode !== undefined) {
          sandboxMode = mode
          value.permission = permissionPreset ?? sandboxMode
          publish()
        }
      } else if (type === 'plan/mode' && typeof data?.active === 'boolean') {
        value.plan = data.active
        publish()
      } else if (type === 'agent-preset/selected') {
        const preset = readString(data, 'agentPreset')
        if (preset !== undefined) {
          value.agent = preset
          publish()
        }
      }
      return
    }
    if (message.method !== 'session/update' || !object(message.params)) return
    const sessionId = readString(message.params, 'sessionId', 'session_id')
    const update = object(message.params.update) ? message.params.update : undefined
    const type = readString(update, 'sessionUpdate', 'session_update')
    if (value.state !== 'running' && hasPendingPrompt(sessionId)
      && RUNNING_UPDATE_TYPES.has(type)) {
      value.state = 'running'
      publish()
    }
  }

  function hasPendingPrompt(sessionId) {
    if (sessionId === undefined) return false
    for (const pendingSessionId of pendingPrompts.values()) {
      if (pendingSessionId === sessionId) return true
    }
    return false
  }

  function onConfigSnapshot(snapshot) {
    if (snapshot === null || typeof snapshot !== 'object') return
    if (typeof snapshot.sessionId === 'string' && snapshot.sessionId.length > 0
      && value.session.sessionId === undefined) {
      value.session = { sessionId: snapshot.sessionId, bound: true }
    }
    value.model = optionValue(snapshot.options, 'model') ?? value.model
    value.effort = optionValue(snapshot.options, 'effort') ?? value.effort
    publish()
  }

  function optionValue(options, id) {
    if (!Array.isArray(options)) return undefined
    const option = options.find((candidate) => candidate?.id === id)
    if (option === undefined) return undefined
    const raw = option.currentValue ?? option.current_value
    return typeof raw === 'string' ? raw : undefined
  }

  const core = { current, subscribe, observeClient, observeAgent }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionStatusService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observeClient,
        observeAgent,
      }
  if (typeof ctx.provide !== 'function') ctx.acpSessionStatus = service

  if (typeof sessionConfig?.subscribe === 'function') {
    sessionConfig.subscribe(onConfigSnapshot)
  }
  // Seed model/effort from anything already folded (session/load replay).
  if (typeof sessionConfig?.list === 'function') {
    onConfigSnapshot({ sessionId: undefined, options: sessionConfig.list() })
  }
  if (typeof events?.register === 'function') {
    // register(observer) only: the service scopes the subscription to its
    // own Context, so pass the folding object alone.
    events.register({ observeClient, observeAgent })
  }

  return service
}

function isAuthRequired(error) {
  return object(error) && (error.code === AUTH_REQUIRED_CODE
    || error.code === 'auth_required' || error.code === 'AuthRequired')
}

function readString(value, ...keys) {
  if (!object(value)) return undefined
  for (const key of keys) {
    if (typeof value[key] === 'string' && value[key].length > 0) return value[key]
  }
  return undefined
}

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function response(message) {
  return !Object.hasOwn(message, 'method')
    && (Object.hasOwn(message, 'result') || Object.hasOwn(message, 'error'))
}

export function apply(ctx, options = {}) {
  installAcpSessionStatus(ctx, options)
}

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
const SETUP_METHODS = new Set(['session/new', 'session/load', 'session/resume'])
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
  selectSession(sessionId) { return this.core.selectSession(sessionId) }
}

function zeroSession(sessionId, bound = sessionId !== undefined) {
  return {
    state: 'idle',
    session: { sessionId, bound },
    model: undefined,
    effort: undefined,
    permission: undefined,
    plan: undefined,
    agent: undefined,
    permissionPreset: undefined,
    sandboxMode: undefined,
  }
}

export function installAcpSessionStatus(ctx, options = {}) {
  const events = options.events ?? ctx.acpClientEvents ?? ctx.get?.('acpClientEvents')
  const sessionConfig = options.sessionConfig
    ?? ctx.acpSessionConfig ?? ctx.get?.('acpSessionConfig')

  const listeners = new Set()
  const sessions = new Map()
  const fallback = zeroSession(undefined, false)
  let sessionId
  let selectionKnown = false
  let connection = 'connecting'
  let server
  const auth = { status: undefined, method: undefined }
  let initializeId
  const pendingAuthenticate = new Set()
  const pendingSetup = new Map()
  const pendingPrompts = new Map()

  function stateFor(targetSessionId, bound = targetSessionId !== undefined) {
    if (typeof targetSessionId !== 'string' || targetSessionId.length === 0) {
      return fallback
    }
    let value = sessions.get(targetSessionId)
    if (value === undefined) {
      value = zeroSession(targetSessionId, bound)
      sessions.set(targetSessionId, value)
    } else if (bound) {
      value.session.bound = true
    }
    return value
  }

  function current() {
    const value = stateFor(sessionId)
    const { permissionPreset: _permissionPreset, sandboxMode: _sandboxMode, ...visible } = value
    return structuredClone({ connection, server, auth, ...visible })
  }

  function publish() {
    const snapshot = current()
    for (const listener of [...listeners]) listener(snapshot)
  }

  function publishIf(targetSessionId) {
    if (targetSessionId === sessionId) publish()
  }

  function selectSession(nextSessionId) {
    selectionKnown = true
    sessionId = typeof nextSessionId === 'string' && nextSessionId.length > 0
      ? nextSessionId
      : undefined
    publish()
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
      auth.status = 'signing in'
      publish()
      return
    }
    if (SETUP_METHODS.has(message.method) && message.id !== undefined) {
      pendingSetup.set(
        message.id,
        message.method !== 'session/new'
          ? readString(message.params, 'sessionId', 'session_id')
          : undefined,
      )
      return
    }
    if (message.method === 'session/prompt' && message.id !== undefined) {
      const promptSessionId = readString(message.params, 'sessionId', 'session_id')
      if (promptSessionId === undefined) return
      if (!selectionKnown && sessionId === undefined) sessionId = promptSessionId
      pendingPrompts.set(message.id, promptSessionId)
      const value = stateFor(promptSessionId)
      if (value.state === 'idle') {
        value.state = 'starting'
        publishIf(promptSessionId)
      }
    }
  }

  function observeAgent(message) {
    if (!object(message)) return
    if (message.id !== undefined && message.id === initializeId) {
      initializeId = undefined
      connection = 'attached'
      const result = object(message.result) ? message.result : undefined
      server = readString(result?.agentInfo, 'name')
      const methods = Array.isArray(result?.authMethods) ? result.authMethods : []
      const method = methods.find((candidate) => object(candidate)
        && typeof candidate.id === 'string' && candidate.id.length > 0)
      if (method !== undefined && auth.status === undefined) {
        auth.method = readString(method, 'name', 'label') ?? method.id
      }
      publish()
      return
    }
    if (message.id !== undefined && pendingAuthenticate.has(message.id)) {
      pendingAuthenticate.delete(message.id)
      if (message.error === undefined) {
        auth.status = 'configured'
      } else if (isAuthRequired(message.error)) {
        auth.status = 'needs sign-in'
      } else {
        auth.status = undefined
      }
      publish()
      return
    }
    if (message.id !== undefined && pendingSetup.has(message.id)) {
      const requested = pendingSetup.get(message.id)
      pendingSetup.delete(message.id)
      if (message.error !== undefined) {
        if (isAuthRequired(message.error)) auth.status = 'needs sign-in'
        if (requested !== undefined) {
          stateFor(requested, false).session.bound = false
        }
        if (!selectionKnown) sessionId = requested
      } else {
        const bound = readString(message.result, 'sessionId', 'session_id') ?? requested
        if (bound !== undefined) stateFor(bound).session = { sessionId: bound, bound: true }
        if (!selectionKnown) sessionId = bound
      }
      publish()
      return
    }
    if (response(message) && pendingPrompts.has(message.id)) {
      const promptSessionId = pendingPrompts.get(message.id)
      pendingPrompts.delete(message.id)
      let changed = false
      if (message.error !== undefined && isAuthRequired(message.error)
        && auth.status !== 'needs sign-in') {
        auth.status = 'needs sign-in'
        changed = true
      }
      const value = stateFor(promptSessionId)
      if (!hasPendingPrompt(promptSessionId) && value.state !== 'idle') {
        value.state = 'idle'
        changed = true
      }
      if (changed && (promptSessionId === sessionId || auth.status === 'needs sign-in')) publish()
      return
    }
    if (message.error !== undefined && isAuthRequired(message.error)) {
      auth.status = 'needs sign-in'
      publish()
      return
    }

    if (message.method === 'session.status' && object(message.params)) {
      const statusSessionId = readString(message.params, 'sessionId', 'session_id') ?? sessionId
      if (statusSessionId === undefined) return
      if (!selectionKnown && sessionId === undefined) sessionId = statusSessionId
      const status = readString(message.params, 'status')
      const value = stateFor(statusSessionId)
      if (status === 'running' || (status === 'idle' && !hasPendingPrompt(statusSessionId))) {
        value.state = status
        publishIf(statusSessionId)
      }
      return
    }
    if (message.method === 'session.event' && object(message.params)) {
      const eventSessionId = readString(message.params, 'sessionId', 'session_id') ?? sessionId
      if (eventSessionId === undefined) return
      if (!selectionKnown && sessionId === undefined) sessionId = eventSessionId
      const value = stateFor(eventSessionId)
      const event = object(message.params.event) ? message.params.event : undefined
      const type = readString(event, 'type')
      const data = object(event?.data) ? event.data : undefined
      if (type === 'permission/preset') {
        const preset = readString(data, 'preset')
        if (preset !== undefined) {
          value.permissionPreset = preset
          value.permission = value.permissionPreset ?? value.sandboxMode
          publishIf(eventSessionId)
        }
      } else if (type === 'sandbox/mode') {
        const mode = readString(data, 'mode')
        if (mode !== undefined) {
          value.sandboxMode = mode
          value.permission = value.permissionPreset ?? value.sandboxMode
          publishIf(eventSessionId)
        }
      } else if (type === 'plan/mode' && typeof data?.active === 'boolean') {
        value.plan = data.active
        publishIf(eventSessionId)
      } else if (type === 'agent-preset/selected') {
        const preset = readString(data, 'agentPreset')
        if (preset !== undefined) {
          value.agent = preset
          publishIf(eventSessionId)
        }
      }
      return
    }
    if (message.method !== 'session/update' || !object(message.params)) return
    const updateSessionId = readString(message.params, 'sessionId', 'session_id')
    if (updateSessionId === undefined) return
    if (!selectionKnown && sessionId === undefined) sessionId = updateSessionId
    const update = object(message.params.update) ? message.params.update : undefined
    const type = readString(update, 'sessionUpdate', 'session_update')
    const value = stateFor(updateSessionId)
    if (value.state !== 'running' && hasPendingPrompt(updateSessionId)
      && RUNNING_UPDATE_TYPES.has(type)) {
      value.state = 'running'
      publishIf(updateSessionId)
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
    const configSessionId = typeof snapshot.sessionId === 'string' && snapshot.sessionId.length > 0
      ? snapshot.sessionId
      : sessionId
    const value = stateFor(configSessionId)
    value.model = optionValue(snapshot.options, 'model') ?? value.model
    value.effort = optionValue(snapshot.options, 'effort') ?? value.effort
    publishIf(configSessionId)
  }

  function optionValue(options, id) {
    if (!Array.isArray(options)) return undefined
    const option = options.find((candidate) => candidate?.id === id)
    if (option === undefined) return undefined
    const raw = option.currentValue ?? option.current_value
    return typeof raw === 'string' ? raw : undefined
  }

  const core = { current, subscribe, observeClient, observeAgent, selectSession }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionStatusService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observeClient,
        observeAgent,
        selectSession,
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
    events.register({ observeClient, observeAgent, selectSession })
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

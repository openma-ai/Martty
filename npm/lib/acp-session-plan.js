/** Live structured Plan state folded from standard ACP Session updates. */

import { Service } from '@deepseek-ai/cordis'

export const name = 'acp-session-plan'
export const inject = []

const SETUP_METHODS = new Set(['session/new', 'session/load', 'session/resume'])

class AcpSessionPlanService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpSessionPlan')
    this.core = core
  }

  list() {
    return this.core.list()
  }

  current() {
    return this.core.current()
  }

  subscribe(listener) {
    return this.core.subscribe(this.ctx, listener)
  }

  observeClient(message) {
    return this.core.observeClient(message)
  }

  observeAgent(message) {
    return this.core.observeAgent(message)
  }

  selectSession(sessionId) {
    return this.core.selectSession(sessionId)
  }
}

/** Install `ctx.acpSessionPlan`, backed only by standard ACP traffic. */
export function installAcpSessionPlan(ctx) {
  let sessionId
  let selectionKnown = false
  const plansBySession = new Map()
  const pending = new Map()
  const listeners = new Set()

  function activePlans() {
    return plansBySession.get(sessionId) ?? new Map()
  }

  function list() {
    return cloneJson([...activePlans().values()])
  }

  function current() {
    return cloneJson([...activePlans().values()].at(-1) ?? null)
  }

  function snapshot() {
    return { sessionId, plans: list() }
  }

  function publish() {
    const next = snapshot()
    for (const listener of [...listeners]) listener(next)
  }

  function reset(targetSessionId) {
    if (typeof targetSessionId !== 'string' || targetSessionId.length === 0) return
    plansBySession.set(targetSessionId, new Map())
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
      throw new Error('acpSessionPlan.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'acpSessionPlan.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observeClient(message) {
    if (!isObject(message) || message.id === undefined || !SETUP_METHODS.has(message.method)) return
    pending.set(message.id, {
      sessionId: message.method !== 'session/new'
        ? readString(message.params, 'sessionId', 'session_id')
        : undefined,
    })
  }

  function observeAgent(message) {
    if (!isObject(message)) return
    if (message.id !== undefined && pending.has(message.id)) {
      const tracked = pending.get(message.id)
      pending.delete(message.id)
      if (message.error !== undefined || !isObject(message.result)) return
      const bound = readString(message.result, 'sessionId', 'session_id') ?? tracked.sessionId
      if (!selectionKnown) sessionId = bound
      reset(bound)
      return
    }
    if (message.method !== 'session/update' || !isObject(message.params)) return
    const updatedSession = readString(message.params, 'sessionId', 'session_id')
    if (updatedSession === undefined) return
    if (!selectionKnown && sessionId === undefined) sessionId = updatedSession
    let plans = plansBySession.get(updatedSession)
    if (plans === undefined) {
      plans = new Map()
      plansBySession.set(updatedSession, plans)
    }
    const update = message.params.update
    if (!isObject(update)) return
    const type = readString(update, 'sessionUpdate', 'session_update')
    if (type === 'plan_removed') {
      const id = readString(update, 'planId', 'plan_id')
      if (id === undefined) plans.clear()
      else plans.delete(id)
      if (updatedSession === sessionId) publish()
      return
    }
    if (type !== 'plan' && type !== 'plan_update') return
    const plan = normalizePlan(update)
    if (plan === null) return
    if (plan.kind === 'items' && plan.entries.length === 0) {
      plans.delete(plan.id)
    } else {
      plans.delete(plan.id)
      plans.set(plan.id, plan)
    }
    if (updatedSession === sessionId) publish()
  }

  const core = { list, current, subscribe, observeClient, observeAgent, selectSession }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionPlanService(ctx, core)
    : {
        list,
        current,
        subscribe(listener) {
          return subscribe(ctx, listener)
        },
        observeClient,
        observeAgent,
        selectSession,
      }
  if (typeof ctx.provide !== 'function') ctx.acpSessionPlan = service
  return service
}

function normalizePlan(update) {
  const nested = isObject(update.plan) ? update.plan : update
  const id = readString(nested, 'planId', 'plan_id')
    ?? readString(update, 'planId', 'plan_id')
    ?? 'default'
  const declaredKind = readString(nested, 'type', 'kind')
  if (declaredKind === 'items' || Array.isArray(nested.entries)) {
    const entries = Array.isArray(nested.entries)
      ? nested.entries.map(normalizeEntry).filter((entry) => entry !== null)
      : []
    return { id, kind: 'items', entries }
  }
  if (declaredKind === 'markdown' || typeof nested.content === 'string') {
    const content = typeof nested.content === 'string' ? nested.content : ''
    return { id, kind: 'markdown', content }
  }
  if (declaredKind === 'file' || typeof nested.uri === 'string') {
    const uri = typeof nested.uri === 'string' ? nested.uri : ''
    return { id, kind: 'file', uri }
  }
  return null
}

function normalizeEntry(value) {
  if (!isObject(value) || typeof value.content !== 'string') return null
  return {
    content: value.content,
    ...(typeof value.priority === 'string' ? { priority: value.priority } : {}),
    ...(typeof value.status === 'string' ? { status: value.status } : {}),
  }
}

function readString(value, ...keys) {
  if (!isObject(value)) return undefined
  for (const key of keys) {
    if (typeof value[key] === 'string' && value[key].length > 0) return value[key]
  }
  return undefined
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function cloneJson(value) {
  return value === undefined ? undefined : structuredClone(value)
}

export function apply(ctx) {
  installAcpSessionPlan(ctx)
}

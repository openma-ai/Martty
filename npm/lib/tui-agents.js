/** Native-painter Agent navigation projected into the Cordis Client tree. */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-agents'
export const inject = []

class TuiAgentsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiAgents')
    this.core = core
  }

  current() { return this.core.current() }
  subscribe(listener) { return this.core.subscribe(this.ctx, listener) }
  observe(snapshot) { return this.core.observe(snapshot) }
  select(id) { return this.core.select(id) }
  navigate(action) { return this.core.navigate(action) }
  bindNotify(notify) { return this.core.bindNotify(notify) }
}

const EMPTY = Object.freeze({ activeId: null, selectedId: null, items: [] })
const ITEM_KINDS = new Set(['main', 'subagent', 'history'])
const ITEM_STATUSES = new Set(['idle', 'running', 'finished', 'failed'])
const NAVIGATION_ACTIONS = new Set(['begin', 'previous', 'next', 'confirm', 'cancel'])

export function installTuiAgents(ctx, options = {}) {
  let value = EMPTY
  let send = typeof options.notify === 'function' ? options.notify : undefined
  const listeners = new Set()

  function current() {
    return structuredClone(value)
  }

  function subscribe(effectCtx, listener) {
    if (typeof listener !== 'function') {
      throw new Error('tuiAgents.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'tuiAgents.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observe(snapshot) {
    if (!object(snapshot) || snapshot.protocol !== 0 || !Array.isArray(snapshot.items)) return false
    const items = snapshot.items.map(normalizeItem).filter((item) => item !== null)
    const activeId = idOrNull(snapshot.activeId)
    const selectedId = idOrNull(snapshot.selectedId)
    value = {
      activeId: items.some((item) => sameId(item.id, activeId)) ? activeId : null,
      selectedId: items.some((item) => sameId(item.id, selectedId)) ? selectedId : null,
      items,
    }
    const next = current()
    for (const listener of [...listeners]) listener(next)
    return true
  }

  function bindNotify(notify) {
    if (typeof notify !== 'function') throw new Error('tuiAgents.bindNotify: notify must be a function')
    send = notify
  }

  function select(id) {
    const selected = idOrNull(id)
    if (selected === null || !value.items.some((item) => sameId(item.id, selected))) return false
    if (typeof send !== 'function') return false
    send(CORDIS_METHODS.agentsSelect, { protocol: 0, id: selected })
    return true
  }

  function navigate(action) {
    if (!NAVIGATION_ACTIONS.has(action) || typeof send !== 'function') return false
    send(CORDIS_METHODS.agentsNavigate, { protocol: 0, action })
    return true
  }

  const core = { current, subscribe, observe, select, navigate, bindNotify }
  const service = typeof ctx.provide === 'function'
    ? new TuiAgentsService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observe,
        select,
        navigate,
        bindNotify,
      }
  if (typeof ctx.provide !== 'function') ctx.tuiAgents = service
  return service
}

function normalizeItem(item) {
  if (!object(item)) return null
  const id = idOrNull(item.id)
  if (id === null || typeof item.label !== 'string' || item.label.length === 0
    || !ITEM_KINDS.has(item.kind) || !ITEM_STATUSES.has(item.status)) return null
  return {
    id, label: item.label, kind: item.kind, status: item.status,
    ...(typeof item.current === 'boolean' ? { current: item.current } : {}),
  }
}

function idOrNull(value) {
  return (typeof value === 'string' && value.length > 0)
    || (Number.isSafeInteger(value) && value >= 0)
    ? value
    : null
}

function sameId(left, right) {
  return right !== null && String(left) === String(right)
}

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

export function apply(ctx) {
  installTuiAgents(ctx)
}

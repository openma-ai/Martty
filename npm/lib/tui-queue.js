/** Native-painter Queue state projected into the Cordis Client tree. */

import { Service } from '@deepseek-ai/cordis'

export const name = 'tui-queue'
export const inject = []

class TuiQueueService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiQueue')
    this.core = core
  }

  current() { return this.core.current() }
  subscribe(listener) { return this.core.subscribe(this.ctx, listener) }
  observe(snapshot) { return this.core.observe(snapshot) }
}

const EMPTY = Object.freeze({
  count: 0,
  items: [],
  selectedId: null,
  editingId: null,
  deleteConfirm: false,
})

export function installTuiQueue(ctx) {
  let value = EMPTY
  const listeners = new Set()

  function current() {
    return structuredClone(value)
  }

  function subscribe(effectCtx, listener) {
    if (typeof listener !== 'function') {
      throw new Error('tuiQueue.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'tuiQueue.subscribe')
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
    value = {
      count: Number.isInteger(snapshot.count) && snapshot.count >= items.length
        ? snapshot.count
        : items.length,
      items,
      selectedId: idOrNull(snapshot.selectedId),
      editingId: idOrNull(snapshot.editingId),
      deleteConfirm: snapshot.deleteConfirm === true,
    }
    const next = current()
    for (const listener of [...listeners]) listener(next)
    return true
  }

  const core = { current, subscribe, observe }
  const service = typeof ctx.provide === 'function'
    ? new TuiQueueService(ctx, core)
    : {
        current,
        subscribe(listener) { return subscribe(ctx, listener) },
        observe,
      }
  if (typeof ctx.provide !== 'function') ctx.tuiQueue = service
  return service
}

function normalizeItem(item) {
  if (!object(item) || idOrNull(item.id) === null
    || !Number.isInteger(item.ordinal) || item.ordinal < 1
    || typeof item.summary !== 'string') return null
  return { id: item.id, ordinal: item.ordinal, summary: item.summary }
}

function idOrNull(value) {
  return (typeof value === 'string' && value.length > 0)
    || (Number.isSafeInteger(value) && value >= 0)
    ? value
    : null
}

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

export function apply(ctx) {
  installTuiQueue(ctx)
}

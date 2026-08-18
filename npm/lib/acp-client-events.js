/** Effect-scoped fan-out for standard ACP messages observed by Client plugins. */

import { Service } from '@deepseek-ai/cordis'

export const name = 'acp-client-events'
export const inject = []

class AcpClientEventsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpClientEvents')
    this.core = core
  }

  register(observer) { return this.core.register(this.ctx, observer) }
  observeClient(message) { return this.core.observeClient(message) }
  observeAgent(message) { return this.core.observeAgent(message) }
}

export function installAcpClientEvents(ctx) {
  const observers = new Set()

  function register(effectCtx, observer) {
    if (observer === null || typeof observer !== 'object'
      || (typeof observer.observeClient !== 'function'
        && typeof observer.observeAgent !== 'function')) {
      throw new Error('acpClientEvents.register: observeClient or observeAgent is required')
    }
    const setup = () => {
      observers.add(observer)
      return () => observers.delete(observer)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'acpClientEvents.register')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observeClient(message) {
    for (const observer of [...observers]) observer.observeClient?.(message)
  }

  function observeAgent(message) {
    for (const observer of [...observers]) observer.observeAgent?.(message)
  }

  const core = { register, observeClient, observeAgent }
  const service = typeof ctx.provide === 'function'
    ? new AcpClientEventsService(ctx, core)
    : {
        register(observer) { return register(ctx, observer) },
        observeClient,
        observeAgent,
      }
  if (typeof ctx.provide !== 'function') ctx.acpClientEvents = service
  return service
}

export function apply(ctx) {
  installAcpClientEvents(ctx)
}

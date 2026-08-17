/**
 * Live standard ACP Session configuration on the Client tree.
 *
 * The mux observes the existing ACP setup/update traffic. Mutations are sent
 * to the native ACP client, which remains the sole owner of
 * `session/set_config_option` and therefore also folds response-only state.
 */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS, CORDIS_PROTOCOL } from './cordis-protocol.js'

export const name = 'acp-session-config'
export const inject = []

const SETUP_METHODS = new Set(['session/new', 'session/load'])

class AcpSessionConfigService extends Service {
  constructor(ctx, core) {
    super(ctx, 'acpSessionConfig')
    this.core = core
  }

  list() {
    return this.core.list()
  }

  current(id) {
    return this.core.current(id)
  }

  byCategory(category) {
    return this.core.byCategory(category)
  }

  transaction(selector) {
    return this.core.transaction(selector)
  }

  set(id, value) {
    return this.core.set(id, value)
  }

  subscribe(listener) {
    return this.core.subscribe(this.ctx, listener)
  }

  bindTransport(requestTui) {
    return this.core.bindTransport(requestTui)
  }

  observeClient(message) {
    return this.core.observeClient(message)
  }

  observeAgent(message) {
    return this.core.observeAgent(message)
  }
}

/** Install the `ctx.acpSessionConfig` standard ACP state service. */
export function installAcpSessionConfig(ctx) {
  let sessionId
  let options = []
  let requestTui
  const pending = new Map()
  const listeners = new Set()

  function list() {
    return cloneJson(options)
  }

  function current(id) {
    if (typeof id !== 'string' || id.length === 0) {
      throw new Error('acpSessionConfig.current: id must be a non-empty string')
    }
    const option = options.find((candidate) => candidate?.id === id)
    return cloneJson(option?.currentValue ?? option?.current_value)
  }

  function byCategory(category) {
    if (typeof category !== 'string' || category.length === 0) {
      throw new Error('acpSessionConfig.byCategory: category must be a non-empty string')
    }
    return cloneJson(options.filter((candidate) => candidate?.category === category))
  }

  function snapshot() {
    return { sessionId, options: list() }
  }

  function publish(nextSessionId, nextOptions) {
    if (typeof nextSessionId === 'string' && nextSessionId.length > 0) {
      sessionId = nextSessionId
    }
    if (!Array.isArray(nextOptions)) return
    options = cloneJson(nextOptions)
    const next = snapshot()
    for (const listener of [...listeners]) listener(next)
  }

  function subscribe(effectCtx, listener) {
    if (typeof listener !== 'function') {
      throw new Error('acpSessionConfig.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof effectCtx?.effect === 'function'
      ? effectCtx.effect(setup, 'acpSessionConfig.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function observeClient(message) {
    if (!isObject(message) || message.id === undefined || typeof message.method !== 'string') return
    if (SETUP_METHODS.has(message.method)) {
      const requested = message.method === 'session/load'
        ? readString(message.params, 'sessionId', 'session_id')
        : undefined
      pending.set(message.id, { kind: 'setup', sessionId: requested })
      return
    }
    if (message.method === 'session/set_config_option') {
      pending.set(message.id, {
        kind: 'config',
        sessionId: readString(message.params, 'sessionId', 'session_id') ?? sessionId,
      })
    }
  }

  function observeAgent(message) {
    if (!isObject(message)) return
    if (message.id !== undefined && pending.has(message.id)) {
      const tracked = pending.get(message.id)
      pending.delete(message.id)
      if (message.error !== undefined || !isObject(message.result)) return
      const bound = readString(message.result, 'sessionId', 'session_id') ?? tracked.sessionId
      const next = readOptions(message.result)
      if (tracked.kind === 'setup') {
        sessionId = bound
        publish(bound, next ?? [])
      } else if (bound === undefined || bound === sessionId) {
        publish(bound, next)
      }
      return
    }
    if (message.method !== 'session/update' || !isObject(message.params)) return
    const updatedSession = readString(message.params, 'sessionId', 'session_id')
    if (sessionId !== undefined && updatedSession !== sessionId) return
    const update = message.params.update
    if (!isObject(update)) return
    const type = readString(update, 'sessionUpdate', 'session_update')
    if (type !== 'config_option_update') return
    publish(updatedSession, readOptions(update))
  }

  function bindTransport(nextRequestTui) {
    if (typeof nextRequestTui !== 'function') {
      throw new Error('acpSessionConfig.bindTransport: requestTui must be a function')
    }
    requestTui = nextRequestTui
    return () => {
      if (requestTui === nextRequestTui) requestTui = undefined
    }
  }

  async function set(id, value) {
    if (typeof id !== 'string' || id.length === 0) {
      throw new Error('acpSessionConfig.set: id must be a non-empty string')
    }
    if (sessionId === undefined) {
      throw new Error('acpSessionConfig.set: no ACP Session is active')
    }
    if (typeof requestTui !== 'function') {
      throw new Error('acpSessionConfig.set: native ACP transport is unavailable')
    }
    const option = options.find((candidate) => candidate?.id === id)
    if (option === undefined) {
      throw new Error(`acpSessionConfig.set: option "${id}" is not advertised by the current Session`)
    }
    validateValue(option, value)
    const result = await requestTui(CORDIS_METHODS.sessionConfigSet, {
      protocol: CORDIS_PROTOCOL,
      sessionId,
      configId: id,
      value,
    })
    if (!isObject(result)) {
      throw new Error('acpSessionConfig.set: native ACP client returned an invalid response')
    }
    const resultSession = readString(result, 'sessionId', 'session_id') ?? sessionId
    publish(resultSession, readOptions(result))
    return list()
  }

  function transaction(selector) {
    const option = resolveTransactionOption(selector, options)
    const original = cloneJson(option.currentValue ?? option.current_value)
    if (original === undefined) {
      throw new Error(
        `acpSessionConfig.transaction: option "${option.id}" has no current value to restore`,
      )
    }

    let desired = cloneJson(original)
    let applied = cloneJson(original)
    let chain = Promise.resolve(list())
    let finalizing
    let settled = false

    const write = async (value) => {
      const result = await set(option.id, value)
      applied = cloneJson(value)
      return result
    }

    const preview = (value) => {
      if (settled) return Promise.resolve(list())
      if (finalizing) return finalizing
      if (Object.is(value, desired)) return chain
      desired = cloneJson(value)
      const next = cloneJson(value)
      const task = () => write(next)
      chain = chain.then(task, task)
      return chain
    }

    const finish = (value) => {
      if (settled) return finalizing ?? Promise.resolve(list())
      if (finalizing) return finalizing
      desired = cloneJson(value)
      const settle = async () => {
        if (!Object.is(applied, desired)) await write(desired)
        settled = true
        return list()
      }
      finalizing = chain.then(settle, settle).catch((error) => {
        finalizing = undefined
        throw error
      })
      return finalizing
    }

    return {
      option: cloneJson(option),
      original: cloneJson(original),
      value() {
        return cloneJson(desired)
      },
      preview,
      commit(value = desired) {
        return finish(value)
      },
      rollback() {
        return finish(original)
      },
    }
  }

  const core = {
    list,
    current,
    byCategory,
    transaction,
    set,
    subscribe,
    bindTransport,
    observeClient,
    observeAgent,
  }
  const service = typeof ctx.provide === 'function'
    ? new AcpSessionConfigService(ctx, core)
    : {
        list,
        current,
        byCategory,
        transaction,
        set,
        subscribe(listener) {
          return subscribe(ctx, listener)
        },
        bindTransport,
        observeClient,
        observeAgent,
      }
  if (typeof ctx.provide !== 'function') ctx.acpSessionConfig = service
  return service
}

function resolveTransactionOption(selector, options) {
  if (!isObject(selector)) {
    throw new Error('acpSessionConfig.transaction: selector must be an object')
  }
  const hasCategory = typeof selector.category === 'string' && selector.category.length > 0
  const hasId = typeof selector.id === 'string' && selector.id.length > 0
  if (hasCategory === hasId) {
    throw new Error(
      'acpSessionConfig.transaction: selector must contain exactly one non-empty category or id',
    )
  }
  const matches = hasCategory
    ? options.filter((candidate) => candidate?.category === selector.category)
    : options.filter((candidate) => candidate?.id === selector.id)
  if (matches.length !== 1) {
    const key = hasCategory ? `category "${selector.category}"` : `id "${selector.id}"`
    throw new Error(
      `acpSessionConfig.transaction: expected exactly one option for ${key}, found ${matches.length}`,
    )
  }
  return cloneJson(matches[0])
}

function validateValue(option, value) {
  const kind = option.type
  const current = option.currentValue ?? option.current_value
  if (kind === 'boolean' || typeof current === 'boolean') {
    if (typeof value !== 'boolean') {
      throw new Error(`acpSessionConfig.set: option "${option.id}" requires a boolean value`)
    }
    return
  }
  if (typeof value !== 'string') {
    throw new Error(`acpSessionConfig.set: option "${option.id}" requires a string value id`)
  }
  if (kind === 'select' && Array.isArray(option.options)) {
    const values = selectValues(option.options)
    if (!values.has(value)) {
      throw new Error(
        `acpSessionConfig.set: value "${value}" is not advertised for option "${option.id}"`,
      )
    }
  }
}

function selectValues(entries, out = new Set()) {
  for (const entry of entries) {
    if (!isObject(entry)) continue
    const value = entry.value ?? entry.id
    if (typeof value === 'string') out.add(value)
    if (Array.isArray(entry.options)) selectValues(entry.options, out)
  }
  return out
}

function readOptions(value) {
  if (!isObject(value)) return undefined
  const found = value.configOptions ?? value.config_options
  return Array.isArray(found) ? found : undefined
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
  if (value === undefined) return undefined
  return structuredClone(value)
}

export function apply(ctx) {
  installAcpSessionConfig(ctx)
}

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

const SETUP_METHODS = new Set(['session/new', 'session/load', 'session/resume'])

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

  selectSession(sessionId) {
    return this.core.selectSession(sessionId)
  }
}

/** Install the `ctx.acpSessionConfig` standard ACP state service. */
export function installAcpSessionConfig(ctx) {
  let sessionId
  let selectionKnown = false
  const optionsBySession = new Map()
  let requestTui
  const pending = new Map()
  const listeners = new Set()

  function listFor(targetSessionId) {
    return cloneJson(optionsBySession.get(targetSessionId) ?? [])
  }

  function list() {
    return listFor(sessionId)
  }

  function current(id) {
    if (typeof id !== 'string' || id.length === 0) {
      throw new Error('acpSessionConfig.current: id must be a non-empty string')
    }
    const option = (optionsBySession.get(sessionId) ?? [])
      .find((candidate) => candidate?.id === id)
    return cloneJson(option?.currentValue ?? option?.current_value)
  }

  function byCategory(category) {
    if (typeof category !== 'string' || category.length === 0) {
      throw new Error('acpSessionConfig.byCategory: category must be a non-empty string')
    }
    return cloneJson(
      (optionsBySession.get(sessionId) ?? [])
        .filter((candidate) => candidate?.category === category),
    )
  }

  function snapshot() {
    return { sessionId, options: list() }
  }

  function publish() {
    const next = snapshot()
    for (const listener of [...listeners]) listener(next)
  }

  function updateSession(targetSessionId, nextOptions) {
    if (typeof targetSessionId !== 'string' || targetSessionId.length === 0
      || !Array.isArray(nextOptions)) return
    optionsBySession.set(targetSessionId, cloneJson(nextOptions))
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
      const requested = message.method !== 'session/new'
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
        if (!selectionKnown) sessionId = bound
        updateSession(bound, next ?? [])
      } else {
        updateSession(bound, next)
      }
      return
    }
    if (message.method !== 'session/update' || !isObject(message.params)) return
    const updatedSession = readString(message.params, 'sessionId', 'session_id')
    const update = message.params.update
    if (!isObject(update)) return
    const type = readString(update, 'sessionUpdate', 'session_update')
    if (type !== 'config_option_update') return
    if (!selectionKnown && sessionId === undefined) sessionId = updatedSession
    updateSession(updatedSession, readOptions(update))
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

  async function setForSession(targetSessionId, id, value) {
    if (typeof id !== 'string' || id.length === 0) {
      throw new Error('acpSessionConfig.set: id must be a non-empty string')
    }
    if (targetSessionId === undefined) {
      throw new Error('acpSessionConfig.set: no ACP Session is active')
    }
    if (typeof requestTui !== 'function') {
      throw new Error('acpSessionConfig.set: native ACP transport is unavailable')
    }
    const option = (optionsBySession.get(targetSessionId) ?? [])
      .find((candidate) => candidate?.id === id)
    if (option === undefined) {
      throw new Error(`acpSessionConfig.set: option "${id}" is not advertised by the current Session`)
    }
    validateValue(option, value)
    const result = await requestTui(CORDIS_METHODS.sessionConfigSet, {
      protocol: CORDIS_PROTOCOL,
      sessionId: targetSessionId,
      configId: id,
      value,
    })
    if (!isObject(result)) {
      throw new Error('acpSessionConfig.set: native ACP client returned an invalid response')
    }
    const resultSession = readString(result, 'sessionId', 'session_id') ?? targetSessionId
    updateSession(resultSession, readOptions(result))
    return listFor(targetSessionId)
  }

  async function set(id, value) {
    return setForSession(sessionId, id, value)
  }

  function transaction(selector) {
    const transactionSessionId = sessionId
    if (transactionSessionId === undefined) {
      throw new Error('acpSessionConfig.transaction: no ACP Session is active')
    }
    const transactionOptions = optionsBySession.get(transactionSessionId) ?? []
    const option = resolveTransactionOption(selector, transactionOptions)
    const original = cloneJson(option.currentValue ?? option.current_value)
    if (original === undefined) {
      throw new Error(
        `acpSessionConfig.transaction: option "${option.id}" has no current value to restore`,
      )
    }

    let desired = cloneJson(original)
    let applied = cloneJson(original)
    let chain = Promise.resolve(listFor(transactionSessionId))
    let finalizing
    let settled = false

    const write = async (value) => {
      const result = await setForSession(transactionSessionId, option.id, value)
      applied = cloneJson(value)
      return result
    }

    const preview = (value) => {
      if (settled) return Promise.resolve(listFor(transactionSessionId))
      if (finalizing) return finalizing
      if (Object.is(value, desired)) return chain
      desired = cloneJson(value)
      const next = cloneJson(value)
      const task = () => write(next)
      chain = chain.then(task, task)
      return chain
    }

    const finish = (value) => {
      if (settled) return finalizing ?? Promise.resolve(listFor(transactionSessionId))
      if (finalizing) return finalizing
      desired = cloneJson(value)
      const settle = async () => {
        if (!Object.is(applied, desired)) await write(desired)
        settled = true
        return listFor(transactionSessionId)
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
    selectSession,
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
        selectSession,
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

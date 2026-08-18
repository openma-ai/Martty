/** Lifecycle-owned local slash commands for TUI Client Plugins. */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-commands'
export const inject = []

const PROTOCOL = 0
const NAME = /^[a-z0-9][a-z0-9-]*$/

class TuiCommandsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiCommands')
    this.core = core
  }

  register(options, handler) {
    return this.core.register(this.ctx, options, handler)
  }

  dispatch(params) {
    return this.core.dispatch(params)
  }

  list() {
    return this.core.list()
  }

  bindNotify(notify) {
    return this.core.bindNotify(notify)
  }
}

function validateCommand(options) {
  if (options === null || typeof options !== 'object' || Array.isArray(options)) {
    throw new Error('tuiCommands.register: options must be an object')
  }
  if (typeof options.name !== 'string' || !NAME.test(options.name)) {
    throw new Error('tuiCommands.register: name must be a lowercase slash identifier without /')
  }
  if (typeof options.description !== 'string' || options.description.length === 0) {
    throw new Error('tuiCommands.register: description must be a non-empty string')
  }
  const extra = Object.keys(options).filter((key) => key !== 'name' && key !== 'description')
  if (extra.length > 0) {
    throw new Error(`tuiCommands.register: unknown option field(s) ${extra.join(', ')}`)
  }
  return {
    name: options.name,
    description: options.description,
  }
}

/**
 * @param {object} ctx
 * @param {{ notify?: (method: string, params: object) => void }} [options]
 */
export function installTuiCommands(ctx, options = {}) {
  const entries = new Map()
  let send = typeof options.notify === 'function' ? options.notify : undefined

  function publish() {
    if (typeof send !== 'function') return
    send(CORDIS_METHODS.commandsUpdate, {
      protocol: PROTOCOL,
      commands: [...entries.values()].map((entry) => ({ ...entry.command })),
    })
  }

  function bindNotify(notify) {
    if (typeof notify !== 'function') throw new Error('tuiCommands.bindNotify: notify must be a function')
    send = notify
    publish()
  }

  function register(effectCtx, options, handler) {
    const command = validateCommand(options)
    if (typeof handler !== 'function') {
      throw new Error('tuiCommands.register: handler must be a function')
    }
    if (entries.has(command.name)) {
      throw new Error(`tuiCommands.register: command "${command.name}" is already registered`)
    }
    const entry = { command, handler, active: false }
    const setup = () => {
      if (entries.has(command.name)) {
        throw new Error(`tuiCommands.register: command "${command.name}" is already registered`)
      }
      entry.active = true
      entries.set(command.name, entry)
      publish()
      return () => {
        if (!entry.active) return
        entry.active = false
        if (entries.get(command.name) === entry) entries.delete(command.name)
        publish()
      }
    }
    const release = typeof effectCtx.effect === 'function'
      ? effectCtx.effect(setup, `tuiCommands.register(${JSON.stringify(command.name)})`)
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  async function dispatch(params) {
    if (params === null || typeof params !== 'object' || params.protocol !== PROTOCOL) {
      throw new Error('tuiCommands.dispatch: unsupported command payload')
    }
    const entry = entries.get(params.name)
    if (entry === undefined || !entry.active) {
      throw new Error(`tuiCommands.dispatch: command "${String(params.name)}" is not registered`)
    }
    const args = typeof params.args === 'string' ? params.args : ''
    return entry.handler(args, { name: entry.command.name })
  }

  function list() {
    return [...entries.values()].map((entry) => ({ ...entry.command }))
  }

  const core = { register, dispatch, list, bindNotify }
  const service = typeof ctx.provide === 'function'
    ? new TuiCommandsService(ctx, core)
    : {
        register(options, handler) {
          return register(ctx, options, handler)
        },
        dispatch,
        list,
        bindNotify,
      }
  if (typeof ctx.provide !== 'function') ctx.tuiCommands = service
  return service
}

export function apply(ctx) {
  installTuiCommands(ctx)
}

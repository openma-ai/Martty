/**
 * Structured TUI shell slots. Plugins contribute TuiNode trees; the service
 * owns validation, aggregation, lifecycle, and compositor snapshots.
 */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-slots'
export const inject = []

export const SLOT_NAMES = Object.freeze(['chrome.right'])

class TuiSlotsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiSlots')
    this.core = core
  }

  inject(slot, callback) {
    return this.core.inject(this.ctx, slot, callback)
  }

  register(options, nodes) {
    return this.core.register(this.ctx, options, nodes)
  }

  list() {
    return this.core.list()
  }

  bindNotify(notify) {
    return this.core.bindNotify(notify)
  }
}

const PROTOCOL = 0
const THEME_TOKENS = new Set([
  'bg', 'surface', 'panel', 'fg', 'fg_secondary', 'fg_tertiary', 'caption',
  'brand', 'brand_soft', 'bubble_bg', 'bubble_fg', 'border', 'code_bg', 'ok',
  'warn', 'err', 'hint', 'chip_bg',
])

const NODE_FIELDS = Object.freeze({
  group: { required: ['id', 'kind', 'children'], optional: ['title', 'tone'] },
  markdown: { required: ['id', 'kind', 'text'], optional: ['streaming'] },
  reasoning: { required: ['id', 'kind', 'text', 'done'], optional: ['seconds'] },
  user: { required: ['id', 'kind', 'text'], optional: ['queued'] },
  generic: { required: ['id', 'kind', 'title', 'body'], optional: ['status'] },
  terminal: { required: ['id', 'kind', 'title', 'body'], optional: ['exit'] },
  diff: { required: ['id', 'kind', 'title', 'unified'], optional: ['path'] },
  image: { required: ['id', 'kind', 'name', 'mime'], optional: ['dataBase64'] },
  notice: { required: ['id', 'kind', 'level', 'text'], optional: [] },
  unknown: { required: ['id', 'kind', 'want'], optional: ['title', 'detail'] },
})

function object(value, path) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`tuiSlots: ${path} must be an object`)
  }
  return value
}

function string(value, path, allowEmpty = true) {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) {
    throw new Error(`tuiSlots: ${path} must be ${allowEmpty ? 'a string' : 'a non-empty string'}`)
  }
}

function optionalString(value, path) {
  if (value !== undefined) string(value, path)
}

function optionalBoolean(value, path) {
  if (value !== undefined && typeof value !== 'boolean') {
    throw new Error(`tuiSlots: ${path} must be a boolean`)
  }
}

function validateNode(node, path, ids) {
  object(node, path)
  string(node.kind, `${path}.kind`, false)
  const shape = NODE_FIELDS[node.kind]
  if (shape === undefined) throw new Error(`tuiSlots: ${path}.kind "${node.kind}" is not supported`)
  const fields = new Set([...shape.required, ...shape.optional])
  const extras = Object.keys(node).filter((key) => !fields.has(key))
  if (extras.length > 0) throw new Error(`tuiSlots: ${path} has unknown field(s) ${extras.join(', ')}`)
  for (const field of shape.required) {
    if (!Object.hasOwn(node, field)) throw new Error(`tuiSlots: ${path}.${field} is required`)
  }
  string(node.id, `${path}.id`, false)
  if (ids.has(node.id)) throw new Error(`tuiSlots: duplicate node id "${node.id}"`)
  ids.add(node.id)

  switch (node.kind) {
    case 'group':
      optionalString(node.title, `${path}.title`)
      if (node.tone !== undefined && !THEME_TOKENS.has(node.tone)) {
        throw new Error(`tuiSlots: ${path}.tone must be a theme token`)
      }
      node.children = validateNodes(node.children, `${path}.children`, ids)
      break
    case 'markdown':
      string(node.text, `${path}.text`)
      optionalBoolean(node.streaming, `${path}.streaming`)
      break
    case 'reasoning':
      string(node.text, `${path}.text`)
      if (typeof node.done !== 'boolean') throw new Error(`tuiSlots: ${path}.done must be a boolean`)
      if (node.seconds !== undefined && (typeof node.seconds !== 'number' || node.seconds < 0)) {
        throw new Error(`tuiSlots: ${path}.seconds must be a non-negative number`)
      }
      break
    case 'user':
      string(node.text, `${path}.text`)
      optionalBoolean(node.queued, `${path}.queued`)
      break
    case 'generic':
      string(node.title, `${path}.title`)
      string(node.body, `${path}.body`)
      if (node.status !== undefined && !['running', 'ok', 'err'].includes(node.status)) {
        throw new Error(`tuiSlots: ${path}.status must be running, ok, or err`)
      }
      break
    case 'terminal':
      string(node.title, `${path}.title`)
      string(node.body, `${path}.body`)
      if (node.exit !== undefined && node.exit !== null && !Number.isInteger(node.exit)) {
        throw new Error(`tuiSlots: ${path}.exit must be an integer or null`)
      }
      break
    case 'diff':
      string(node.title, `${path}.title`)
      string(node.unified, `${path}.unified`)
      optionalString(node.path, `${path}.path`)
      break
    case 'image':
      string(node.name, `${path}.name`)
      string(node.mime, `${path}.mime`)
      optionalString(node.dataBase64, `${path}.dataBase64`)
      break
    case 'notice':
      string(node.text, `${path}.text`)
      if (!['info', 'warn', 'error'].includes(node.level)) {
        throw new Error(`tuiSlots: ${path}.level must be info, warn, or error`)
      }
      break
    case 'unknown':
      string(node.want, `${path}.want`, false)
      optionalString(node.title, `${path}.title`)
      optionalString(node.detail, `${path}.detail`)
      break
    default:
  }
  return { ...node }
}

function validateNodes(nodes, path = 'nodes', ids = new Set()) {
  if (!Array.isArray(nodes)) throw new Error(`tuiSlots: ${path} must be an array`)
  return nodes.map((node, index) => validateNode(structuredClone(node), `${path}[${index}]`, ids))
}

function namespaceNode(node, contributionId) {
  const namespaced = { ...node, id: `${contributionId}:${node.id}` }
  if (node.kind === 'group') {
    namespaced.children = node.children.map((child) => namespaceNode(child, contributionId))
  }
  return namespaced
}

function disposerOf(value) {
  if (typeof value === 'function') return value
  if (value !== null && typeof value === 'object' && typeof value.dispose === 'function') {
    return () => value.dispose()
  }
  return () => {}
}

/** Install the declared `chrome.right` registry on a Cordis client context. */
export function installTuiSlots(ctx, options = {}) {
  const contributions = new Map()
  let nextSequence = 0
  let revision = 0
  let send = typeof options.notify === 'function' ? options.notify : undefined
  let queued

  function snapshot() {
    const nodes = [...contributions.values()]
      .sort((left, right) => left.order - right.order || left.sequence - right.sequence)
      .flatMap((entry) => entry.nodes.map((node) => namespaceNode(node, entry.id)))
    revision += 1
    return { protocol: PROTOCOL, slot: 'chrome.right', rev: revision, nodes }
  }

  function publish() {
    const params = snapshot()
    if (typeof send === 'function') send(CORDIS_METHODS.slotsUpdate, params)
    else queued = params
  }

  function bindNotify(notify) {
    if (typeof notify !== 'function') throw new Error('tuiSlots.bindNotify: notify must be a function')
    send = notify
    if (queued !== undefined) {
      const params = queued
      queued = undefined
      send(CORDIS_METHODS.slotsUpdate, params)
    }
  }

  function register(effectCtx, options, nodes) {
    object(options, 'register options')
    if (options.name !== 'chrome.right') {
      throw new Error(`tuiSlots.register: slot "${String(options.name)}" is not declared; use chrome.right`)
    }
    string(options.id, 'register options.id', false)
    const extras = Object.keys(options).filter((key) => !['name', 'id', 'order'].includes(key))
    if (extras.length > 0) throw new Error(`tuiSlots.register: unknown option(s) ${extras.join(', ')}`)
    const order = options.order ?? 0
    if (typeof order !== 'number' || !Number.isFinite(order)) {
      throw new Error('tuiSlots.register: order must be a finite number')
    }
    if (contributions.has(options.id)) {
      throw new Error(`tuiSlots.register: contribution "${options.id}" is already registered`)
    }
    const entry = {
      id: options.id,
      order,
      sequence: nextSequence++,
      nodes: validateNodes(nodes),
      active: false,
    }
    const setup = () => {
      entry.active = true
      contributions.set(entry.id, entry)
      publish()
      return () => {
        if (!entry.active) return
        entry.active = false
        if (contributions.get(entry.id) === entry) contributions.delete(entry.id)
        publish()
      }
    }
    const releaseEffect = typeof effectCtx.effect === 'function'
      ? effectCtx.effect(setup, `tuiSlots.register(${JSON.stringify(entry.id)})`)
      : setup()
    let disposed = false
    const controller = {
      update(nextNodes) {
        if (disposed || !entry.active) throw new Error(`tuiSlots: contribution "${entry.id}" is disposed`)
        entry.nodes = validateNodes(nextNodes)
        publish()
      },
      dispose() {
        if (disposed) return
        disposed = true
        return releaseEffect?.()
      },
    }
    return controller
  }

  function inject(effectCtx, slot, callback) {
    if (slot !== 'chrome.right') {
      throw new Error(`tuiSlots.inject: slot "${String(slot)}" is not declared; use chrome.right`)
    }
    if (typeof callback !== 'function') throw new Error('tuiSlots.inject: callback must be a function')
    const setup = () => disposerOf(callback())
    const releaseEffect = typeof effectCtx.effect === 'function'
      ? effectCtx.effect(setup, `tuiSlots.inject(${JSON.stringify(slot)})`)
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return releaseEffect?.()
    }
  }

  function list() {
    return SLOT_NAMES.map((slot) => ({
      name: slot,
      kind: 'list',
      scope: 'root',
      occupants: [...contributions.values()].map(({ id, order }) => ({ id, order })),
    }))
  }

  const core = { inject, register, list, bindNotify }
  const service = typeof ctx.provide === 'function'
    ? new TuiSlotsService(ctx, core)
    : {
        inject(slot, callback) {
          return inject(ctx, slot, callback)
        },
        register(options, nodes) {
          return register(ctx, options, nodes)
        },
        list,
        bindNotify,
      }
  if (typeof ctx.provide !== 'function') ctx.tuiSlots = service
  return service
}

export function apply(ctx) {
  installTuiSlots(ctx)
}

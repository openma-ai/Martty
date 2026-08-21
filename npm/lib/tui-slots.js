/**
 * Structured TUI shell slots. Plugins contribute TuiNode trees; the service
 * owns validation, aggregation, lifecycle, and compositor snapshots.
 */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-slots'
export const inject = []

export const SLOT_NAMES = Object.freeze([
  'welcome.hero',
  'chrome.right',
  'conversation.input.dock',
  'conversation.composer.dock',
])

const SLOT_DEFINITIONS = Object.freeze({
  'welcome.hero': Object.freeze({ kind: 'single', scope: 'root' }),
  'chrome.right': Object.freeze({ kind: 'list', scope: 'root' }),
  'conversation.input.dock': Object.freeze({ kind: 'list', scope: 'session' }),
  'conversation.composer.dock': Object.freeze({ kind: 'list', scope: 'session' }),
})

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
  ascii: { required: ['id', 'kind', 'lines'], optional: ['tone'] },
  logo: { required: ['id', 'kind', 'name'], optional: [] },
  text: { required: ['id', 'kind', 'text'], optional: ['tone'] },
  group: { required: ['id', 'kind', 'children'], optional: ['title', 'tone'] },
  markdown: { required: ['id', 'kind', 'text'], optional: ['streaming'] },
  reasoning: { required: ['id', 'kind', 'text', 'done'], optional: ['seconds'] },
  user: { required: ['id', 'kind', 'text'], optional: ['queued'] },
  generic: { required: ['id', 'kind', 'title', 'body'], optional: ['status', 'action'] },
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

function optionalAction(value, path) {
  if (value === undefined) return
  object(value, path)
  const extras = Object.keys(value).filter((key) => !['kind', 'name', 'args'].includes(key))
  if (extras.length > 0) throw new Error(`tuiSlots: ${path} has unknown field(s) ${extras.join(', ')}`)
  if (value.kind !== 'command') throw new Error(`tuiSlots: ${path}.kind must be command`)
  string(value.name, `${path}.name`, false)
  optionalString(value.args, `${path}.args`)
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
    case 'ascii':
      if (!Array.isArray(node.lines) || node.lines.length === 0) {
        throw new Error(`tuiSlots: ${path}.lines must be a non-empty array`)
      }
      node.lines.forEach((line, index) => string(line, `${path}.lines[${index}]`))
      if (node.tone !== undefined && !THEME_TOKENS.has(node.tone)) {
        throw new Error(`tuiSlots: ${path}.tone must be a theme token`)
      }
      break
    case 'logo':
      if (node.name !== 'deepseek') {
        throw new Error(`tuiSlots: ${path}.name must be a registered logo preset`)
      }
      break
    case 'text':
      string(node.text, `${path}.text`)
      if (node.tone !== undefined && !THEME_TOKENS.has(node.tone)) {
        throw new Error(`tuiSlots: ${path}.tone must be a theme token`)
      }
      break
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
      optionalAction(node.action, `${path}.action`)
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

export function validateNodes(nodes, path = 'nodes', ids = new Set()) {
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

/** Install the declared root-slot registry on a Cordis client context. */
export function installTuiSlots(ctx, options = {}) {
  const contributions = new Map()
  let nextSequence = 0
  const revisions = new Map(SLOT_NAMES.map((slot) => [slot, 0]))
  let send = typeof options.notify === 'function' ? options.notify : undefined
  const queued = new Map()

  function snapshot(slot) {
    const nodes = [...contributions.values()]
      .filter((entry) => entry.slot === slot)
      .sort((left, right) => left.order - right.order || left.sequence - right.sequence)
      .flatMap((entry) => entry.nodes.map((node) => namespaceNode(node, entry.id)))
    const revision = (revisions.get(slot) ?? 0) + 1
    revisions.set(slot, revision)
    return { protocol: PROTOCOL, slot, rev: revision, nodes }
  }

  function publish(slot) {
    const params = snapshot(slot)
    if (typeof send === 'function') send(CORDIS_METHODS.slotsUpdate, params)
    else queued.set(slot, params)
  }

  function bindNotify(notify) {
    if (typeof notify !== 'function') throw new Error('tuiSlots.bindNotify: notify must be a function')
    send = notify
    for (const slot of SLOT_NAMES) {
      const params = queued.get(slot)
      if (params === undefined) continue
      queued.delete(slot)
      send(CORDIS_METHODS.slotsUpdate, params)
    }
  }

  function register(effectCtx, options, nodes) {
    object(options, 'register options')
    const definition = SLOT_DEFINITIONS[options.name]
    if (definition === undefined) {
      throw new Error(
        `tuiSlots.register: slot "${String(options.name)}" is not declared; use ${SLOT_NAMES.join(' or ')}`,
      )
    }
    string(options.id, 'register options.id', false)
    const extras = Object.keys(options).filter((key) => !['name', 'id', 'order'].includes(key))
    if (extras.length > 0) throw new Error(`tuiSlots.register: unknown option(s) ${extras.join(', ')}`)
    const order = options.order ?? 0
    if (typeof order !== 'number' || !Number.isFinite(order)) {
      throw new Error('tuiSlots.register: order must be a finite number')
    }
    const key = `${options.name}\u0000${options.id}`
    if (contributions.has(key)) {
      throw new Error(
        `tuiSlots.register: contribution "${options.id}" is already registered in ${options.name}`,
      )
    }
    if (definition.kind === 'single'
      && [...contributions.values()].some((entry) => entry.slot === options.name)) {
      throw new Error(`tuiSlots.register: single slot "${options.name}" is already occupied`)
    }
    const entry = {
      key,
      slot: options.name,
      id: options.id,
      order,
      sequence: nextSequence++,
      nodes: validateNodes(nodes),
      active: false,
    }
    const setup = () => {
      if (definition.kind === 'single'
        && [...contributions.values()].some((candidate) => candidate.slot === entry.slot)) {
        throw new Error(`tuiSlots.register: single slot "${entry.slot}" is already occupied`)
      }
      entry.active = true
      contributions.set(entry.key, entry)
      publish(entry.slot)
      return () => {
        if (!entry.active) return
        entry.active = false
        if (contributions.get(entry.key) === entry) contributions.delete(entry.key)
        publish(entry.slot)
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
        publish(entry.slot)
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
    if (SLOT_DEFINITIONS[slot] === undefined) {
      throw new Error(
        `tuiSlots.inject: slot "${String(slot)}" is not declared; use ${SLOT_NAMES.join(' or ')}`,
      )
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
      ...SLOT_DEFINITIONS[slot],
      occupants: [...contributions.values()]
        .filter((entry) => entry.slot === slot)
        .map(({ id, order }) => ({ id, order })),
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

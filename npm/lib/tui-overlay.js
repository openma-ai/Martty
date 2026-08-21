/** Native, semantic TUI overlays for Client Plugins. */

import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'
import { validateNodes } from './tui-slots.js'

export const name = 'tui-overlay'
export const inject = []

const PROTOCOL = 0

class TuiOverlayService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiOverlay')
    this.core = core
  }

  openSlider(options, handlers) {
    return this.core.openSlider(options, handlers)
  }

  openSelect(options, handlers) {
    return this.core.openSelect(options, handlers)
  }

  openView(options, handlers) {
    return this.core.openView(options, handlers)
  }

  dispatch(params) {
    return this.core.dispatch(params)
  }

  active() {
    return this.core.active()
  }

  bindNotify(notify) {
    return this.core.bindNotify(notify)
  }
}

function finite(value, path) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new Error(`tuiOverlay.openSlider: ${path} must be a finite number`)
  }
  return value
}

function validateSlider(input) {
  if (input === null || typeof input !== 'object' || Array.isArray(input)) {
    throw new Error('tuiOverlay.openSlider: options must be an object')
  }
  if (typeof input.id !== 'string' || input.id.length === 0) {
    throw new Error('tuiOverlay.openSlider: id must be a non-empty string')
  }
  if (typeof input.title !== 'string' || input.title.length === 0) {
    throw new Error('tuiOverlay.openSlider: title must be a non-empty string')
  }
  const min = finite(input.min, 'min')
  const max = finite(input.max, 'max')
  const step = finite(input.step, 'step')
  const value = finite(input.value, 'value')
  if (min >= max) throw new Error('tuiOverlay.openSlider: min must be less than max')
  if (step <= 0) throw new Error('tuiOverlay.openSlider: step must be greater than zero')
  const marks = input.marks === undefined ? [] : input.marks
  if (!Array.isArray(marks)) throw new Error('tuiOverlay.openSlider: marks must be an array')
  const validatedMarks = marks.map((mark, index) => {
    if (mark === null || typeof mark !== 'object' || Array.isArray(mark)) {
      throw new Error(`tuiOverlay.openSlider: marks[${index}] must be an object`)
    }
    const markValue = finite(mark.value, `marks[${index}].value`)
    if (markValue < min || markValue > max) {
      throw new Error(`tuiOverlay.openSlider: marks[${index}].value is outside the slider range`)
    }
    if (typeof mark.label !== 'string' || mark.label.length === 0) {
      throw new Error(`tuiOverlay.openSlider: marks[${index}].label must be a non-empty string`)
    }
    if (mark.id !== undefined && (typeof mark.id !== 'string' || mark.id.length === 0)) {
      throw new Error(`tuiOverlay.openSlider: marks[${index}].id must be a non-empty string`)
    }
    return {
      value: markValue,
      ...(mark.id === undefined ? {} : { id: mark.id }),
      label: mark.label,
    }
  })
  const snapToMarks = input.snapToMarks === true
  if (snapToMarks && validatedMarks.length === 0) {
    throw new Error('tuiOverlay.openSlider: snapToMarks needs at least one mark')
  }
  return {
    kind: 'slider',
    id: input.id,
    title: input.title,
    min,
    max,
    step,
    marks: validatedMarks,
    snapToMarks,
    value: Math.min(max, Math.max(min, value)),
  }
}

function validateView(input) {
  if (input === null || typeof input !== 'object' || Array.isArray(input)) {
    throw new Error('tuiOverlay.openView: options must be an object')
  }
  if (typeof input.id !== 'string' || input.id.length === 0) {
    throw new Error('tuiOverlay.openView: id must be a non-empty string')
  }
  if (typeof input.title !== 'string' || input.title.length === 0) {
    throw new Error('tuiOverlay.openView: title must be a non-empty string')
  }
  return {
    kind: 'view',
    id: input.id,
    title: input.title,
    nodes: validateNodes(input.nodes, 'view.nodes'),
  }
}

function validateSelect(input) {
  if (input === null || typeof input !== 'object' || Array.isArray(input)) {
    throw new Error('tuiOverlay.openSelect: options must be an object')
  }
  if (typeof input.id !== 'string' || input.id.length === 0) {
    throw new Error('tuiOverlay.openSelect: id must be a non-empty string')
  }
  if (typeof input.title !== 'string' || input.title.length === 0) {
    throw new Error('tuiOverlay.openSelect: title must be a non-empty string')
  }
  if (!Array.isArray(input.options) || input.options.length === 0) {
    throw new Error('tuiOverlay.openSelect: options must be a non-empty array')
  }
  const values = new Set()
  const options = input.options.map((option, index) => {
    if (option === null || typeof option !== 'object' || Array.isArray(option)) {
      throw new Error(`tuiOverlay.openSelect: options[${index}] must be an object`)
    }
    if (typeof option.value !== 'string' || option.value.length === 0) {
      throw new Error(`tuiOverlay.openSelect: options[${index}].value must be a non-empty string`)
    }
    if (values.has(option.value)) {
      throw new Error(`tuiOverlay.openSelect: duplicate value "${option.value}"`)
    }
    values.add(option.value)
    if (typeof option.label !== 'string' || option.label.length === 0) {
      throw new Error(`tuiOverlay.openSelect: options[${index}].label must be a non-empty string`)
    }
    if (option.description !== undefined && typeof option.description !== 'string') {
      throw new Error(`tuiOverlay.openSelect: options[${index}].description must be a string`)
    }
    return {
      value: option.value,
      label: option.label,
      ...(option.description === undefined ? {} : { description: option.description }),
    }
  })
  const value = input.value === undefined ? options[0].value : input.value
  if (typeof value !== 'string' || !values.has(value)) {
    throw new Error('tuiOverlay.openSelect: value must match one option')
  }
  return { kind: 'select', id: input.id, title: input.title, value, options }
}

/**
 * @param {object} ctx
 * @param {{ notify?: (method: string, params: object) => void }} [options]
 */
export function installTuiOverlay(ctx, options = {}) {
  const queue = []
  let send = typeof options.notify === 'function' ? options.notify : undefined
  let current = null

  function emit(method, params) {
    if (typeof send === 'function') send(method, params)
    else queue.push({ method, params })
  }

  function bindNotify(notify) {
    if (typeof notify !== 'function') throw new Error('tuiOverlay.bindNotify: notify must be a function')
    send = notify
    for (const item of queue.splice(0)) send(item.method, item.params)
  }

  function publish(overlay) {
    emit(CORDIS_METHODS.overlayUpdate, { protocol: PROTOCOL, overlay })
  }

  function controllerFor(entry) {
    return {
      close() {
        if (entry.closed) return
        entry.closed = true
        if (current === entry) {
          current = null
          publish(null)
        }
      },
    }
  }

  function openSlider(options, handlers = {}) {
    if (current !== null) {
      throw new Error(`tuiOverlay.openSlider: overlay "${current.overlay.id}" is already open`)
    }
    const slider = validateSlider(options)
    const entry = { overlay: slider, handlers, closed: false }
    current = entry
    publish({ ...slider, marks: slider.marks.map((mark) => ({ ...mark })) })
    return controllerFor(entry)
  }

  function openSelect(options, handlers = {}) {
    if (current !== null) {
      throw new Error(`tuiOverlay.openSelect: overlay "${current.overlay.id}" is already open`)
    }
    const select = validateSelect(options)
    const entry = { overlay: select, handlers, closed: false }
    current = entry
    publish(structuredClone(select))
    return controllerFor(entry)
  }

  function openView(options, handlers = {}) {
    const view = validateView(options)
    if (current !== null) {
      if (current.overlay.kind === 'view' && current.overlay.id === view.id) {
        current.overlay = view
        current.handlers = handlers
        publish(structuredClone(view))
        return controllerFor(current)
      }
      throw new Error(`tuiOverlay.openView: overlay "${current.overlay.id}" is already open`)
    }
    const entry = { overlay: view, handlers, closed: false }
    current = entry
    publish(structuredClone(view))
    return controllerFor(entry)
  }

  async function dispatch(params) {
    if (params === null || typeof params !== 'object' || params.protocol !== PROTOCOL) {
      throw new Error('tuiOverlay.dispatch: unsupported overlay event')
    }
    const entry = current
    if (entry === null || entry.closed || params.id !== entry.overlay.id) {
      throw new Error(`tuiOverlay.dispatch: overlay "${String(params.id)}" is not active`)
    }

    if (entry.overlay.kind === 'view') {
      if (params.event !== 'cancel' && params.event !== 'submit') {
        throw new Error(`tuiOverlay.dispatch: unknown view event "${String(params.event)}"`)
      }
      const handler = params.event === 'submit'
        ? entry.handlers.onSubmit
        : entry.handlers.onCancel
      entry.closed = true
      if (current === entry) {
        current = null
        publish(null)
      }
      return handler?.()
    }

    if (entry.overlay.kind === 'select') {
      if (!['change', 'submit', 'cancel'].includes(params.event)) {
        throw new Error(`tuiOverlay.dispatch: unknown select event "${String(params.event)}"`)
      }
      if (params.event !== 'cancel') {
        if (typeof params.value !== 'string'
          || !entry.overlay.options.some((option) => option.value === params.value)) {
          throw new Error('tuiOverlay.dispatch: select value is not an option')
        }
        entry.overlay.value = params.value
      }
      if (params.event === 'change') {
        return entry.handlers.onChange?.(entry.overlay.value)
      }
      const handler = params.event === 'submit'
        ? entry.handlers.onSubmit
        : entry.handlers.onCancel
      entry.closed = true
      if (current === entry) {
        current = null
        publish(null)
      }
      return handler?.(entry.overlay.value)
    }

    const slider = entry.overlay
    const value = finite(params.value, 'event.value')
    if (value < slider.min || value > slider.max) {
      throw new Error('tuiOverlay.dispatch: event.value is outside the slider range')
    }
    slider.value = value
    if (params.event === 'change') {
      return entry.handlers.onChange?.(value)
    }
    if (params.event !== 'submit' && params.event !== 'cancel') {
      throw new Error(`tuiOverlay.dispatch: unknown event "${String(params.event)}"`)
    }
    const mark = slider.marks.reduce((nearest, candidate) => {
      if (nearest === undefined) return candidate
      return Math.abs(candidate.value - value) < Math.abs(nearest.value - value)
        ? candidate
        : nearest
    }, undefined)
    const handler = params.event === 'submit'
      ? entry.handlers.onSubmit
      : entry.handlers.onCancel
    const controller = { close: () => {} }
    entry.closed = true
    if (current === entry) {
      current = null
      publish(null)
    }
    return handler?.(value, mark, controller)
  }

  function active() {
    return current === null ? null : structuredClone(current.overlay)
  }

  const core = { openSlider, openSelect, openView, dispatch, active, bindNotify }
  const service = typeof ctx.provide === 'function'
    ? new TuiOverlayService(ctx, core)
    : { openSlider, openSelect, openView, dispatch, active, bindNotify }
  if (typeof ctx.provide !== 'function') ctx.tuiOverlay = service
  return service
}

export function apply(ctx) {
  installTuiOverlay(ctx)
}

/** UI Presets compose several UI plugin contributions into one saved choice. */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { Service } from '@deepseek-ai/cordis'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-presets'
export const inject = ['tuiCommands', 'tuiOverlay']

const ID = /^[a-z0-9][a-z0-9-]*$/

class TuiPresetsService extends Service {
  constructor(ctx, core) {
    super(ctx, 'tuiPresets')
    this.core = core
  }

  register(options, mount) {
    return this.core.register(options, mount)
  }

  registerOwned(owner, options, mount, registerOptions) {
    return this.core.registerOwned(owner, options, mount, registerOptions)
  }

  activate(id) {
    return this.core.activate(id)
  }

  list() {
    return this.core.list()
  }

  catalog() {
    return this.core.catalog()
  }

  owner(id) {
    return this.core.owner(id)
  }

  isLoaded(id) {
    return this.core.isLoaded(id)
  }

  bindLifecycle(fn) {
    return this.core.bindLifecycle(fn)
  }

  bindNotify(fn) {
    return this.core.bindNotify(fn)
  }

  active() {
    return this.core.active()
  }
}

function readSettings(settingsPath) {
  if (typeof settingsPath !== 'string' || settingsPath.length === 0) return {}
  try {
    const value = JSON.parse(readFileSync(settingsPath, 'utf8'))
    return value !== null && typeof value === 'object' && !Array.isArray(value) ? value : {}
  } catch {
    return {}
  }
}

function writePreferred(settingsPath, id) {
  if (typeof settingsPath !== 'string' || settingsPath.length === 0) return
  const settings = readSettings(settingsPath)
  settings.uiPreset = id
  mkdirSync(path.dirname(settingsPath), { recursive: true })
  writeFileSync(settingsPath, `${JSON.stringify(settings, null, 2)}\n`)
}

function releaseOf(value) {
  if (typeof value === 'function') return value
  if (value !== null && typeof value === 'object' && typeof value.dispose === 'function') {
    return () => value.dispose()
  }
  return () => {}
}

export function installTuiPresets(ctx, options = {}) {
  const entries = new Map()
  const settingsPath = options.settingsPath
  const saved = readSettings(settingsPath).uiPreset
  let preferredId = typeof saved === 'string' && ID.test(saved) ? saved : 'default'
  let activeEntry
  let releaseActive
  let presetSelector
  let disposed = false
  let lifecycle
  let send = typeof options.notify === 'function' ? options.notify : undefined
  const queue = []

  function emit(params) {
    if (send !== undefined) send(CORDIS_METHODS.uiUpdate, params)
    else queue.push(params)
  }

  function publish() {
    emit({ protocol: 0, plugins: catalog() })
  }

  function bindNotify(fn) {
    if (typeof fn !== 'function') throw new Error('tuiPresets.bindNotify needs a function')
    send = fn
    const pending = queue.splice(0, queue.length)
    for (const params of pending) send(CORDIS_METHODS.uiUpdate, params)
    publish()
  }

  function mount(entry) {
    const mounted = entry.mount()
    if (mounted !== undefined && typeof mounted?.then === 'function') {
      throw new Error('tuiPresets: preset mounts must be synchronous')
    }
    return releaseOf(mounted)
  }

  function switchTo(entry) {
    if (activeEntry === entry) return
    const previous = activeEntry
    const releasePrevious = releaseActive
    releasePrevious?.()
    activeEntry = undefined
    releaseActive = undefined
    try {
      releaseActive = mount(entry)
      activeEntry = entry
    } catch (error) {
      if (previous !== undefined) {
        try {
          releaseActive = mount(previous)
          activeEntry = previous
        } catch {
          activeEntry = undefined
          releaseActive = undefined
        }
      }
      throw error
    }
  }

  function activate(id, persist = true) {
    if (disposed) throw new Error('tuiPresets: registry is disposed')
    if (typeof id !== 'string' || !ID.test(id)) {
      throw new Error('tuiPresets.activate: id must be a lowercase preset identifier')
    }
    const entry = entries.get(id)
    if (entry === undefined) {
      throw new Error(`tuiPresets.activate: preset "${id}" is not registered`)
    }
    if (!entry.loaded) {
      throw new Error(`tuiPresets.activate: UI Plugin for "${id}" is not loaded`)
    }
    switchTo(entry)
    if (persist) {
      preferredId = id
      writePreferred(settingsPath, id)
    }
    publish()
  }

  function fallback() {
    const preferred = entries.get(preferredId)
    const stock = entries.get('default')
    const next = (preferred?.loaded === true ? preferred : undefined)
      ?? (stock?.loaded === true ? stock : undefined)
      ?? [...entries.values()].find((entry) => entry.loaded)
    if (next !== undefined) switchTo(next)
  }

  function refreshCommandInput() {
    stopCommand?.update?.({
      input: {
        hint: 'plugin',
        options: catalog().map((entry) => ({
          value: entry.id,
          label: entry.label,
          description: `${entry.source} · ${entry.status}`,
        })),
      },
    })
    publish()
  }

  function register(options, mountPreset) {
    return registerOwned(undefined, options, mountPreset, { source: 'static' })
  }

  function registerOwned(owner, options, mountPreset, registerOptions = {}) {
    if (disposed) throw new Error('tuiPresets: registry is disposed')
    if (options === null || typeof options !== 'object' || Array.isArray(options)) {
      throw new Error('tuiPresets.register: options must be an object')
    }
    if (typeof options.id !== 'string' || !ID.test(options.id)) {
      throw new Error('tuiPresets.register: id must be a lowercase preset identifier')
    }
    if (typeof options.label !== 'string' || options.label.length === 0) {
      throw new Error('tuiPresets.register: label must be a non-empty string')
    }
    const extras = Object.keys(options).filter((key) => !['id', 'label'].includes(key))
    if (extras.length > 0) {
      throw new Error(`tuiPresets.register: unknown option(s) ${extras.join(', ')}`)
    }
    if (typeof mountPreset !== 'function') {
      throw new Error('tuiPresets.register: mount must be a function')
    }
    if (owner !== undefined && (typeof owner !== 'string' || owner.length === 0)) {
      throw new Error('tuiPresets.registerOwned: owner must be a non-empty string')
    }
    const source = owner === undefined ? 'static' : registerOptions.source ?? 'dynamic'
    if (source !== 'static' && source !== 'dynamic') {
      throw new Error('tuiPresets.register: source must be static or dynamic')
    }
    const retained = entries.get(options.id)
    const resumesOwner = owner !== undefined
      && retained?.owner === owner
      && retained.loaded === false
    if (retained !== undefined && !resumesOwner) {
      throw new Error(`tuiPresets.register: preset "${options.id}" is already registered`)
    }
    const entry = resumesOwner
      ? Object.assign(retained, { label: options.label, mount: mountPreset, source, loaded: true })
      : { id: options.id, label: options.label, mount: mountPreset, owner, source, loaded: true }
    entries.set(entry.id, entry)
    refreshCommandInput()
    if (activeEntry === undefined || entry.id === preferredId) fallback()

    let stopped = false
    return () => {
      if (stopped) return
      stopped = true
      if (entries.get(entry.id) !== entry) return
      const wasActive = activeEntry === entry
      if (wasActive) {
        releaseActive?.()
        activeEntry = undefined
        releaseActive = undefined
      }
      if (entry.owner === undefined) entries.delete(entry.id)
      else entry.loaded = false
      refreshCommandInput()
      if (wasActive) fallback()
    }
  }

  function list() {
    return [...entries.values()].map(({ id, label }) => ({ id, label }))
  }

  function catalog() {
    return [...entries.values()].map((entry) => ({
      id: entry.id,
      label: entry.label,
      source: entry.source,
      status: activeEntry === entry ? 'active' : entry.loaded
        ? (entry.source === 'dynamic' ? 'running' : 'ready')
        : 'stopped',
      ...(entry.owner === undefined ? {} : { owner: { pluginId: entry.owner } }),
    }))
  }

  function owner(id) {
    return entries.get(id)?.owner
  }

  function isLoaded(id) {
    return entries.get(id)?.loaded === true
  }

  function bindLifecycle(fn) {
    if (typeof fn !== 'function') throw new Error('tuiPresets.bindLifecycle needs a function')
    lifecycle = fn
    return () => {
      if (lifecycle === fn) lifecycle = undefined
    }
  }

  async function requestActivate(id) {
    const entry = entries.get(id)
    if (entry === undefined) throw new Error(`tuiPresets.activate: preset "${id}" is not registered`)
    if (entry.loaded) return activate(id)
    if (lifecycle === undefined || entry.owner === undefined) {
      throw new Error(`tuiPresets.activate: UI Plugin for "${id}" is not loaded`)
    }
    await lifecycle(id, entry.owner)
    if (entries.get(id)?.loaded === true && active() !== id) activate(id)
  }

  function active() {
    return activeEntry?.id
  }

  const stopCommand = ctx.tuiCommands.register({
    name: 'ui',
    description: 'Switch UI Plugin',
  }, async (args = '') => {
    const id = args.trim().toLowerCase()
    if (id.length === 0) {
      presetSelector?.close()
      presetSelector = ctx.tuiOverlay.openSelect({
        id: 'ui-preset',
        title: 'UI Plugin',
        value: active(),
        options: catalog().map((entry) => ({
          value: entry.id,
          label: entry.label,
          description: `${entry.source} · ${entry.status}`,
        })),
      }, {
        onSubmit(value) {
          presetSelector = undefined
          return requestActivate(value)
        },
        onCancel() {
          presetSelector = undefined
        },
      })
      return
    }
    await requestActivate(id)
  })
  refreshCommandInput()

  function dispose() {
    if (disposed) return
    disposed = true
    stopCommand?.()
    presetSelector?.close()
    presetSelector = undefined
    releaseActive?.()
    activeEntry = undefined
    releaseActive = undefined
    entries.clear()
    lifecycle = undefined
  }

  const core = {
    register, registerOwned, activate, list, catalog, owner, isLoaded, bindLifecycle, bindNotify,
    active, dispose,
  }
  const service = typeof ctx.provide === 'function'
    ? new TuiPresetsService(ctx, core)
    : core
  service.dispose = dispose
  if (typeof ctx.provide !== 'function') ctx.tuiPresets = service
  return service
}

export function apply(ctx, options) {
  const service = installTuiPresets(ctx, options)
  return () => service.dispose()
}

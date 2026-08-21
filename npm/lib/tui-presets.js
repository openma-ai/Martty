/** UI Presets compose several UI plugin contributions into one saved choice. */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { Service } from '@deepseek-ai/cordis'

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

  activate(id) {
    return this.core.activate(id)
  }

  list() {
    return this.core.list()
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
    switchTo(entry)
    if (persist) {
      preferredId = id
      writePreferred(settingsPath, id)
    }
  }

  function fallback() {
    const next = entries.get(preferredId) ?? entries.get('default') ?? entries.values().next().value
    if (next !== undefined) switchTo(next)
  }

  function refreshCommandInput() {
    stopCommand?.update?.({
      input: {
        hint: 'preset',
        options: list().map((entry) => ({
          value: entry.id,
          label: entry.label,
        })),
      },
    })
  }

  function register(options, mountPreset) {
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
    if (entries.has(options.id)) {
      throw new Error(`tuiPresets.register: preset "${options.id}" is already registered`)
    }
    const entry = { id: options.id, label: options.label, mount: mountPreset }
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
      entries.delete(entry.id)
      refreshCommandInput()
      if (wasActive) fallback()
    }
  }

  function list() {
    return [...entries.values()].map(({ id, label }) => ({ id, label }))
  }

  function active() {
    return activeEntry?.id
  }

  const stopCommand = ctx.tuiCommands.register({
    name: 'ui',
    description: 'Switch UI preset',
  }, async (args = '') => {
    const id = args.trim().toLowerCase()
    if (id.length === 0) {
      const choices = list()
      presetSelector?.close()
      presetSelector = ctx.tuiOverlay.openSelect({
        id: 'ui-preset',
        title: 'UI preset',
        value: active(),
        options: choices.map((entry) => ({
          value: entry.id,
          label: entry.label,
        })),
      }, {
        onSubmit(value) {
          presetSelector = undefined
          activate(value)
        },
        onCancel() {
          presetSelector = undefined
        },
      })
      return
    }
    activate(id)
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
  }

  const core = { register, activate, list, active, dispose }
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

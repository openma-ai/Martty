/**
 * Plugin ABI for TUI palette packs: `ctx.tuiTheme.register(palette)`.
 *
 * Transport method `_dsh/cordis/tui/theme/update` is compositor-private.
 * Plugins never call it.
 * Registrations that happen before JSON-RPC `notify` exists are queued and
 * flushed from `bindNotify`.
 */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute } from 'node:path'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-theme'
export const inject = []

const defaultPalette = JSON.parse(
  readFileSync(new URL('./palettes/default.json', import.meta.url), 'utf8'),
)

export const TOKEN_NAMES = Object.freeze([
  'bg',
  'surface',
  'panel',
  'fg',
  'fg_secondary',
  'fg_tertiary',
  'caption',
  'brand',
  'brand_soft',
  'bubble_bg',
  'bubble_fg',
  'border',
  'code_bg',
  'ok',
  'warn',
  'err',
  'hint',
  'chip_bg',
])

const TOKEN_DESCRIPTIONS = {
  bg: 'Full-screen background.',
  surface: 'Raised transcript surface.',
  panel: 'Composer, status, and chrome panels.',
  fg: 'Primary text.',
  fg_secondary: 'Secondary text.',
  fg_tertiary: 'Tertiary text.',
  caption: 'Captions and timestamps.',
  brand: 'Brand accent.',
  brand_soft: 'Soft brand fill.',
  bubble_bg: 'User bubble background.',
  bubble_fg: 'User bubble text.',
  border: 'Borders and rules.',
  code_bg: 'Code block background.',
  ok: 'Success state.',
  warn: 'Warning state.',
  err: 'Error state.',
  hint: 'Hint and prompt-row accent.',
  chip_bg: 'Chip fill.',
}

/** Closed token directory for Cordis Client inspect (`Theme.listTokens`). */
export const TOKEN_INSPECT = Object.freeze(
  TOKEN_NAMES.map((name) => Object.freeze({
    name,
    description: TOKEN_DESCRIPTIONS[name],
    valueType: '#RRGGBB',
    requiresLightAndDark: true,
  })),
)

const HEX = /^#[0-9A-Fa-f]{6}$/
const PALETTE_FIELDS = new Set(['id', 'label', 'dark', 'light', 'background'])
const BACKGROUND_FIELDS = new Set(['source', 'fit', 'anchor', 'opacity'])
const BACKGROUND_SOURCE_FIELDS = new Set(['kind', 'path', 'mediaType', 'base64'])
const BACKGROUND_ANCHOR_FIELDS = new Set(['x', 'y'])
const PROTOCOL = 0

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
  settings.theme = id
  mkdirSync(dirname(settingsPath), { recursive: true })
  writeFileSync(settingsPath, `${JSON.stringify(settings, null, 2)}\n`)
}

/**
 * Validate a palette against tui-palette protocol 0 (closed token names).
 * @param {unknown} palette
 * @returns {{ id: string, label: string, dark: Record<string, string>, light: Record<string, string>, background?: object }}
 */
export function validatePalette(palette) {
  if (palette === null || typeof palette !== 'object' || Array.isArray(palette)) {
    throw new Error('tuiTheme.register: palette must be an object')
  }
  const extraFields = Object.keys(palette).filter((key) => !PALETTE_FIELDS.has(key))
  if (extraFields.length > 0) {
    throw new Error(`tuiTheme.register: unknown field(s) ${extraFields.join(', ')}`)
  }
  if (typeof palette.id !== 'string' || palette.id.length < 1) {
    throw new Error('tuiTheme.register: id must be a non-empty string')
  }
  if (typeof palette.label !== 'string' || palette.label.length < 1) {
    throw new Error('tuiTheme.register: label must be a non-empty string')
  }
  const validated = {
    id: palette.id,
    label: palette.label,
    dark: validateTokenMap(palette.dark, 'dark'),
    light: validateTokenMap(palette.light, 'light'),
  }
  if (palette.background !== undefined) {
    validated.background = validateBackground(palette.background)
  }
  return validated
}

function exactFields(value, allowed, at) {
  const extra = Object.keys(value).filter((key) => !allowed.has(key))
  if (extra.length > 0) {
    throw new Error(`tuiTheme.register: ${at} unknown field(s) ${extra.join(', ')}`)
  }
}

function finiteUnit(value, fallback, at) {
  const selected = value === undefined ? fallback : value
  if (typeof selected !== 'number' || !Number.isFinite(selected)
    || selected < 0 || selected > 1) {
    throw new Error(`tuiTheme.register: ${at} must be a finite number from 0 to 1`)
  }
  return selected
}

function validateBackground(background) {
  if (background === null || typeof background !== 'object' || Array.isArray(background)) {
    throw new Error('tuiTheme.register: background must be an object')
  }
  exactFields(background, BACKGROUND_FIELDS, 'background')
  const source = background.source
  if (source === null || typeof source !== 'object' || Array.isArray(source)) {
    throw new Error('tuiTheme.register: background.source must be an object')
  }
  exactFields(source, BACKGROUND_SOURCE_FIELDS, 'background.source')
  let validatedSource
  if (source.kind === 'file') {
    if (typeof source.path !== 'string' || !isAbsolute(source.path)) {
      throw new Error('tuiTheme.register: background.source.path must be absolute')
    }
    if (source.mediaType !== undefined || source.base64 !== undefined) {
      throw new Error('tuiTheme.register: file background source only accepts kind and path')
    }
    validatedSource = { kind: 'file', path: source.path }
  } else if (source.kind === 'data') {
    if (source.path !== undefined || source.mediaType !== 'image/png'
      || typeof source.base64 !== 'string' || source.base64.length === 0) {
      throw new Error(
        'tuiTheme.register: data background source needs image/png mediaType and base64',
      )
    }
    validatedSource = {
      kind: 'data',
      mediaType: 'image/png',
      base64: source.base64,
    }
  } else {
    throw new Error('tuiTheme.register: background.source.kind must be file or data')
  }

  const fit = background.fit ?? 'cover'
  if (!['cover', 'contain', 'stretch'].includes(fit)) {
    throw new Error('tuiTheme.register: background.fit must be cover, contain, or stretch')
  }
  const anchor = background.anchor ?? {}
  if (anchor === null || typeof anchor !== 'object' || Array.isArray(anchor)) {
    throw new Error('tuiTheme.register: background.anchor must be an object')
  }
  exactFields(anchor, BACKGROUND_ANCHOR_FIELDS, 'background.anchor')
  return {
    source: validatedSource,
    fit,
    anchor: {
      x: finiteUnit(anchor.x, 0.5, 'background.anchor.x'),
      y: finiteUnit(anchor.y, 0.5, 'background.anchor.y'),
    },
    opacity: finiteUnit(background.opacity, 1, 'background.opacity'),
  }
}

function validateTokenMap(map, which) {
  if (map === null || typeof map !== 'object' || Array.isArray(map)) {
    throw new Error(`tuiTheme.register: ${which} must be an object`)
  }
  const missing = TOKEN_NAMES.filter((name) => !Object.hasOwn(map, name))
  if (missing.length > 0) {
    throw new Error(`tuiTheme.register: ${which} missing token(s) ${missing.join(', ')}`)
  }
  const extra = Object.keys(map).filter((name) => !TOKEN_NAMES.includes(name))
  if (extra.length > 0) {
    throw new Error(`tuiTheme.register: ${which} unknown token(s) ${extra.join(', ')}`)
  }
  const out = {}
  for (const name of TOKEN_NAMES) {
    const value = map[name]
    if (typeof value !== 'string' || !HEX.test(value)) {
      throw new Error(`tuiTheme.register: ${which}.${name} must be a #RRGGBB hex color`)
    }
    out[name] = value
  }
  return out
}

/**
 * Install `ctx.tuiTheme`. The service catalog starts with builtin `default`.
 * `register` grows the catalog (like adding an agent preset). `/theme` and
 * `activate` switch which pack covers. Builtin `default` is never mutated.
 * @param {object} ctx
 * @param {{ notify?: (method: string, params: object) => void, settingsPath?: string }} [options]
 * @returns {{ register: Function, activate: Function, list: Function, active: Function, preferred: Function, subscribe: Function, observeSelected: Function, exportInspectTokens: Function, bindNotify: Function, flush: Function }}
 */
export function installTuiTheme(ctx, options = {}) {
  const palettes = new Map()
  const queue = []
  const listeners = new Set()
  const settingsPath = options.settingsPath
  const savedTheme = readSettings(settingsPath).theme
  let preferredId = typeof savedTheme === 'string' && savedTheme.length > 0
    ? savedTheme
    : 'default'
  let send = typeof options.notify === 'function' ? options.notify : undefined
  let activeId = 'default'
  let activationRevision = 0

  const builtin = validatePalette(defaultPalette)
  palettes.set(builtin.id, { palette: builtin, owner: undefined, loaded: true })

  function emit(method, params) {
    if (typeof send === 'function') {
      send(method, params)
      return
    }
    queue.push({ method, params })
  }

  function flush() {
    if (typeof send !== 'function') return
    const pending = queue.splice(0, queue.length)
    for (const item of pending) send(item.method, item.params)
  }

  function bindNotify(fn) {
    send = fn
    flush()
  }

  function select(id, notifyPainter) {
    if (typeof id !== 'string' || id.length < 1) {
      throw new Error('tuiTheme.activate: id must be a non-empty string')
    }
    const entry = palettes.get(id)
    if (entry === undefined) {
      throw new Error(`tuiTheme.activate: palette "${id}" is not registered`)
    }
    if (!entry.loaded) {
      throw new Error(`tuiTheme.activate: theme Plugin for palette "${id}" is not loaded`)
    }
    const { palette } = entry
    const previousId = activeId
    activeId = id
    activationRevision += 1
    if (notifyPainter) {
      emit(CORDIS_METHODS.themeUpdate, { protocol: PROTOCOL, palette, activate: true })
    }
    if (previousId !== id) {
      for (const listener of [...listeners]) {
        try {
          listener(id)
        } catch {
          // A broken Plugin subscriber must not block the registry commit.
        }
      }
    }
  }

  function activate(id) {
    select(id, true)
    preferredId = id
    writePreferred(settingsPath, id)
  }

  function observeSelected(params) {
    if (params === null || typeof params !== 'object' || Array.isArray(params)) {
      throw new Error('tuiTheme.observeSelected: params must be an object')
    }
    if (params.protocol !== PROTOCOL) return false
    select(params.id, false)
    preferredId = params.id
    writePreferred(settingsPath, params.id)
    return true
  }

  function register(palette, registerOptions = {}) {
    return registerOwned(undefined, palette, registerOptions)
  }

  /** Dynamic Client runner entry: pin a palette to its owning Plugin id. */
  function registerOwned(owner, palette, registerOptions = {}) {
    if (owner !== undefined && (typeof owner !== 'string' || owner.length === 0)) {
      throw new Error('tuiTheme.registerOwned: owner must be a non-empty string')
    }
    const validated = validatePalette(palette)
    if (validated.id === 'default') {
      throw new Error('tuiTheme.register: cannot replace builtin palette "default"')
    }
    const replace = registerOptions.replace === true
    const retained = palettes.get(validated.id)
    const resumesOwner = owner !== undefined
      && retained?.owner === owner
      && retained.loaded === false
    if (retained !== undefined && !replace && !resumesOwner) {
      throw new Error(`tuiTheme.register: palette "${validated.id}" is already registered`)
    }
    const cover = owner !== undefined || registerOptions.activate === true
    let entry
    let released = false
    const setup = () => {
      const previousId = cover ? activeId : undefined
      let leaseRevision
      entry = { palette: validated, owner, loaded: true }
      palettes.set(validated.id, entry)
      emit(CORDIS_METHODS.themeUpdate, {
        protocol: PROTOCOL,
        palette: validated,
        activate: false,
        loaded: true,
        ...(owner === undefined ? {} : { owner: { pluginId: owner } }),
      })
      if (cover) {
        select(validated.id, true)
        leaseRevision = activationRevision
      }
      return () => {
        const wasActive = activeId === validated.id
        const ownsActivation = cover && wasActive && activationRevision === leaseRevision
        const removed = palettes.get(validated.id) === entry
        if (!removed) return
        if (ownsActivation) {
          const restoreId = previousId !== undefined && palettes.get(previousId)?.loaded === true
            ? previousId
            : 'default'
          select(restoreId, true)
        } else if (wasActive) {
          select('default', true)
        }
        if (owner === undefined) {
          palettes.delete(validated.id)
          emit(CORDIS_METHODS.themeRemove, { protocol: PROTOCOL, id: validated.id })
        } else {
          entry.loaded = false
          emit(CORDIS_METHODS.themeUpdate, {
            protocol: PROTOCOL,
            palette: validated,
            activate: false,
            loaded: false,
            owner: { pluginId: owner },
          })
        }
      }
    }
    const update = (patch) => {
      if (released || entry === undefined || palettes.get(validated.id) !== entry) {
        throw new Error(`tuiTheme.register: palette "${validated.id}" is no longer registered`)
      }
      if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) {
        throw new Error('tuiTheme.register: update patch must be an object')
      }
      const extra = Object.keys(patch).filter((key) => key !== 'background')
      if (extra.length > 0) {
        throw new Error(`tuiTheme.register: update unknown field(s) ${extra.join(', ')}`)
      }
      if (!Object.hasOwn(patch, 'background')) {
        throw new Error('tuiTheme.register: update needs background')
      }
      const next = { ...entry.palette }
      if (patch.background === null) delete next.background
      else next.background = validateBackground(patch.background)
      entry.palette = next
      emit(CORDIS_METHODS.themeUpdate, {
        protocol: PROTOCOL,
        palette: next,
        activate: false,
        loaded: true,
        ...(owner === undefined ? {} : { owner: { pluginId: owner } }),
      })
    }
    let release
    if (typeof ctx.effect === 'function') {
      release = ctx.effect(setup, `tuiTheme.register(${JSON.stringify(validated.id)})`)
    } else {
      release = setup()
    }
    const dispose = () => {
      if (released) return
      released = true
      return release?.()
    }
    dispose.update = update
    dispose.dispose = dispose
    return dispose
  }

  function list() {
    return [...palettes.values()].map(({ palette }) => ({
      id: palette.id,
      label: palette.label,
    }))
  }

  function active() {
    return activeId
  }

  function preferred() {
    return preferredId
  }

  function owner(id) {
    return palettes.get(id)?.owner
  }

  function isLoaded(id) {
    return palettes.get(id)?.loaded === true
  }

  function subscribe(listener) {
    if (typeof listener !== 'function') {
      throw new Error('tuiTheme.subscribe: listener must be a function')
    }
    const setup = () => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    }
    const release = typeof ctx.effect === 'function'
      ? ctx.effect(setup, 'tuiTheme.subscribe')
      : setup()
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      return release?.()
    }
  }

  function exportInspectTokens() {
    return TOKEN_INSPECT.map((token) => ({ ...token }))
  }

  const service = {
    register,
    registerOwned,
    activate,
    list,
    active,
    preferred,
    owner,
    isLoaded,
    subscribe,
    observeSelected,
    exportInspectTokens,
    bindNotify,
    flush,
  }
  if (typeof ctx.provide === 'function') {
    ctx.provide('tuiTheme', service)
  } else {
    ctx.tuiTheme = service
  }

  return service
}

/** Cordis plugin entry: provide the client-tree theme registry. */
export function apply(ctx, options) {
  installTuiTheme(ctx, options)
}

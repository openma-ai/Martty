/** Host-side directory of installed package entries for the separate TUI Client tree. */

import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { Service } from '@deepseek-ai/cordis'

export const name = 'tui-client-plugin-registry'
export const inject = []

const ID = /^[a-z0-9][a-z0-9-]*$/
const KINDS = new Set(['theme', 'ui', 'ui-preset'])

function normalizeEntry(value) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error('tuiClientPlugins.register: entry must be an absolute path or file URL')
  }
  if (path.isAbsolute(value)) return pathToFileURL(value).href
  let url
  try {
    url = new URL(value)
  } catch {
    throw new Error('tuiClientPlugins.register: entry must be an absolute path or file URL')
  }
  if (url.protocol !== 'file:') {
    throw new Error('tuiClientPlugins.register: entry must use the file protocol')
  }
  return url.href
}

class TuiClientPluginsService extends Service {
  constructor(ctx) {
    super(ctx, 'tuiClientPlugins')
    this.entries = new Map()
  }

  register(options) {
    if (options === null || typeof options !== 'object' || Array.isArray(options)) {
      throw new Error('tuiClientPlugins.register: options must be an object')
    }
    if (typeof options.id !== 'string' || !ID.test(options.id)) {
      throw new Error('tuiClientPlugins.register: id must be a lowercase package identifier')
    }
    if (!KINDS.has(options.kind)) {
      throw new Error('tuiClientPlugins.register: kind must be "theme" or "ui"')
    }
    if (this.entries.has(options.id)) {
      throw new Error(`tuiClientPlugins.register: id ${JSON.stringify(options.id)} is already registered`)
    }
    const entry = {
      id: options.id,
      kind: options.kind === 'ui-preset' ? 'ui' : options.kind,
      entry: normalizeEntry(options.entry),
    }
    this.entries.set(entry.id, entry)
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      if (this.entries.get(entry.id) === entry) this.entries.delete(entry.id)
    }
  }

  list() {
    return [...this.entries.values()].map((entry) => ({ ...entry }))
  }
}

export function apply(ctx) {
  return new TuiClientPluginsService(ctx)
}

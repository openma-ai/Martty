/** Merge installed package entries and Creator-authored TUI Client plugins. */

import { applyClientHalf, applyClientPlugin, createClientTimer } from './client-run.js'

const LOCAL_PREFIX = 'tui-local:'
const PACKAGE_PREFIX = 'tui-package:'

/**
 * @param {object} ctx
 * @param {{
 *   store: object,
 *   packages?: Array<{ id: string, kind: 'theme' | 'ui-preset', entry: string }>,
 *   tuiTheme?: object,
 *   tuiPresets?: object,
 *   tuiSlots?: object,
 *   tuiCommands?: object,
 *   tuiOverlay?: object,
 *   acpSessionConfig?: object,
 *   acpSessionPlan?: object,
 *   acpSessionStats?: object,
 *   acpSessionStatus?: object,
 * }} options
 */
export function installTuiLocalPlugins(ctx, options) {
  const { store } = options
  if (store === undefined || typeof store.list !== 'function' || typeof store.resolve !== 'function') {
    throw new Error('tuiLocalPlugins: a durable store is required')
  }
  const records = new Map()
  const failures = []
  const timer = createClientTimer()
  let discovered = false
  let disposed = false

  const env = (pluginId) => ({
    pluginId,
    tuiTheme: options.tuiTheme,
    tuiPresets: options.tuiPresets,
    tuiSlots: options.tuiSlots,
    tuiCommands: options.tuiCommands,
    tuiOverlay: options.tuiOverlay,
    acpSessionConfig: options.acpSessionConfig,
    acpSessionPlan: options.acpSessionPlan,
    acpSessionStats: options.acpSessionStats,
    acpSessionStatus: options.acpSessionStatus,
    timer,
  })

  async function mount(record) {
    let applied
    if (record.entry !== undefined) {
      record.module ??= await import(record.entry)
      const plugin = typeof record.module.apply === 'function'
        ? record.module
        : record.module.default
      applied = await applyClientPlugin(plugin, env(record.pluginId))
    } else {
      applied = await applyClientHalf(record.artifact.code.client, env(record.pluginId))
    }
    if (applied.waitingFor.length > 0) {
      applied.dispose()
      throw new Error(`waiting for unavailable service(s): ${applied.waitingFor.join(', ')}`)
    }
    record.release = applied.dispose
    record.loaded = true
  }

  async function recover(record) {
    records.set(record.pluginId, record)
    try {
      await mount(record)
      if (record.kind === 'theme') {
        record.release?.()
        record.release = undefined
        record.loaded = false
      }
      return true
    } catch (error) {
      record.release?.()
      records.delete(record.pluginId)
      failures.push({
        artifactId: record.artifactId,
        message: error instanceof Error ? error.message : String(error),
      })
      return false
    }
  }

  async function discover() {
    if (disposed) throw new Error('tuiLocalPlugins: registry is disposed')
    if (discovered) return
    discovered = true
    const installedIds = new Set()
    for (const entry of options.packages ?? []) {
      installedIds.add(entry.id)
      await recover({
        artifactId: entry.id,
        pluginId: `${PACKAGE_PREFIX}${entry.id}`,
        kind: entry.kind,
        entry: entry.entry,
        module: undefined,
        loaded: false,
        release: undefined,
      })
    }
    for (const row of store.list()) {
      if (installedIds.has(row.id)) {
        failures.push({
          artifactId: row.id,
          message: 'Creator artifact is shadowed by an installed package with the same id',
        })
        continue
      }
      if (row.broken !== undefined) {
        failures.push({ artifactId: row.id, message: row.broken })
        continue
      }
      let artifact
      try {
        artifact = store.resolve(row.id)
      } catch (error) {
        failures.push({
          artifactId: row.id,
          message: error instanceof Error ? error.message : String(error),
        })
        continue
      }
      const pluginId = `${LOCAL_PREFIX}${artifact.id}`
      const record = {
        artifact,
        artifactId: artifact.id,
        pluginId,
        kind: artifact.kind,
        loaded: false,
        release: undefined,
      }
      await recover(record)
    }
    const preferredTheme = options.tuiTheme?.preferred?.()
    const preferredOwner = typeof preferredTheme === 'string'
      ? options.tuiTheme?.owner?.(preferredTheme)
      : undefined
    if (typeof preferredOwner === 'string' && records.has(preferredOwner)) {
      try {
        await start(preferredOwner)
        options.tuiTheme.activate(preferredTheme)
      } catch (error) {
        failures.push({
          artifactId: records.get(preferredOwner)?.artifactId ?? preferredOwner,
          message: `failed to restore preferred theme: ${error instanceof Error ? error.message : String(error)}`,
        })
      }
    }
  }

  async function start(pluginId) {
    if (disposed) throw new Error('tuiLocalPlugins: registry is disposed')
    const record = records.get(pluginId)
    if (record === undefined) throw new Error(`tuiLocalPlugins: unknown Plugin ${JSON.stringify(pluginId)}`)
    if (record.loaded) return
    await mount(record)
  }

  async function stop(pluginId) {
    const record = records.get(pluginId)
    if (record === undefined || !record.loaded) return false
    record.release?.()
    record.release = undefined
    record.loaded = false
    return true
  }

  function has(pluginId) {
    return records.has(pluginId)
  }

  function isLoaded(pluginId) {
    return records.get(pluginId)?.loaded === true
  }

  function list() {
    return [...records.values()].map((record) => ({
      artifactId: record.artifactId,
      pluginId: record.pluginId,
      kind: record.kind,
      loaded: record.loaded,
    }))
  }

  function diagnostics() {
    return failures.map((failure) => ({ ...failure }))
  }

  async function dispose() {
    if (disposed) return
    disposed = true
    for (const record of [...records.values()].reverse()) {
      record.release?.()
      record.release = undefined
      record.loaded = false
    }
    records.clear()
  }

  const service = { discover, start, stop, has, isLoaded, list, diagnostics, dispose }
  if (typeof ctx.provide === 'function') ctx.provide('tuiLocalPlugins', service)
  if (typeof ctx.effect === 'function') {
    ctx.effect(() => () => service.dispose(), 'tui-local-plugins')
  } else {
    ctx.tuiLocalPlugins = service
  }
  return service
}

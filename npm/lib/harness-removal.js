/** Removal is intentionally limited to saved recipes and exact private binary installations. */
import { existsSync, lstatSync, readdirSync, realpathSync, renameSync, rmSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import path from 'node:path'
import { savedHarnesses, removeHarnessConfiguration } from './harnesses.js'

const inside = (root, value) => value === root || value.startsWith(root + path.sep)
const references = entry => [entry.command, ...(entry.args ?? []), ...Object.entries(entry.env ?? {})
  .flatMap(([key, value]) => key.toUpperCase() === 'PATH' ? value.split(path.delimiter) : [value])]
  .filter(value => typeof value === 'string')
  .map(value => !path.isAbsolute(value) && value.startsWith('--') && value.includes('=') ? value.slice(value.indexOf('=') + 1) : value)
  .filter(value => path.isAbsolute(value))

function hasLink(directory) {
  if (lstatSync(directory).isSymbolicLink()) return true
  if (!lstatSync(directory).isDirectory()) return false
  return readdirSync(directory).some(name => hasLink(path.join(directory, name)))
}

function privateInstallation(settingsPath, entry) {
  const root = path.resolve(path.dirname(settingsPath), 'bin')
  const canonicalRoot = existsSync(root) ? realpathSync(root) : root
  const candidate = references(entry).find(value => inside(root, path.resolve(value)) || inside(canonicalRoot, path.resolve(value)))
  if (!candidate) return { resources: [], cleanupReason: 'No private binary installation. External programs and shared npx/uvx caches are kept.' }
  const parts = path.relative(inside(root, path.resolve(candidate)) ? root : canonicalRoot, path.resolve(candidate)).split(path.sep)
  // The installer owns bin/<registry-id>/<version>/<platform>, never the whole bin or id directory.
  if (parts.length < 4 || parts.slice(0, 3).some(part => !/^[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(part))) {
    return { resources: [], cleanupReason: 'The path is not a recognized private binary installation.' }
  }
  const directory = path.join(root, ...parts.slice(0, 3))
  let cursor = directory
  while (true) {
    try {
      if (lstatSync(cursor).isSymbolicLink()) return { resources: [], cleanupReason: 'A symbolic link is present; resource cleanup is disabled.' }
    } catch (error) { if (error.code !== 'ENOENT') throw error }
    const parent = path.dirname(cursor)
    if (cursor === path.dirname(root)) break
    cursor = parent
  }
  if (!existsSync(directory)) return { resources: [], cleanupReason: 'Private installation is already absent.' }
  if (hasLink(directory)) return { resources: [], cleanupReason: 'The installation contains a symbolic link; resource cleanup is disabled.' }
  return { resources: [directory] }
}

export function planHarnessRemoval(settingsPath, id, options = {}) {
  const entries = savedHarnesses(settingsPath)
  const entry = entries.find(entry => entry.id === id)
  if (!entry) throw new Error('Only a saved Harness configuration can be removed')
  if (options.isCurrent?.(entry)) throw new Error('This Harness is still running. Restart Martty with another default before removing it')
  const forced = [options.forcedHarness, ...(options.defaults ?? []).filter(entry => entry.source === 'forced')].filter(Boolean)
  if (forced.some(value => value.id === id || value.command === entry.command)) throw new Error('This Harness is product-forced and cannot be removed here')
  const installation = privateInstallation(settingsPath, entry)
  if (installation.resources.length) {
    const directory = installation.resources[0]
    const shared = [...entries.filter(value => value.id !== id), ...(options.defaults ?? []), ...forced]
      .some(value => references(value).some(ref => {
        const resolved = existsSync(ref) ? realpathSync(ref) : path.resolve(ref)
        return inside(directory, path.resolve(ref)) || inside(realpathSync(directory), resolved)
      }))
    if (shared) return { entry, resources: [], cleanupReason: 'The installation is shared by another Harness configuration; its resources must be kept.' }
  }
  return { entry, ...installation }
}

export function removeHarness(settingsPath, expected, options = {}) {
  const plan = planHarnessRemoval(settingsPath, expected.entry.id, options)
  if (JSON.stringify(plan.entry) !== JSON.stringify(expected.entry)) throw new Error('Harness configuration changed; review removal again')
  if (options.cleanup && plan.cleanupReason) throw new Error(plan.cleanupReason)
  if (options.cleanup && JSON.stringify(plan.resources) !== JSON.stringify(expected.resources)) throw new Error('Installation paths changed; review removal again')
  // Move the exact validated installation aside before changing settings. No external cache or ancestor is deleted.
  const moved = []
  try {
    if (options.cleanup) for (const resource of plan.resources) {
      const quarantine = path.join(path.dirname(resource), `.removed-${randomUUID()}`)
      renameSync(resource, quarantine)
      moved.push({ resource, quarantine })
    }
    removeHarnessConfiguration(settingsPath, plan.entry)
  } catch (error) {
    for (const { resource, quarantine } of moved.reverse()) renameSync(quarantine, resource)
    throw error
  }
  for (const { quarantine } of moved) {
    try { rmSync(quarantine, { recursive: true }) }
    catch (error) { throw new Error(`Configuration removed, but cleanup failed at ${quarantine}: ${error.message}`) }
  }
  return { entry: plan.entry, removed: moved.map(({ resource }) => resource) }
}

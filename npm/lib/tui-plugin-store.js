/** Durable user-authored TUI Client plugins. */

import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'

const ARTIFACT_ID = /^[a-z0-9][a-z0-9-]*$/
const KINDS = new Set(['theme', 'ui-preset'])
const MANIFEST = 'plugin.json'

function artifactId(value) {
  if (typeof value !== 'string' || !ARTIFACT_ID.test(value)) {
    throw new Error('tuiPluginStore: id must be a lowercase artifact identifier')
  }
  return value
}

function nonEmpty(value, field) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new Error(`tuiPluginStore: ${field} must be a non-empty string`)
  }
  return value
}

function validateManifest(value, expectedId) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('manifest must be an object')
  }
  if (value.schemaVersion !== 0) throw new Error('schemaVersion must be 0')
  const id = artifactId(value.id)
  if (id !== expectedId) throw new Error(`manifest id must match directory ${JSON.stringify(expectedId)}`)
  if (!KINDS.has(value.kind)) throw new Error('kind must be "theme" or "ui-preset"')
  const name = nonEmpty(value.name, 'name')
  const purpose = nonEmpty(value.purpose, 'purpose')
  if (value.source === null || typeof value.source !== 'object' || Array.isArray(value.source)) {
    throw new Error('source must be an object')
  }
  const pluginId = nonEmpty(value.source.pluginId, 'source.pluginId')
  const packageId = nonEmpty(value.source.packageId, 'source.packageId')
  if (value.code === null || typeof value.code !== 'object' || Array.isArray(value.code)) {
    throw new Error('code must be an object')
  }
  const client = nonEmpty(value.code.client, 'code.client')
  return {
    schemaVersion: 0,
    id,
    kind: value.kind,
    name,
    purpose,
    source: { pluginId, packageId },
    code: { client },
  }
}

function readManifest(file, id) {
  let parsed
  try {
    parsed = JSON.parse(readFileSync(file, 'utf8'))
  } catch (error) {
    throw new Error(`invalid JSON: ${error instanceof Error ? error.message : String(error)}`)
  }
  return validateManifest(parsed, id)
}

export function tuiPluginRoot(env = process.env) {
  return path.join(marttyHome(env), 'plugins')
}

export function marttyHome(env = process.env, userHome = homedir()) {
  if (typeof env.MARTTY_HOME === 'string' && env.MARTTY_HOME.length > 0) {
    return env.MARTTY_HOME
  }
  if (typeof env.DSH_HOME === 'string' && env.DSH_HOME.length > 0) {
    return path.join(env.DSH_HOME, '.martty')
  }
  return path.join(userHome, '.martty')
}

export function legacyTuiPluginRoot(env = process.env, userHome = homedir()) {
  const dshHome = typeof env.DSH_HOME === 'string' && env.DSH_HOME.length > 0
    ? env.DSH_HOME
    : path.join(userHome, '.dsh')
  return path.join(dshHome, '.tui-plugins')
}

/**
 * @param {{ root?: string }} [options]
 */
export function createTuiPluginStore(options = {}) {
  const root = options.root ?? tuiPluginRoot()
  if (typeof root !== 'string' || root.length === 0 || !path.isAbsolute(root)) {
    throw new Error('tuiPluginStore: root must be an absolute path')
  }
  const legacyRoot = options.legacyRoot
    ?? (options.root === undefined ? legacyTuiPluginRoot() : undefined)
  if (legacyRoot !== undefined && legacyRoot !== root && existsSync(legacyRoot)) {
    mkdirSync(root, { recursive: true })
    for (const entry of readdirSync(legacyRoot, { withFileTypes: true })) {
      if (!entry.isDirectory() || !ARTIFACT_ID.test(entry.name)) continue
      const source = path.join(legacyRoot, entry.name)
      const target = path.join(root, entry.name)
      if (!existsSync(target)) cpSync(source, target, { recursive: true, errorOnExist: true })
    }
  }

  const paths = (id) => {
    const safe = artifactId(id)
    const dir = path.join(root, safe)
    return { dir, file: path.join(dir, MANIFEST) }
  }

  function list() {
    if (!existsSync(root)) return []
    return readdirSync(root, { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && ARTIFACT_ID.test(entry.name))
      .sort((left, right) => left.name.localeCompare(right.name))
      .map((entry) => {
        const { file } = paths(entry.name)
        try {
          const artifact = readManifest(file, entry.name)
          return {
            id: artifact.id,
            kind: artifact.kind,
            name: artifact.name,
            purpose: artifact.purpose,
            path: file,
            broken: undefined,
          }
        } catch (error) {
          return {
            id: entry.name,
            kind: undefined,
            name: entry.name,
            purpose: '',
            path: file,
            broken: error instanceof Error ? error.message : String(error),
          }
        }
      })
  }

  function resolve(id) {
    const { file } = paths(id)
    try {
      return { ...readManifest(file, id), path: file }
    } catch (error) {
      throw new Error(
        `tuiPluginStore: artifact ${JSON.stringify(id)} is broken: `
        + `${error instanceof Error ? error.message : String(error)}`,
      )
    }
  }

  function save(input, saveOptions = {}) {
    const manifest = validateManifest({
      schemaVersion: 0,
      id: input?.id,
      kind: input?.kind,
      name: input?.name,
      purpose: input?.purpose,
      source: input?.source,
      code: { client: input?.clientCode },
    }, input?.id)
    const { dir, file } = paths(manifest.id)
    const replace = saveOptions.replace === true
    if (existsSync(dir) && !replace) {
      throw new Error(`tuiPluginStore: artifact ${JSON.stringify(manifest.id)} already exists`)
    }
    mkdirSync(dir, { recursive: true })
    const temporary = path.join(dir, `.${MANIFEST}.${process.pid}.${Date.now()}.tmp`)
    try {
      writeFileSync(temporary, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' })
      renameSync(temporary, file)
    } finally {
      rmSync(temporary, { force: true })
    }
    return { id: manifest.id, path: file }
  }

  function remove(id) {
    const { dir } = paths(id)
    if (!existsSync(dir)) return false
    rmSync(dir, { recursive: true, force: false })
    return true
  }

  return { root, list, resolve, save, remove }
}

/** ACP Registry loading, normalization, and managed binary installation. */

import { createHash } from 'node:crypto'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

export const ACP_REGISTRY_URL = 'https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json'

const MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
const TARGETS = Object.freeze({
  'darwin-arm64': 'darwin-aarch64',
  'darwin-x64': 'darwin-x86_64',
  'linux-arm64': 'linux-aarch64',
  'linux-x64': 'linux-x86_64',
  'win32-arm64': 'windows-aarch64',
  'win32-x64': 'windows-x86_64',
})

function stringArray(value) {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
    ? [...value]
    : []
}

function stringEnvironment(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return {}
  const entries = Object.entries(value)
    .filter(([key, item]) => key.length > 0 && typeof item === 'string')
  return Object.fromEntries(entries)
}

export function registryPlatformKey(platform = process.platform, arch = process.arch) {
  return TARGETS[`${platform}-${arch}`]
}

/**
 * Convert the public ACP Registry schema into Martty launch distributions.
 * Unsupported platform-specific binaries are omitted; package distributions
 * remain available on every platform.
 */
export function normalizeAcpRegistry(value, options = {}) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return []
  if (!Array.isArray(value.agents)) return []
  const target = registryPlatformKey(options.platform, options.arch)
  return value.agents.flatMap((agent) => {
    if (agent === null || typeof agent !== 'object' || Array.isArray(agent)) return []
    if (typeof agent.id !== 'string' || !/^[a-z][a-z0-9-]*$/.test(agent.id)) return []
    if (typeof agent.name !== 'string' || agent.name.trim().length === 0) return []
    if (typeof agent.version !== 'string' || agent.version.trim().length === 0) return []
    const distribution = agent.distribution
    if (distribution === null || typeof distribution !== 'object' || Array.isArray(distribution)) {
      return []
    }
    const distributions = []
    for (const type of ['npx', 'uvx']) {
      const spec = distribution[type]
      if (spec === null || typeof spec !== 'object' || Array.isArray(spec)) continue
      if (typeof spec.package !== 'string' || spec.package.trim().length === 0) continue
      distributions.push({
        type,
        command: type,
        args: [spec.package, ...stringArray(spec.args)],
        env: stringEnvironment(spec.env),
      })
    }
    const binary = target === undefined ? undefined : distribution.binary?.[target]
    if (binary !== null && typeof binary === 'object' && !Array.isArray(binary)
      && typeof binary.archive === 'string' && binary.archive.length > 0
      && typeof binary.cmd === 'string' && binary.cmd.length > 0) {
      distributions.push({
        type: 'binary',
        target,
        command: binary.cmd,
        args: stringArray(binary.args),
        env: stringEnvironment(binary.env),
        archive: binary.archive,
        ...(typeof binary.sha256 === 'string' && binary.sha256.length > 0
          ? { sha256: binary.sha256.toLowerCase() }
          : {}),
      })
    }
    if (distributions.length === 0) return []
    return [{
      id: agent.id,
      label: agent.name,
      version: agent.version,
      description: typeof agent.description === 'string' ? agent.description : '',
      distributions,
    }]
  })
}

function timeoutSignal(timeoutMs) {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  timer.unref?.()
  return { signal: controller.signal, cancel: () => clearTimeout(timer) }
}

export async function fetchAcpRegistry(options = {}) {
  const fetchImpl = options.fetchImpl ?? globalThis.fetch
  if (typeof fetchImpl !== 'function') throw new Error('ACP Registry requires fetch support')
  const timeout = timeoutSignal(options.timeoutMs ?? 8_000)
  let response
  try {
    response = await fetchImpl(options.registryUrl ?? ACP_REGISTRY_URL, {
      headers: { accept: 'application/json' },
      signal: timeout.signal,
    })
  } catch (error) {
    if (timeout.signal.aborted) throw new Error('ACP Registry request timed out')
    throw new Error(`could not fetch ACP Registry: ${error instanceof Error ? error.message : String(error)}`)
  } finally {
    timeout.cancel()
  }
  if (response?.ok !== true) {
    throw new Error(`ACP Registry returned HTTP ${response?.status ?? 'error'}`)
  }
  let value
  try {
    value = await response.json()
  } catch (error) {
    throw new Error(`ACP Registry returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`)
  }
  const records = normalizeAcpRegistry(value, options)
  if (records.length === 0 && Array.isArray(value?.agents) && value.agents.length > 0) {
    throw new Error('ACP Registry has no distributions for this platform')
  }
  return records
}

function safeComponent(value, label) {
  if (typeof value !== 'string' || value.length === 0 || value === '.' || value === '..'
    || value.includes('/') || value.includes('\\')) {
    throw new Error(`invalid ${label} in ACP Registry`)
  }
  return value.replace(/[^a-zA-Z0-9._+-]/g, '-')
}

function relativeCommand(value) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error('binary distribution has no command')
  }
  const normalized = value.replaceAll('\\', '/').replace(/^\.\//, '')
  if (path.posix.isAbsolute(normalized)
    || normalized.split('/').some((part) => part === '..' || part.length === 0)) {
    throw new Error('binary command must stay inside the installed archive')
  }
  return normalized.split('/')
}

function containedPath(root, parts) {
  const resolved = path.resolve(root, ...parts)
  const prefix = `${path.resolve(root)}${path.sep}`
  if (!resolved.startsWith(prefix)) throw new Error('binary command escapes the install directory')
  return resolved
}

export function managedBinaryPath(entry, options = {}) {
  const distribution = entry?.distribution
  if (distribution?.type !== 'binary') return undefined
  if (typeof options.settingsPath !== 'string' || options.settingsPath.length === 0) return undefined
  const id = safeComponent(entry.id, 'agent id')
  const version = safeComponent(entry.version, 'agent version')
  const target = safeComponent(distribution.target, 'binary target')
  const commandParts = relativeCommand(distribution.command)
  const installRoot = options.installRoot ?? path.join(path.dirname(options.settingsPath), 'bin')
  return containedPath(path.join(installRoot, id, version, target), commandParts)
}

function archiveSuffix(url) {
  let pathname = ''
  try {
    pathname = new URL(url).pathname.toLowerCase()
  } catch {
    throw new Error('binary distribution has an invalid archive URL')
  }
  for (const suffix of ['.tar.gz', '.tar.bz2', '.tar.xz', '.tar.zst', '.tgz', '.zip', '.tar']) {
    if (pathname.endsWith(suffix)) return suffix
  }
  throw new Error('binary distribution uses an unsupported archive format')
}

function runArchiveTool(command, args, action) {
  const result = spawnSync(command, args, { encoding: 'utf8' })
  if (result.error !== undefined || result.status !== 0) {
    const detail = result.error?.message ?? result.stderr?.trim() ?? `exit ${result.status}`
    throw new Error(`could not ${action} binary archive: ${detail}`)
  }
  return result.stdout
}

function validateArchiveEntries(entries) {
  for (const raw of entries.split(/\r?\n/).filter(Boolean)) {
    const entry = raw.replaceAll('\\', '/')
    if (entry.startsWith('/') || /^[a-zA-Z]:\//.test(entry)
      || entry.split('/').some((part) => part === '..')) {
      throw new Error('binary archive contains a path outside its install directory')
    }
  }
}

function defaultExtractArchive(archivePath, destination, archiveUrl) {
  const suffix = archiveSuffix(archiveUrl)
  if (suffix === '.zip' && process.platform !== 'win32') {
    validateArchiveEntries(runArchiveTool('unzip', ['-Z1', archivePath], 'inspect'))
    runArchiveTool('unzip', ['-q', archivePath, '-d', destination], 'extract')
    return
  }
  const tar = process.platform === 'win32' ? 'tar.exe' : 'tar'
  validateArchiveEntries(runArchiveTool(tar, ['-tf', archivePath], 'inspect'))
  runArchiveTool(tar, ['-xf', archivePath, '-C', destination], 'extract')
}

function executable(command) {
  try {
    return statSync(command).isFile()
  } catch {
    return false
  }
}

/**
 * Download an official binary distribution into Martty's own data directory.
 * The settings path anchors the default at $MARTTY_HOME/bin.
 */
export async function installRegistryBinary(entry, options = {}) {
  const distribution = entry?.distribution
  if (distribution?.type !== 'binary') throw new Error('registry entry is not a binary distribution')
  if (typeof options.settingsPath !== 'string' || options.settingsPath.length === 0) {
    throw new Error('binary installation needs a Martty settings path')
  }
  const id = safeComponent(entry.id, 'agent id')
  const version = safeComponent(entry.version, 'agent version')
  const target = safeComponent(distribution.target, 'binary target')
  const commandParts = relativeCommand(distribution.command)
  const installRoot = options.installRoot ?? path.join(path.dirname(options.settingsPath), 'bin')
  const installDir = path.join(installRoot, id, version, target)
  const installedCommand = containedPath(installDir, commandParts)
  if (executable(installedCommand)) {
    return {
      id: entry.id,
      label: entry.label,
      command: installedCommand,
      args: [...distribution.args],
      env: { ...distribution.env },
    }
  }
  if (existsSync(installDir)) {
    throw new Error(`incomplete binary installation already exists at ${installDir}`)
  }

  const fetchImpl = options.fetchImpl ?? globalThis.fetch
  if (typeof fetchImpl !== 'function') throw new Error('binary installation requires fetch support')
  const timeout = timeoutSignal(options.timeoutMs ?? 60_000)
  let response
  try {
    response = await fetchImpl(distribution.archive, { signal: timeout.signal })
  } catch (error) {
    if (timeout.signal.aborted) throw new Error('binary download timed out')
    throw new Error(`could not download binary: ${error instanceof Error ? error.message : String(error)}`)
  } finally {
    timeout.cancel()
  }
  if (response?.ok !== true) throw new Error(`binary download returned HTTP ${response?.status ?? 'error'}`)
  const declaredSize = Number(response.headers?.get?.('content-length'))
  if (Number.isFinite(declaredSize) && declaredSize > MAX_ARCHIVE_BYTES) {
    throw new Error('binary archive is larger than 512 MiB')
  }
  const archive = Buffer.from(await response.arrayBuffer())
  if (archive.length > MAX_ARCHIVE_BYTES) throw new Error('binary archive is larger than 512 MiB')
  if (typeof distribution.sha256 === 'string') {
    const actual = createHash('sha256').update(archive).digest('hex')
    if (actual !== distribution.sha256.toLowerCase()) {
      throw new Error(`binary checksum mismatch: expected ${distribution.sha256}, received ${actual}`)
    }
  }

  const parent = path.dirname(installDir)
  mkdirSync(parent, { recursive: true })
  const temporary = mkdtempSync(path.join(parent, '.install-'))
  try {
    const archivePath = path.join(temporary, `download${archiveSuffix(distribution.archive)}`)
    const extracted = path.join(temporary, 'payload')
    mkdirSync(extracted)
    writeFileSync(archivePath, archive, { mode: 0o600 })
    const extract = options.extractArchive ?? defaultExtractArchive
    await extract(archivePath, extracted, distribution.archive)
    const extractedCommand = containedPath(extracted, commandParts)
    if (!existsSync(extractedCommand)) {
      throw new Error(`binary archive does not contain ${distribution.command}`)
    }
    if (process.platform !== 'win32') chmodSync(extractedCommand, 0o755)
    if (!executable(extractedCommand)) throw new Error('binary command is not an executable file')
    renameSync(extracted, installDir)
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
  return {
    id: entry.id,
    label: entry.label,
    command: installedCommand,
    args: [...distribution.args],
    env: { ...distribution.env },
  }
}

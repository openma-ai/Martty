/** ACP Registry loading, normalization, and managed binary installation. */

import { createHash, randomUUID } from 'node:crypto'
import {
  chmodSync,
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  renameSync,
  rmSync,
  statSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { downloadFile } from './download.js'

export const ACP_REGISTRY_URL = 'https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json'

function registryCachePath(options) {
  return typeof options.settingsPath === 'string'
    ? path.join(path.dirname(options.settingsPath), 'cache', 'acp-registry.json') : undefined
}

/** Last validated catalog, or the official bundled snapshot on first/offline launch. */
export function readAcpRegistrySnapshot(options = {}) {
  const cachePath = registryCachePath(options)
  if (cachePath !== undefined) {
    try {
      if (statSync(cachePath).size > 16 * 1024 * 1024) throw new Error('oversized cache')
      const cached = JSON.parse(readFileSync(cachePath, 'utf8'))
      if (cached.source === (options.registryUrl ?? ACP_REGISTRY_URL) && Array.isArray(cached.catalog?.agents)) {
        return normalizeAcpRegistry(cached.catalog, options)
      }
    } catch { /* Missing/corrupt cache cannot block the bundled catalog. */ }
  }
  return normalizeAcpRegistry(JSON.parse(readFileSync(new URL('./acp-registry.snapshot.json', import.meta.url), 'utf8')), options)
}

function cacheRegistry(value, options) {
  const cachePath = registryCachePath(options)
  if (cachePath === undefined) return
  const temporary = `${cachePath}.${randomUUID()}.tmp`
  try {
    mkdirSync(path.dirname(cachePath), { recursive: true })
    writeFileSync(temporary, JSON.stringify({ source: options.registryUrl ?? ACP_REGISTRY_URL, catalog: value }))
    renameSync(temporary, cachePath)
  } catch { /* Read-only storage must not discard a successful network result. */ }
  finally { try { rmSync(temporary, { force: true }) } catch { /* best effort */ } }
}

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

function timeoutSignal(timeoutMs, externalSignal, operation) {
  const controller = new AbortController()
  const cancel = () => {
    const error = new Error(`${operation} cancelled`)
    error.name = 'AbortError'
    controller.abort(error)
  }
  if (externalSignal?.aborted) cancel()
  else externalSignal?.addEventListener('abort', cancel, { once: true })
  let timer
  const reset = (duration = timeoutMs) => {
    clearTimeout(timer)
    if (duration !== undefined && !controller.signal.aborted) {
      timer = setTimeout(() => controller.abort(new Error(`${operation} timed out`)), duration)
    }
  }
  reset()
  return {
    signal: controller.signal,
    reset,
    cancel() {
      clearTimeout(timer)
      externalSignal?.removeEventListener('abort', cancel)
    },
  }
}

function abortable(promise, signal) {
  if (signal.aborted) {
    // The operation may synchronously abort its owner and return an already
    // rejected promise. Consume it even though cancellation wins the race.
    Promise.resolve(promise).catch(() => {})
    return Promise.reject(signal.reason)
  }
  return new Promise((resolve, reject) => {
    const onAbort = () => {
      signal.removeEventListener('abort', onAbort)
      reject(signal.reason)
    }
    signal.addEventListener('abort', onAbort, { once: true })
    Promise.resolve(promise).then(resolve, reject).finally(() => {
      signal.removeEventListener('abort', onAbort)
    })
  })
}

async function* responseChunks(response, signal) {
  if (typeof response.body?.getReader !== 'function') {
    signal.throwIfAborted()
    yield Buffer.from(await abortable(response.arrayBuffer(), signal))
    return
  }
  const reader = response.body.getReader()
  try {
    for (;;) {
      signal.throwIfAborted()
      const { done, value } = await abortable(reader.read(), signal)
      if (done) return
      yield Buffer.from(value)
    }
  } finally {
    // Cancel the body as well as the request: injected fetch implementations may
    // return a Response whose stream is not connected to the request signal.
    await reader.cancel().catch(() => {})
    reader.releaseLock()
  }
}

export async function fetchAcpRegistry(options = {}) {
  const fetchImpl = options.fetchImpl ?? globalThis.fetch
  if (typeof fetchImpl !== 'function') throw new Error('ACP Registry requires fetch support')
  const timeout = timeoutSignal(options.timeoutMs ?? 8_000, options.signal, 'ACP Registry request')
  try {
    timeout.signal.throwIfAborted()
    const response = await abortable(fetchImpl(options.registryUrl ?? ACP_REGISTRY_URL, {
      headers: { accept: 'application/json' },
      signal: timeout.signal,
    }), timeout.signal)
    if (response?.ok !== true) {
      throw new Error(`ACP Registry returned HTTP ${response?.status ?? 'error'}`)
    }
    let value
    try {
      if (typeof response.body?.getReader === 'function') {
        const chunks = []
        let size = 0
        for await (const chunk of responseChunks(response, timeout.signal)) {
          size += chunk.length
          if (size > 16 * 1024 * 1024) throw new Error('catalog is larger than 16 MiB')
          chunks.push(chunk)
        }
        value = JSON.parse(Buffer.concat(chunks).toString('utf8'))
      } else {
        value = await abortable(response.json(), timeout.signal)
      }
    } catch (error) {
      throw new Error(`ACP Registry returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`)
    }
    if (!Array.isArray(value?.agents)) throw new Error('ACP Registry returned an invalid catalog')
    const records = normalizeAcpRegistry(value, options)
    if (records.length === 0 && Array.isArray(value?.agents) && value.agents.length > 0) {
      throw new Error('ACP Registry has no distributions for this platform')
    }
    cacheRegistry(value, options)
    return records
  } catch (error) {
    if (timeout.signal.aborted) throw timeout.signal.reason
    if (error instanceof Error && error.message.startsWith('ACP Registry')) throw error
    throw new Error(`could not fetch ACP Registry: ${error instanceof Error ? error.message : String(error)}`)
  } finally {
    timeout.cancel()
  }
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

function runArchiveTool(command, args, action, signal) {
  signal.throwIfAborted()
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    let stdout = ''
    let stderr = ''
    let failure
    const stop = () => child.kill('SIGKILL')
    signal.addEventListener('abort', stop, { once: true })
    child.stdout.setEncoding('utf8').on('data', (chunk) => {
      stdout += chunk
      if (stdout.length > 4 * 1024 * 1024) {
        failure = new Error('archive file listing is too large')
        stop()
      }
    })
    child.stderr.setEncoding('utf8').on('data', (chunk) => {
      stderr = (stderr + chunk).slice(-65_536)
    })
    child.on('error', (error) => { failure = error })
    child.on('close', (code) => {
      signal.removeEventListener('abort', stop)
      if (signal.aborted) reject(signal.reason)
      else if (failure !== undefined || code !== 0) {
        reject(new Error(`could not ${action} binary archive: ${failure?.message ?? (stderr.trim() || `exit ${code}`)}`))
      } else resolve(stdout)
    })
  })
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

async function defaultExtractArchive(archivePath, destination, archiveUrl, { signal }) {
  const suffix = archiveSuffix(archiveUrl)
  if (suffix === '.zip' && process.platform !== 'win32') {
    validateArchiveEntries(await runArchiveTool('unzip', ['-Z1', archivePath], 'inspect', signal))
    await runArchiveTool('unzip', ['-q', archivePath, '-d', destination], 'extract', signal)
    return
  }
  const tar = process.platform === 'win32' ? 'tar.exe' : 'tar'
  validateArchiveEntries(await runArchiveTool(tar, ['-tf', archivePath], 'inspect', signal))
  await runArchiveTool(tar, ['-xf', archivePath, '-C', destination], 'extract', signal)
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
  if (options.signal?.aborted) {
    const error = new Error('binary installation cancelled')
    error.name = 'AbortError'
    throw error
  }
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
    options.onProgress?.({ phase: 'complete' })
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

  const suffix = archiveSuffix(distribution.archive)
  const parent = path.dirname(installDir)
  mkdirSync(parent, { recursive: true })
  const temporary = mkdtempSync(path.join(parent, '.install-'))
  try {
    const archivePath = path.join(temporary, `download${suffix}`)
    await (options.downloadFile ?? downloadFile)(distribution.archive, archivePath, {
      signal: options.signal,
      onProgress: options.onProgress,
      connectTimeoutMs: options.connectTimeoutMs ?? options.timeoutMs ?? 30_000,
      idleTimeoutMs: options.idleTimeoutMs ?? options.timeoutMs ?? 60_000,
      maxBytes: MAX_ARCHIVE_BYTES,
    })
    options.signal?.throwIfAborted()
    // Validate independently of the transport before trusting an archive.
    if (statSync(archivePath).size > MAX_ARCHIVE_BYTES) throw new Error('binary archive is larger than 512 MiB')
    if (typeof distribution.sha256 === 'string') {
      const hash = createHash('sha256')
      for await (const chunk of createReadStream(archivePath, { signal: options.signal })) hash.update(chunk)
      const actual = hash.digest('hex')
      if (actual !== distribution.sha256.toLowerCase()) {
        throw new Error(`binary checksum mismatch: expected ${distribution.sha256}, received ${actual}`)
      }
    }

    const extracted = path.join(temporary, 'payload')
    mkdirSync(extracted)
    // Archive inspection and extraction share a ten-minute default deadline;
    // callers can override it, and cancellation still interrupts the extractor.
    const extraction = timeoutSignal(options.extractTimeoutMs ?? 10 * 60_000, options.signal, 'binary extraction')
    try {
      options.onProgress?.({ phase: 'extract' })
      extraction.signal.throwIfAborted()
      if (options.extractArchive !== undefined) {
        await abortable(options.extractArchive(archivePath, extracted, distribution.archive, {
          signal: extraction.signal,
        }), extraction.signal)
      } else {
        // The native extractor waits for the terminated child to close before
        // cleanup, including on Windows where open files cannot be removed.
        await defaultExtractArchive(archivePath, extracted, distribution.archive, { signal: extraction.signal })
      }
      options.onProgress?.({ phase: 'verify' })
      extraction.signal.throwIfAborted()
      const extractedCommand = containedPath(extracted, commandParts)
      if (!existsSync(extractedCommand)) {
        throw new Error(`binary archive does not contain ${distribution.command}`)
      }
      if (process.platform !== 'win32') chmodSync(extractedCommand, 0o755)
      if (!executable(extractedCommand)) throw new Error('binary command is not an executable file')
      renameSync(extracted, installDir)
    } catch (error) {
      if (extraction.signal.aborted) throw extraction.signal.reason
      throw error
    } finally {
      extraction.cancel()
    }
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
  options.onProgress?.({ phase: 'complete' })
  return {
    id: entry.id,
    label: entry.label,
    command: installedCommand,
    args: [...distribution.args],
    env: { ...distribution.env },
  }
}

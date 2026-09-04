import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'

const HARNESS_ID = /^[a-z0-9][a-z0-9-]*$/
const ANSI = Object.freeze({ reset: '\x1b[0m', bold: '\x1b[1m', dim: '\x1b[2m', cyan: '\x1b[36m', green: '\x1b[32m' })

/**
 * Published ACP adapters that can be resolved from the user's PATH.  The
 * registry is deliberately data-only: every adapter follows the same
 * local-first discovery and install-fallback flow.
 */
export const HARNESS_REGISTRY = Object.freeze([
  Object.freeze({
    id: 'codex',
    label: 'Codex Harness',
    commands: Object.freeze([
      Object.freeze({ command: 'codex-acp', args: Object.freeze([]) }),
    ]),
    install: Object.freeze({
      command: 'npx',
      args: Object.freeze(['@agentclientprotocol/codex-acp']),
    }),
    fallback: Object.freeze({
      command: 'npx',
      args: Object.freeze(['@agentclientprotocol/codex-acp']),
    }),
  }),
])

function paint(value, tone, color) {
  return color ? `${ANSI[tone]}${value}${ANSI.reset}` : value
}

function cellWidth(char) {
  const code = char.codePointAt(0)
  if (code === undefined || code < 0x20 || (code >= 0x7f && code < 0xa0)) return 0
  if ((code >= 0x300 && code <= 0x36f) || (code >= 0xfe00 && code <= 0xfe0f)) return 0
  return code >= 0x1100 && (
    code <= 0x115f || code === 0x2329 || code === 0x232a
    || (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f)
    || (code >= 0xac00 && code <= 0xd7a3)
    || (code >= 0xf900 && code <= 0xfaff)
    || (code >= 0xfe10 && code <= 0xfe19)
    || (code >= 0xfe30 && code <= 0xfe6f)
    || (code >= 0xff00 && code <= 0xff60)
    || (code >= 0xffe0 && code <= 0xffe6)
    || (code >= 0x1f300 && code <= 0x1faff)
  ) ? 2 : 1
}

function displayWidth(value) {
  return [...value].reduce((width, char) => width + cellWidth(char), 0)
}

function clip(value, width) {
  if (width <= 0) return ''
  if (displayWidth(value) <= width) return value
  if (width === 1) return '…'
  let result = ''
  let used = 0
  for (const char of value) {
    const next = cellWidth(char)
    if (used + next > width - 1) break
    result += char
    used += next
  }
  return `${result}…`
}

function compactPath(value, options) {
  if (!path.isAbsolute(value)) return value
  const roots = [
    [options.cwd, '.'],
    [options.home ?? homedir(), '~'],
  ]
  for (const [root, prefix] of roots) {
    if (typeof root !== 'string' || root.length === 0) continue
    const relative = path.relative(root, value)
    if (relative === '') return prefix
    if (!relative.startsWith('..') && !path.isAbsolute(relative)) {
      return `${prefix}/${relative}`
    }
  }
  return value
}

function argumentText(args, options) {
  return args.map((part) => {
    const display = compactPath(part, options)
    return /\s/.test(display) ? JSON.stringify(display) : display
  }).join(' ')
}

function sourceLabel(source) {
  if (source === 'configured') return 'Settings'
  if (source === 'forced') return 'Forced'
  if (source === 'builtin') return 'Bundled'
  if (source === 'path') return 'PATH'
  if (source === 'registry') return 'npx registry'
  return source
}

function fieldLine(name, value, columns, color, indent = 4) {
  const prefix = `${' '.repeat(indent)}${name.padEnd(9)}`
  return `${paint(prefix, 'dim', color)}${clip(value, columns - displayWidth(prefix))}`
}

function harnessHelp(color = false) {
  const section = (value) => paint(value, 'bold', color)
  const command = (value) => paint(value, 'cyan', color)
  return `${section('Martty Harnesses')}

Manage the ACP Harness used by standalone Martty.

${section('Usage')}
  ${command('martty harness <command> [options]')}

${section('Commands')}
  list              Show saved, bundled, and discovered Harnesses
  find [query]      Find ACP Harnesses (local first, npx fallback)
  add <id>          Configure a registry Harness or save a command
  use <id>          Set the default Harness for the next standalone launch
  help              Show this help

${section('Add options')}
  --command <cmd>   Manual ACP command (optional for registry IDs)
  --label <label>   Human-readable name
  --arg <arg>       Command argument; repeat as needed

${section('Examples')}
  ${command('martty harness list')}
  ${command('martty harness find')}
  ${command('martty harness add codex')}
  ${command('martty harness add local --label "Local ACP" --command local-acp --arg --stdio')}
  ${command('martty harness use local')}
  ${command('/harness add local --command local-acp --arg --stdio')}

The selected Harness is saved as defaultHarness in Martty settings. It is
used on the next standalone launch, which starts a new ACP session.
`
}

function addHarnessHelp(color = false) {
  const section = (value) => paint(value, 'bold', color)
  const command = (value) => paint(value, 'cyan', color)
  return `${section('Add a Harness')}

Martty connects to ACP servers, not directly to agent CLIs.

${section('Find an installed ACP Harness')}
  ${command('martty harness find')}
  Checks known commands from the npx registry on PATH, then lists other
  executable *-acp and *_acp commands found on PATH. Missing registry entries
  include a recommended npx command; ${command('martty harness add <id>')} uses
  that fallback automatically.

${section('Custom ACP command')}
  ${command('martty harness add <id> --command <cmd> [options]')}
  --command is only needed when the id is not in the npx registry or its
  local/fallback recipes do not apply.

${section('Options')}
  --command <cmd>   ACP server command to launch (manual fallback)
  --label <label>   Human-readable name
  --arg <arg>       Command argument; repeat as needed

${section('Example')}
  ${command('martty harness add local --label "Local ACP" --command local-acp --arg --stdio')}
`
}

class HarnessUsageError extends Error {
  constructor(message) {
    super(message)
    this.name = 'HarnessUsageError'
    this.exitCode = 2
  }
}

function savedHarnessOutput(harness, color = false) {
  const title = paint(`Saved ${harness.label}`, 'bold', color)
  const lines = [title, '']
  lines.push(fieldLine('ID', harness.id, 100, color, 2))
  lines.push(fieldLine('Command', harness.command, 100, color, 2))
  if (harness.args.length > 0) {
    lines.push(fieldLine('Args', argumentText(harness.args, {}), 100, color, 2))
  }
  lines.push('')
  lines.push(fieldLine('Next', `martty harness use ${harness.id}`, 100, color, 2))
  lines.push(fieldLine('Then', 'restart martty (starts a new ACP session)', 100, color, 2))
  return `${lines.join('\n')}\n`
}

function formatHarnessList(entries, defaultId, options = {}) {
  const columns = Number.isInteger(options.columns) && options.columns > 0
    ? options.columns
    : 100
  const color = options.color === true
  if (entries.length === 0) {
    return `${paint('Martty Harnesses', 'bold', color)}

No Harnesses found.

  ${paint('Add one', 'dim', color)}   martty harness add <id> --command <cmd>
  ${paint('Discover', 'dim', color)}  install an executable named *-acp or *_acp
`
  }

  const lines = [paint(`Harnesses (${entries.length})`, 'bold', color), '']
  for (const entry of entries) {
    const selected = entry.id === defaultId
    const marker = paint(selected ? '●' : '○', selected ? 'green' : 'dim', color)
    const prefix = '  ○ '
    const label = clip(entry.label, columns - displayWidth(prefix))
    lines.push(`  ${marker} ${selected ? paint(label, 'bold', color) : label}`)
    lines.push(fieldLine('ID', entry.id, columns, color))
    lines.push(fieldLine('Source', sourceLabel(entry.source), columns, color))
    lines.push(fieldLine('Command', compactPath(entry.command, options), columns, color))
    if (entry.args.length > 0) {
      lines.push(fieldLine('Args', argumentText(entry.args, options), columns, color))
    }
    lines.push('')
  }
  const defaultLabel = defaultId ?? 'none (bundled fallback on next launch)'
  lines.push(fieldLine('Default', defaultLabel, columns, color, 2))
  lines.push(fieldLine('Set', 'martty harness use <id>', columns, color, 2))
  lines.push(fieldLine('In TUI', '/harness', columns, color, 2))
  return `${lines.join('\n')}\n`
}

function formatHarnessFind(entries, query, options = {}) {
  const columns = Number.isInteger(options.columns) && options.columns > 0
    ? options.columns
    : 100
  const color = options.color === true
  if (entries.length === 0) {
    return `${paint('ACP Harness candidates', 'bold', color)}

No ACP Harnesses found in the local PATH or npx registry.

  ${paint('Add one', 'dim', color)}   martty harness add <id> --command <cmd>
`
  }
  const lines = [paint(`ACP Harness candidates (${entries.length})`, 'bold', color)]
  if (query.length > 0) lines.push(`Query   ${query}`)
  lines.push('')
  for (const entry of entries) {
    lines.push(`  ○ ${clip(entry.label, columns - 6)}`)
    lines.push(fieldLine('ID', entry.id, columns, color))
    lines.push(fieldLine('Source', sourceLabel(entry.source), columns, color))
    lines.push(fieldLine('Status', entry.status ?? 'Found locally', columns, color))
    // Keep discovered executable paths copyable; a missing registry command
    // remains the declared binary name so the user knows what installation
    // will provide.
    lines.push(`${' '.repeat(4)}Command  ${entry.resolvedCommand ?? entry.command}`)
    if (entry.args.length > 0) lines.push(`${' '.repeat(4)}Args     ${argumentText(entry.args, options)}`)
    if (entry.status === 'Not installed') {
      lines.push(fieldLine('Install', formatCommand(entry.install, options), columns, color))
      if (entry.fallback !== undefined) {
        lines.push(fieldLine('Config', `martty harness add ${entry.id}`, columns, color))
      }
      lines.push(fieldLine('After', `martty harness find ${entry.id}`, columns, color))
      lines.push(fieldLine('Manual', `martty harness add ${entry.id} --command <cmd>`, columns, color))
    } else {
      lines.push(fieldLine('Use', `martty harness use ${entry.id}`, columns, color))
    }
    lines.push('')
  }
  lines.push(fieldLine('Manual', 'martty harness add <id> --command <cmd>', columns, color, 2))
  return `${lines.join('\n')}\n`
}

function formatCommand(entry, options = {}) {
  if (entry === undefined || entry === null) return ''
  return argumentText([entry.command, ...(entry.args ?? [])], options)
}

function readSettings(settingsPath) {
  if (!existsSync(settingsPath)) return {}
  let value
  try {
    value = JSON.parse(readFileSync(settingsPath, 'utf8'))
  } catch (error) {
    throw new Error(`invalid Martty settings: ${error.message}`)
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('invalid Martty settings: root must be an object')
  }
  return value
}

function writeSettings(settingsPath, value) {
  mkdirSync(path.dirname(settingsPath), { recursive: true })
  const temporary = `${settingsPath}.${process.pid}.${Date.now()}.tmp`
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`)
  renameSync(temporary, settingsPath)
}

function validateHarness(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('harness must be an object')
  }
  if (typeof value.id !== 'string' || !HARNESS_ID.test(value.id)) {
    throw new Error('harness id must use lowercase letters, numbers, and hyphens')
  }
  if (typeof value.command !== 'string' || value.command.trim().length === 0) {
    throw new Error('harness command must be a non-empty string')
  }
  const args = value.args === undefined ? [] : value.args
  if (!Array.isArray(args) || args.some((arg) => typeof arg !== 'string')) {
    throw new Error('harness args must be an array of strings')
  }
  return {
    id: value.id,
    label: typeof value.label === 'string' && value.label.trim().length > 0
      ? value.label
      : value.id,
    command: value.command,
    args: [...args],
  }
}

function configuredHarnesses(settings) {
  if (!Array.isArray(settings.harnesses)) return []
  return settings.harnesses.map(validateHarness)
}

function withoutLegacyActive(settings) {
  const { activeHarness: _legacyActive, ...current } = settings
  if (current.defaultHarness === undefined && typeof _legacyActive === 'string') {
    current.defaultHarness = _legacyActive
  }
  return current
}

function persistedDefaultId(settings) {
  if (typeof settings.defaultHarness === 'string') return settings.defaultHarness
  // Read legacy settings during the migration window, but never write the
  // old key back. An explicit null/empty default means the bundled fallback.
  if (settings.defaultHarness === undefined && typeof settings.activeHarness === 'string') {
    return settings.activeHarness
  }
  return undefined
}

export function upsertHarness(settingsPath, harness) {
  const next = validateHarness(harness)
  const settings = readSettings(settingsPath)
  const harnesses = configuredHarnesses(settings)
  const index = harnesses.findIndex(({ id }) => id === next.id)
  if (index === -1) harnesses.push(next)
  else harnesses[index] = next
  writeSettings(settingsPath, { ...withoutLegacyActive(settings), harnesses })
  return next
}

export function setDefaultHarness(settingsPath, id) {
  const settings = readSettings(settingsPath)
  const harnesses = configuredHarnesses(settings)
  if (!harnesses.some((harness) => harness.id === id)) {
    throw new Error(`unknown harness ${JSON.stringify(id)}`)
  }
  writeSettings(settingsPath, { ...withoutLegacyActive(settings), harnesses, defaultHarness: id })
}

// Kept as a source-compatible alias for integrations compiled against 0.2.30.
// Persisted settings use only `defaultHarness`.
export const activateHarness = setDefaultHarness

export function selectedHarness(settingsPath) {
  const settings = readSettings(settingsPath)
  const id = persistedDefaultId(settings)
  if (typeof id !== 'string') return undefined
  return configuredHarnesses(settings).find((harness) => harness.id === id)
}

export function tokenizeHarnessArgs(value) {
  const tokens = []
  let token = ''
  let quote
  let escaped = false
  for (const char of String(value ?? '')) {
    if (escaped) {
      token += char
      escaped = false
    } else if (char === '\\') {
      escaped = true
    } else if (quote !== undefined) {
      if (char === quote) quote = undefined
      else token += char
    } else if (char === '"' || char === "'") {
      quote = char
    } else if (/\s/.test(char)) {
      if (token.length > 0) {
        tokens.push(token)
        token = ''
      }
    } else {
      token += char
    }
  }
  if (escaped) token += '\\'
  if (quote !== undefined) throw new HarnessUsageError(`Unclosed ${quote} quote in Harness command`)
  if (token.length > 0) tokens.push(token)
  return tokens
}

function executableFile(command) {
  if (typeof command !== 'string' || command.trim().length === 0) return false
  try {
    const stat = statSync(command)
    return stat.isFile() && (process.platform === 'win32' || (stat.mode & 0o111) !== 0)
  } catch {
    return false
  }
}

function resolvePathCommand(command, pathValue = process.env.PATH ?? '') {
  if (typeof command !== 'string' || command.trim().length === 0) return undefined
  if (path.isAbsolute(command) || command.includes(path.sep)) {
    const resolved = path.resolve(command)
    return executableFile(resolved) ? resolved : undefined
  }
  for (const directory of String(pathValue ?? '').split(path.delimiter).filter(Boolean)) {
    const resolved = path.resolve(directory, command)
    if (executableFile(resolved)) return resolved
  }
  return undefined
}

function normalizeRegistryCommand(value) {
  if (typeof value === 'string') return { command: value, args: [] }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined
  if (typeof value.command !== 'string' || value.command.trim().length === 0) return undefined
  const args = value.args === undefined ? [] : value.args
  if (!Array.isArray(args) || args.some((arg) => typeof arg !== 'string')) return undefined
  return { command: value.command, args: [...args] }
}

function registryRecords(options = {}) {
  const records = options.registry === undefined ? HARNESS_REGISTRY : options.registry
  if (!Array.isArray(records)) return []
  return records.flatMap((record) => {
    if (record === null || typeof record !== 'object' || Array.isArray(record)) return []
    if (typeof record.id !== 'string' || !HARNESS_ID.test(record.id)) return []
    const rawCommands = record.commands ?? (record.command === undefined ? [] : [
      { command: record.command, args: record.args },
    ])
    if (!Array.isArray(rawCommands)) return []
    const commands = rawCommands.map(normalizeRegistryCommand).filter(Boolean)
    if (commands.length === 0) return []
    const install = normalizeRegistryCommand(record.install ?? record.download)
    const fallback = normalizeRegistryCommand(record.fallback)
      ?? (install?.command === 'npx' ? install : undefined)
    return [{
      id: record.id,
      label: typeof record.label === 'string' && record.label.trim().length > 0
        ? record.label
        : record.id,
      commands,
      install,
      fallback,
    }]
  })
}

function registryCandidates(options = {}) {
  return registryRecords(options).map((record) => {
    const local = record.commands
      .map((spec) => ({ spec, command: resolvePathCommand(spec.command, options.pathValue) }))
      .find(({ command }) => command !== undefined)
    if (local !== undefined) {
      return {
        id: record.id,
        label: record.label,
        command: local.spec.command,
        resolvedCommand: local.command,
        args: [...local.spec.args],
        source: 'path',
        status: 'Found locally',
        registry: true,
        install: record.install,
        fallback: record.fallback,
      }
    }
    const primary = record.commands[0]
    return {
      id: record.id,
      label: record.label,
      command: primary.command,
      args: [...primary.args],
      source: 'registry',
      status: 'Not installed',
      registry: true,
      install: record.install,
      fallback: record.fallback,
    }
  })
}

function registryCandidate(id, options = {}) {
  return registryCandidates(options).find((entry) => entry.id === id)
}

export function discoverRegistryHarnesses(options = {}) {
  return registryCandidates(options).filter((entry) => entry.status === 'Found locally')
}

export function addHarness(settingsPath, id, tokens = [], options = {}) {
  const color = false
  if (id === undefined || ['help', '-h', '--help'].includes(id)) {
    throw new HarnessUsageError(addHarnessHelp(color))
  }
  if (!HARNESS_ID.test(id)) {
    throw new HarnessUsageError(
      `Invalid Harness id ${JSON.stringify(id)}. Use lowercase letters, numbers, and hyphens.`,
    )
  }
  let command
  let label
  const args = []
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index]
    const value = tokens[index + 1]
    if (value === undefined) {
      throw new HarnessUsageError(`${token} needs a value.\n\n${addHarnessHelp(color)}`)
    }
    if (token === '--command') command = value
    else if (token === '--label') label = value
    else if (token === '--arg') args.push(value)
    else {
      throw new HarnessUsageError(
        `Unknown add option ${JSON.stringify(token)}.\n\n${addHarnessHelp(color)}`,
      )
    }
    index += 1
  }
  if (typeof command !== 'string' || command.trim().length === 0) {
    const candidate = registryCandidate(id, options)
    if (candidate?.status === 'Found locally') {
      return upsertHarness(settingsPath, {
        id: candidate.id,
        label: label ?? candidate.label,
        command: candidate.resolvedCommand,
        args: args.length > 0 ? args : candidate.args,
      })
    }
    if (candidate?.status === 'Not installed' && candidate.fallback !== undefined) {
      return upsertHarness(settingsPath, {
        id: candidate.id,
        label: label ?? candidate.label,
        command: candidate.fallback.command,
        args: [
          ...candidate.fallback.args,
          ...args,
        ],
      })
    }
    if (candidate?.status === 'Not installed' && candidate.install !== undefined) {
      throw new HarnessUsageError(
        `Harness ${JSON.stringify(id)} is not installed locally.\n\n`
        + `  Install  ${formatCommand(candidate.install)}\n`
        + `  After    martty harness find ${id}\n\n`
        + `  Manual   martty harness add ${id} --command <cmd>`,
      )
    }
    throw new HarnessUsageError(
      `Missing --command for custom Harness ${JSON.stringify(id)}.\n\n`
      + `  martty harness add ${id} --command <cmd>\n\n`
      + 'The command must start an ACP-compatible server on stdin/stdout.',
    )
  }
  return upsertHarness(settingsPath, { id, label, command, args })
}

export function discoverPathHarnesses(pathValue = process.env.PATH ?? '') {
  const found = []
  const names = new Set()
  for (const directory of pathValue.split(path.delimiter).filter(Boolean)) {
    let entries
    try {
      entries = readdirSync(directory, { withFileTypes: true })
    } catch {
      continue
    }
    for (const entry of entries) {
      if (!entry.isFile() && !entry.isSymbolicLink()) continue
      const name = entry.name.replace(/\.(?:cmd|exe|bat|com)$/i, '')
      if (!/(?:^|[-_])acp$/i.test(name) || names.has(name)) continue
      const command = path.resolve(directory, entry.name)
      try {
        const stat = statSync(command)
        if (!stat.isFile()) continue
        if (process.platform !== 'win32' && (stat.mode & 0o111) === 0) continue
      } catch {
        continue
      }
      names.add(name)
      found.push({
        id: `path-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`,
        label: name,
        command,
        args: [],
        source: 'path',
      })
    }
  }
  return found
}

export function discoverHarnesses(settingsPath, options = {}) {
  const configured = configuredHarnesses(readSettings(settingsPath))
    .map((harness) => ({ ...harness, source: 'configured' }))
  const defaults = (options.defaults ?? []).map((entry) => ({
    ...validateHarness(entry),
    source: typeof entry.source === 'string' ? entry.source : 'builtin',
  }))
  const pathEntries = discoverPathHarnesses(options.pathValue)
  // A product-forced recipe must be the visible and selectable entry even
  // when a user has a saved recipe with the same id.
  const forced = defaults.filter((entry) => entry.source === 'forced')
  const ordinaryDefaults = defaults.filter((entry) => entry.source !== 'forced')
  const registry = discoverRegistryHarnesses(options).map((entry) => ({
    ...entry,
    command: entry.resolvedCommand,
  }))
  const seen = new Set()
  const seenCommands = new Set()
  return [...forced, ...configured, ...ordinaryDefaults, ...registry, ...pathEntries].filter((entry) => {
    if (seen.has(entry.id)) return false
    if (seenCommands.has(entry.command)) return false
    seen.add(entry.id)
    seenCommands.add(entry.command)
    return true
  })
}

function searchFields(entry) {
  return [
    entry.id,
    entry.label,
    entry.command,
    entry.resolvedCommand,
    entry.install?.command,
    ...(entry.install?.args ?? []),
  ].filter((value) => typeof value === 'string').map((value) => value.toLowerCase())
}

function searchScore(entry, query = '') {
  const normalized = String(query).trim().toLowerCase()
  if (normalized.length === 0) return 0
  const fields = searchFields(entry)
  const haystack = fields.join(' ')
  const terms = normalized.split(/\s+/).filter(Boolean)
  if (!terms.every((term) => haystack.includes(term))) return Number.POSITIVE_INFINITY
  if (entry.id.toLowerCase() === normalized) return 0
  if (entry.id.toLowerCase().startsWith(normalized)) return 1
  if (entry.label.toLowerCase().startsWith(normalized)) return 2
  if (haystack.includes(normalized)) return 3
  return 4
}

function findHarnessCandidates(settingsPath, options = {}, query = '') {
  const configuredIds = new Set(configuredHarnesses(readSettings(settingsPath)).map(({ id }) => id))
  const entries = [
    ...registryCandidates(options).filter((entry) => !configuredIds.has(entry.id)),
    ...discoverHarnesses(settingsPath, options).filter((entry) => entry.source !== 'configured'),
  ]
  const seen = new Set()
  return entries.map((entry, index) => ({ entry, index, score: searchScore(entry, query) }))
    .filter(({ score }) => Number.isFinite(score))
    .sort((left, right) => left.score - right.score || left.index - right.index)
    .map(({ entry }) => entry)
    .filter((entry) => {
    if (seen.has(entry.id)) return false
    seen.add(entry.id)
    return true
    })
}

export function discoverHarnessCandidates(settingsPath, options = {}, query = '') {
  return findHarnessCandidates(settingsPath, options, query)
}

export function runHarnessCommand(argv, options) {
  const settingsPath = options?.settingsPath
  if (typeof settingsPath !== 'string' || settingsPath.length === 0) {
    throw new Error('harness command needs a settings path')
  }
  const [action, id, ...tokens] = argv
  if (['help', '-h', '--help'].includes(action) || action === undefined) {
    return { code: 0, stdout: harnessHelp(options.color === true), stderr: '' }
  }
  if (action === 'find') {
    const query = [id, ...tokens].filter(Boolean).join(' ').trim()
    const candidates = findHarnessCandidates(settingsPath, options, query)
    return { code: 0, stdout: formatHarnessFind(candidates, query, options), stderr: '' }
  }
  if (action === 'list') {
    const settings = readSettings(settingsPath)
    const defaultId = persistedDefaultId(settings)
    const entries = discoverHarnesses(settingsPath, options)
    return { code: 0, stdout: formatHarnessList(entries, defaultId, options), stderr: '' }
  }
  if (action === 'use') {
    const harness = discoverHarnesses(settingsPath, options).find((entry) => entry.id === id)
    if (harness === undefined) throw new Error(`unknown harness ${JSON.stringify(id ?? '')}`)
    upsertHarness(settingsPath, harness)
    setDefaultHarness(settingsPath, id)
    return {
      code: 0,
      stdout: `default harness ${id}; next standalone launch starts a new session\n`,
      stderr: '',
    }
  }
  if (action !== 'add') throw new Error(`unknown harness command ${JSON.stringify(action ?? '')}`)
  const color = options.color === true
  const wantsAddHelp = id === undefined
    || ['help', '-h', '--help'].includes(id)
    || (tokens.length === 1 && ['-h', '--help'].includes(tokens[0]))
  if (wantsAddHelp) {
    return { code: 0, stdout: addHarnessHelp(color), stderr: '' }
  }
  const harness = addHarness(settingsPath, id, tokens, options)
  return { code: 0, stdout: savedHarnessOutput(harness, color), stderr: '' }
}

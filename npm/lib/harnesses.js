import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
export { tokenizeCommandArgs as tokenizeHarnessArgs } from './command-args.js'
import {
  fetchAcpRegistry,
  installRegistryBinary,
  managedBinaryPath,
  normalizeAcpRegistry,
  readAcpRegistrySnapshot,
} from './harness-registry.js'

export { fetchAcpRegistry } from './harness-registry.js'

const HARNESS_ID = /^[a-z0-9][a-z0-9-]*$/
const ANSI = Object.freeze({ reset: '\x1b[0m', bold: '\x1b[1m', dim: '\x1b[2m', cyan: '\x1b[36m', green: '\x1b[32m' })

// Kept as a source-compatible empty export. The source of truth is the
// official ACP Registry loaded by fetchAcpRegistry(), never a Martty table.
export const HARNESS_REGISTRY = Object.freeze([])

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
  if (source === 'managed') return 'Martty bin'
  if (source === 'forced') return 'Forced'
  if (source === 'builtin') return 'Bundled'
  if (source === 'path') return 'PATH'
  if (source === 'registry') return 'ACP Registry'
  return source
}

function fieldLine(name, value, columns, color, indent = 4) {
  const prefix = `${' '.repeat(indent)}${name.padEnd(Math.max(9, name.length + 1))}`
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
  find [query]      Find ACP Harnesses in ACP Registry and local PATH
  add <id>          Configure a registry Harness or save a command
  use <id>          Set the default Harness for the next standalone launch
  remove <id>       Remove saved configuration (asks for confirmation)
  help              Show this help

${section('Add options')}
  --command <cmd>   Manual ACP command (optional for registry IDs)
  --label <label>   Human-readable name
  --arg <arg>       Command argument; repeat as needed

${section('Examples')}
  ${command('martty harness list')}
  ${command('martty harness find')}
  ${command('martty harness add local --label "Local ACP" --command local-acp --arg --stdio')}
  ${command('martty harness use local')}
  ${command('martty harness find --refresh')}
  ${command('martty harness remove local --dry-run')}
  ${command('/harness add local --command local-acp --arg --stdio')}

Run find without a query to browse every official Registry entry. A query is
only an optional filter after you already know what you are looking for.

Find reads the cached or bundled official Registry immediately. Use --refresh
to fetch its latest catalog; offline refresh retains the local catalog.
Add reuses saved recipes/local installations and saves configuration only.
Use saves defaultHarness for the next standalone launch; it does not start an agent.
That launch starts a new ACP session.

Remove keeps installed files by default. --cleanup also deletes only an exclusive
Martty-owned binary installation; global programs, shared caches, history and
credentials are kept. --dry-run previews exact paths. --yes confirms without a
terminal prompt. Stop other Martty instances using the target before cleanup.
`
}

function addHarnessHelp(color = false) {
  const section = (value) => paint(value, 'bold', color)
  const command = (value) => paint(value, 'cyan', color)
  return `${section('Add a Harness')}

Martty connects to ACP servers, not directly to agent CLIs.

${section('Find an ACP Harness')}
  ${command('martty harness find')}
  Reads the cached/bundled ACP Registry, then adds executable *-acp and *_acp
  commands found on PATH. Package entries run through npx/uvx. Binary entries
  are installed into Martty's private bin directory after you choose them.

${section('Custom ACP command')}
  ${command('martty harness add <id> --command <cmd> [options]')}
  --command is only needed for an id outside the ACP Registry or when its
  declared distribution does not apply to this machine.

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

No ACP Harnesses found in the ACP Registry, local PATH, or settings.

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
    if (entry.distribution?.type !== undefined) {
      lines.push(fieldLine('Distribution', entry.distribution.type, columns, color))
    }
    if (entry.status === 'Available to install') {
      lines.push(fieldLine('Install', `martty harness add ${entry.id}`, columns, color))
      if (entry.installPath !== undefined) {
        lines.push(fieldLine('Location', compactPath(entry.installPath, options), columns, color))
      }
      lines.push(fieldLine('After', 'choose it from /harness (starts a new session)', columns, color))
    } else if (entry.status === 'Needs npx' || entry.status === 'Needs uvx') {
      const runtime = entry.distribution.type === 'npx'
        ? 'Node.js/npm (provides npx)'
        : 'uv (provides uvx)'
      lines.push(fieldLine('Setup', `Install ${runtime}, then run martty harness add ${entry.id}`, columns, color))
    } else if (entry.status === 'Not installed') {
      lines.push(fieldLine('Install', formatCommand(entry.install, options), columns, color))
      if (entry.fallback !== undefined) {
        lines.push(fieldLine('Config', `martty harness add ${entry.id}`, columns, color))
      } else {
        lines.push(fieldLine('Verify', `command -v ${entry.command}`, columns, color))
      }
      lines.push(fieldLine('After', `martty harness find ${entry.id}`, columns, color))
      lines.push(fieldLine('Manual', `martty harness add ${entry.id} --command <cmd>`, columns, color))
    } else if (entry.registry === true && entry.source !== 'configured') {
      lines.push(fieldLine('Add', `martty harness add ${entry.id}`, columns, color))
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
    // A corrupt settings file must never block boot (boot.js: "must not
    // block boot"). Park the unreadable file for diagnosis and start
    // over — the same resilience tui-theme/tui-presets apply.
    quarantineSettings(settingsPath, `invalid Martty settings: ${error.message}`)
    return {}
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    quarantineSettings(settingsPath, 'invalid Martty settings: root must be an object')
    return {}
  }
  return value
}

function quarantineSettings(settingsPath, reason) {
  process.stderr.write(`martty: ${reason} — moving it aside and starting fresh\n`)
  try {
    renameSync(settingsPath, `${settingsPath}.corrupt-${Date.now()}`)
  } catch {
    // Best effort only; the fresh settings write below still proceeds.
  }
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
  const env = value.env === undefined ? undefined : value.env
  if (env !== undefined && (env === null || typeof env !== 'object' || Array.isArray(env)
    || Object.entries(env).some(([key, item]) => key.length === 0 || typeof item !== 'string'))) {
    throw new Error('harness env must be an object of strings')
  }
  const normalizedArgs = /^npx(?:\.(?:cmd|bat|exe))?$/i.test(path.win32.basename(value.command))
    ? [...args].filter((arg) => arg !== '--yes' && arg !== '--prefer-offline')
    : [...args]
  return {
    id: value.id,
    label: typeof value.label === 'string' && value.label.trim().length > 0
      ? value.label
      : value.id,
    command: value.command,
    args: normalizedArgs,
    ...(env !== undefined ? { env: { ...env } } : {}),
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

export function savedHarnesses(settingsPath) {
  return configuredHarnesses(readSettings(settingsPath))
}

export function removeHarnessConfiguration(settingsPath, expected) {
  const settings = withoutLegacyActive(readSettings(settingsPath))
  const harnesses = configuredHarnesses(settings)
  const current = harnesses.find(({ id }) => id === expected.id)
  if (JSON.stringify(current) !== JSON.stringify(expected)) throw new Error('Harness configuration changed; review removal again')
  settings.harnesses = harnesses.filter(({ id }) => id !== expected.id)
  if (settings.defaultHarness === expected.id) delete settings.defaultHarness
  writeSettings(settingsPath, settings)
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

function executableFile(command) {
  if (typeof command !== 'string' || command.trim().length === 0) return false
  try {
    const stat = statSync(command)
    return stat.isFile() && (process.platform === 'win32' || (stat.mode & 0o111) !== 0)
  } catch {
    return false
  }
}

function commandNames(command, options = {}) {
  const platform = options.platform ?? process.platform
  if (platform !== 'win32' || path.extname(command).length > 0) return [command]
  const pathExt = options.pathExt ?? process.env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD'
  const extensions = String(pathExt).split(';')
    .map((extension) => extension.trim())
    .filter(Boolean)
    .map((extension) => extension.startsWith('.') ? extension : `.${extension}`)
  return [command, ...extensions.map((extension) => `${command}${extension}`)]
}

function resolvePathCommand(command, pathValue = process.env.PATH ?? '', options = {}) {
  if (typeof command !== 'string' || command.trim().length === 0) return undefined
  const names = commandNames(command, options)
  if (path.isAbsolute(command) || command.includes(path.sep)) {
    return names.map((name) => path.resolve(name)).find(executableFile)
  }
  for (const directory of String(pathValue ?? '').split(path.delimiter).filter(Boolean)) {
    const resolved = names.map((name) => path.resolve(directory, name)).find(executableFile)
    if (resolved !== undefined) return resolved
  }
  return undefined
}

function normalizeRegistryCommand(value) {
  if (typeof value === 'string') return { command: value, args: [] }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined
  if (typeof value.command !== 'string' || value.command.trim().length === 0) return undefined
  const args = value.args === undefined ? [] : value.args
  if (!Array.isArray(args) || args.some((arg) => typeof arg !== 'string')) return undefined
  const env = value.env === undefined ? {} : value.env
  if (env === null || typeof env !== 'object' || Array.isArray(env)
    || Object.entries(env).some(([key, item]) => key.length === 0 || typeof item !== 'string')) return undefined
  return { command: value.command, args: [...args], env: { ...env } }
}

function registryRecords(options = {}) {
  const rawRegistry = options.registry === undefined ? HARNESS_REGISTRY : options.registry
  const records = rawRegistry !== null && typeof rawRegistry === 'object' && !Array.isArray(rawRegistry)
    ? normalizeAcpRegistry(rawRegistry, options)
    : rawRegistry
  if (!Array.isArray(records)) return []
  return records.flatMap((record) => {
    if (record === null || typeof record !== 'object' || Array.isArray(record)) return []
    if (typeof record.id !== 'string' || !HARNESS_ID.test(record.id)) return []
    if (Array.isArray(record.distributions)) {
      const distributions = record.distributions.filter((distribution) => (
        distribution !== null
        && typeof distribution === 'object'
        && ['binary', 'npx', 'uvx'].includes(distribution.type)
        && typeof distribution.command === 'string'
        && Array.isArray(distribution.args)
      )).map((distribution) => ({
        ...distribution,
        args: distribution.args.filter((arg) => typeof arg === 'string'),
        env: distribution.env && typeof distribution.env === 'object' && !Array.isArray(distribution.env)
          ? Object.fromEntries(Object.entries(distribution.env).filter(([, item]) => typeof item === 'string'))
          : {},
      }))
      if (distributions.length === 0) return []
      return [{
        id: record.id,
        label: typeof record.label === 'string' && record.label.trim().length > 0
          ? record.label
          : record.id,
        version: typeof record.version === 'string' ? record.version : 'unversioned',
        description: typeof record.description === 'string' ? record.description : '',
        distributions,
      }]
    }
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

function registryPackageCandidate(record, distribution, options, localPackages) {
  const runner = resolvePathCommand(distribution.command, options.pathValue, options)
  const local = distribution.type === 'npx' ? localNpmExecutable(distribution, options)
    : localPythonExecutable(distribution, options, localPackages)
  return {
    id: record.id,
    label: record.label,
    version: record.version,
    description: record.description,
    command: local?.command ?? runner ?? distribution.command,
    ...(local !== undefined ? { resolvedCommand: local.command, installedVersion: local.version } : {}),
    args: local === undefined ? [...distribution.args] : distribution.args.slice(1),
    env: { ...distribution.env },
    source: local === undefined ? 'registry' : 'path',
    status: local !== undefined ? 'Found locally'
      : runner === undefined ? `Needs ${distribution.type}` : `Available via ${distribution.type}`,
    registry: true,
    distribution,
    registryRecord: record,
    runner,
  }
}

/** Resolve an installed npm package's declared bin, never a guessed CLI name. */
function localNpmExecutable(distribution, options) {
  const spec = distribution.args[0]
  if (typeof spec !== 'string') return undefined
  const packageName = /^(@[a-z0-9._-]+\/[a-z0-9._-]+|[a-z0-9._-]+)(?:@[^\s/]+)?$/i.exec(spec)?.[1]
  if (packageName === undefined || packageName.split('/').some((part) => part === '.' || part === '..')) {
    return undefined
  }
  const directories = String(options.pathValue ?? process.env.PATH ?? '').split(path.delimiter).filter(Boolean)
  for (const directory of directories) {
    // npm global prefixes use lib/node_modules on Unix and node_modules on
    // Windows. A project may explicitly put node_modules/.bin on its PATH.
    const roots = [
      path.resolve(directory, '..', 'lib', 'node_modules', packageName),
      path.resolve(directory, 'node_modules', packageName),
      ...(path.basename(directory) === '.bin' ? [path.resolve(directory, '..', packageName)] : []),
    ]
    for (const root of roots) {
      try {
        const metadata = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'))
        if (metadata.name !== packageName) continue
        const binValues = typeof metadata.bin === 'string' ? [metadata.bin]
          : metadata.bin !== null && typeof metadata.bin === 'object' && !Array.isArray(metadata.bin)
            ? Object.values(metadata.bin) : []
        const targets = [...new Set(binValues)]
        // Multiple different entrypoints require an explicit command. Picking
        // one could turn a package's management CLI into an ACP launch recipe.
        if (targets.length !== 1 || typeof targets[0] !== 'string') continue
        const packageRoot = realpathSync(root)
        const target = realpathSync(path.resolve(root, targets[0]))
        const relative = path.relative(packageRoot, target)
        if (relative.startsWith(`..${path.sep}`) || relative === '..' || path.isAbsolute(relative)) continue
        for (const entry of readdirSync(directory, { withFileTypes: true })) {
          if (!entry.isSymbolicLink()) continue
          const command = path.resolve(directory, entry.name)
          if (executableFile(command) && realpathSync(command) === target) {
            return { command, version: typeof metadata.version === 'string' ? metadata.version : undefined }
          }
        }
      } catch {
        // Missing, malformed, or stale package metadata is not proof that a
        // same-named executable belongs to this Registry package.
      }
    }
  }
  return undefined
}

function readDirectory(directory) {
  try { return readdirSync(directory, { withFileTypes: true }) } catch { return [] }
}

function pythonPackageName(value) {
  return value.toLowerCase().replace(/[-_.]+/g, '-')
}

/** Inspect installed tool environments reached from PATH; never run uv/pip. */
function localPythonExecutable(distribution, options, cache) {
  const spec = distribution.args[0]
  const name = typeof spec === 'string'
    ? /^([a-z0-9][a-z0-9._-]*)(?:@[^\s/]+|==[^\s/]+)?$/i.exec(spec)?.[1] : undefined
  if (name === undefined) return undefined
  if (!cache.has('python')) {
    const packages = new Map()
    const environments = new Map()
    const directories = String(options.pathValue ?? process.env.PATH ?? '').split(path.delimiter).filter(Boolean)
    for (const directory of directories) {
      for (const entry of readDirectory(directory)) {
        if (!entry.isSymbolicLink() && !entry.isFile()) continue
        const command = path.resolve(directory, entry.name)
        try {
          const target = realpathSync(command)
          const targetDirectory = path.dirname(target)
          if (!['bin', 'scripts'].includes(path.basename(targetDirectory).toLowerCase())) continue
          const root = path.dirname(targetDirectory)
          if (!existsSync(path.join(root, 'pyvenv.cfg')) || !executableFile(command)) continue
          const commands = environments.get(root) ?? []
          commands.push({ command, target })
          environments.set(root, commands)
        } catch { /* Broken PATH links do not prove an installed tool. */ }
      }
    }
    for (const [root, commands] of environments) {
      const sites = [path.join(root, 'Lib', 'site-packages')]
      for (const entry of readDirectory(path.join(root, 'lib'))) {
        if (entry.isDirectory() && /^python\d+\.\d+$/.test(entry.name)) {
          sites.push(path.join(root, 'lib', entry.name, 'site-packages'))
        }
      }
      for (const site of sites) {
        for (const entry of readDirectory(site)) {
          if (!entry.isDirectory() || !entry.name.endsWith('.dist-info')) continue
          try {
            const info = path.join(site, entry.name)
            const metadata = readFileSync(path.join(info, 'METADATA'), 'utf8')
            const packageName = /^Name:[ \t]*(\S+)[ \t]*$/mi.exec(metadata)?.[1]
            if (packageName === undefined) continue
            const scripts = []
            let section
            for (const line of readFileSync(path.join(info, 'entry_points.txt'), 'utf8').split(/\r?\n/)) {
              const heading = /^\s*\[([^\]]+)\]\s*$/.exec(line)
              if (heading) section = heading[1]
              else if (section === 'console_scripts') {
                const script = /^\s*([^\s=]+)\s*=\s*\S+/.exec(line)?.[1]
                if (script !== undefined) scripts.push(script)
              }
            }
            if (scripts.length !== 1) continue
            const local = commands.find(({ target }) => (
              path.basename(target).replace(/\.exe$/i, '') === scripts[0]
            ))
            const key = pythonPackageName(packageName)
            if (local !== undefined && !packages.has(key)) {
              packages.set(key, {
                command: local.command,
                version: /^Version:[ \t]*(\S+)[ \t]*$/mi.exec(metadata)?.[1],
              })
            }
          } catch { /* Incomplete or stale distribution metadata: use the recipe. */ }
        }
      }
    }
    cache.set('python', packages)
  }
  return cache.get('python').get(pythonPackageName(name))
}

function registryCandidates(options = {}, records = registryRecords(options)) {
  const localPackages = new Map()
  return records.map((record) => {
    if (Array.isArray(record.distributions)) {
      const binary = record.distributions.find((distribution) => distribution.type === 'binary')
      if (binary !== undefined) {
        let managed
        try {
          const command = managedBinaryPath({ ...record, distribution: binary }, options)
          if (command !== undefined && executableFile(command)) managed = command
        } catch {
          // Invalid registry paths are left as unavailable rather than escaping
          // the managed install root.
        }
        if (managed !== undefined) {
          return {
            id: record.id,
            label: record.label,
            version: record.version,
            description: record.description,
            command: managed,
            resolvedCommand: managed,
            args: [...binary.args],
            env: { ...binary.env },
            source: 'managed',
            status: 'Installed',
            registry: true,
            distribution: binary,
            registryRecord: record,
          }
        }
        const binaryName = path.basename(binary.command.replaceAll('\\', '/'))
        const local = resolvePathCommand(binaryName, options.pathValue, options)
        if (local !== undefined) {
          return {
            id: record.id,
            label: record.label,
            version: record.version,
            description: record.description,
            command: binaryName,
            resolvedCommand: local,
            args: [...binary.args],
            env: { ...binary.env },
            source: 'path',
            status: 'Found locally',
            registry: true,
            distribution: binary,
            registryRecord: record,
          }
        }
      }
      const packageDistributions = record.distributions.filter((distribution) => (
        distribution.type === 'npx' || distribution.type === 'uvx'
      ))
      const packageCandidates = packageDistributions
        .map((distribution) => registryPackageCandidate(record, distribution, options, localPackages))
      const localPackage = packageCandidates.find(({ status }) => status === 'Found locally')
      if (localPackage !== undefined) return localPackage
      const availablePackage = packageCandidates.find(({ status }) => status.startsWith('Available via '))
      if (availablePackage !== undefined) {
        return availablePackage
      }
      if (binary !== undefined) {
        return {
          id: record.id,
          label: record.label,
          version: record.version,
          description: record.description,
          command: binary.command,
          args: [...binary.args],
          env: { ...binary.env },
          source: 'registry',
          status: 'Available to install',
          registry: true,
          distribution: binary,
          registryRecord: record,
          installPath: (() => {
            try {
              const command = managedBinaryPath({ ...record, distribution: binary }, options)
              return command === undefined ? undefined : path.dirname(command)
            } catch { return undefined }
          })(),
        }
      }
      if (packageCandidates.length > 0) return packageCandidates[0]
      return undefined
    }
    const local = record.commands
      .map((spec) => ({
        spec,
        command: resolvePathCommand(spec.command, options.pathValue, options),
      }))
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
  }).filter(Boolean)
}

function registryCandidate(id, options = {}) {
  // Normalize the catalog, but probe local installations only for this id.
  return registryCandidates(options, registryRecords(options).filter((record) => record.id === id))[0]
}

function runnableRegistryCandidate(entry) {
  return [
    'Found locally', 'Installed', 'Available via npx', 'Available via uvx',
  ].includes(entry.status)
}

export function discoverRegistryHarnesses(options = {}) {
  return registryCandidates(options).filter(runnableRegistryCandidate)
}

function finishHarnessConfiguration(settingsPath, harness, options) {
  // Setup and runtime are independent. persist:false lets callers prepare a
  // recipe before explicitly saving it; neither path initializes an ACP session.
  return options.persist === false ? validateHarness(harness) : upsertHarness(settingsPath, harness)
}

export function addHarness(settingsPath, id, tokens = [], options = {}) {
  return configureHarness(settingsPath, id, tokens, options, () => registryCandidate(id, options))
}

function parseHarnessAdd(id, tokens) {
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
    if (token === '--command' && !value.trim()) throw new HarnessUsageError('--command needs a non-empty value')
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
  return { command, label, args }
}

function configureHarness(settingsPath, id, tokens, options, lookupCandidate) {
  const { command, label, args } = parseHarnessAdd(id, tokens)
  if (typeof command !== 'string' || command.trim().length === 0) {
    const candidate = lookupCandidate()
    if (candidate !== undefined && [
      'Found locally', 'Installed', 'Available via npx', 'Available via uvx',
    ].includes(candidate.status)) {
      return finishHarnessConfiguration(settingsPath, {
        id: candidate.id,
        label: label ?? candidate.label,
        command: candidate.resolvedCommand ?? candidate.command,
        args: args.length > 0 ? args : candidate.args,
        ...(candidate.env !== undefined ? { env: candidate.env } : {}),
      }, options)
    }
    if (candidate?.status === 'Available to install') {
      throw new HarnessUsageError(
        `Harness ${JSON.stringify(id)} is a binary distribution.\n\n`
        + `  Install into Martty with  martty harness add ${id}\n`
        + `  Location                 ${candidate.installPath ?? "Martty's private bin directory"}\n`,
      )
    }
    if (candidate?.status === 'Needs npx' || candidate?.status === 'Needs uvx') {
      const runtime = candidate.distribution.type === 'npx' ? 'Node.js/npm' : 'uv'
      throw new HarnessUsageError(
        `Harness ${JSON.stringify(id)} needs ${candidate.distribution.type}.\n\n`
        + `  Install ${runtime} to provide ${candidate.distribution.type}, then run  martty harness add ${id}\n`,
      )
    }
    if (candidate?.status === 'Not installed' && candidate.fallback !== undefined) {
      return finishHarnessConfiguration(settingsPath, {
        id: candidate.id,
        label: label ?? candidate.label,
        command: candidate.fallback.command,
        args: [
          ...candidate.fallback.args,
          ...args,
        ],
      }, options)
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
  return finishHarnessConfiguration(settingsPath, { id, label, command, args }, options)
}

/** Configure a registry entry; persist:false prepares it without writing settings. */
export async function addHarnessAsync(settingsPath, id, tokens = [], options = {}) {
  parseHarnessAdd(id, tokens)
  if (tokens.includes('--command')) return addHarness(settingsPath, id, tokens, options)
  const candidate = registryCandidate(id, options)
  if (candidate?.status !== 'Available to install') {
    return configureHarness(settingsPath, id, tokens, options, () => candidate)
  }
  // Validate every option and preserve overrides before any download can start.
  const prepared = configureHarness(settingsPath, id, tokens, { ...options, persist: false },
    () => ({ ...candidate, status: 'Installed' }))
  const installed = await installRegistryBinary({
    id: candidate.id,
    label: candidate.label,
    version: candidate.version,
    distribution: candidate.distribution,
  }, options)
  return finishHarnessConfiguration(settingsPath, {
    ...installed,
    label: prepared.label,
    args: prepared.args,
  }, options)
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
  return discoverHarnessEntries(settingsPath, options, registryCandidates(options))
}

function discoverHarnessEntries(settingsPath, options, registryEntries) {
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
  const registry = registryEntries.filter(runnableRegistryCandidate).map((entry) => ({
    ...entry,
    command: entry.resolvedCommand ?? entry.command,
  }))
  const seen = new Set()
  const seenRecipes = new Set()
  const seenCommands = new Set()
  const inferredPathEntries = new Set(pathEntries)
  return [...forced, ...configured, ...ordinaryDefaults, ...registry, ...pathEntries].filter((entry) => {
    if (seen.has(entry.id)) return false
    const command = resolvePathCommand(entry.command, options.pathValue, options) ?? entry.command
    // A bare PATH guess is not a second recipe when settings/the Registry
    // already describe this command's required ACP arguments.
    if (inferredPathEntries.has(entry) && seenCommands.has(command)) return false
    const recipe = JSON.stringify([
      command,
      entry.args ?? [],
      Object.entries(entry.env ?? {}).sort(([left], [right]) => left.localeCompare(right)),
    ])
    if (seenRecipes.has(recipe)) return false
    seen.add(entry.id)
    seenRecipes.add(recipe)
    seenCommands.add(command)
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
    entry.description,
    entry.distribution?.type,
    ...(entry.distribution?.args ?? []),
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
  const configured = configuredHarnesses(readSettings(settingsPath)).map((entry) => ({
    ...entry,
    source: 'configured',
    status: 'Configured',
  }))
  const configuredIds = new Set(configured.map(({ id }) => id))
  const registry = registryCandidates(options)
  const entries = [
    // A configured Harness is still a useful find result: it explains why a
    // machine with only saved entries must not look empty, and lets the user
    // jump straight from discovery to switching.
    ...configured,
    ...registry.filter((entry) => !configuredIds.has(entry.id)),
    ...discoverHarnessEntries(settingsPath, options, registry).filter((entry) => entry.source !== 'configured'),
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

async function resolveRegistry(options = {}) {
  if (options.registry !== undefined) return options.registry
  if (typeof options.fetchRegistry === 'function') return options.fetchRegistry(options)
  return fetchAcpRegistry(options)
}

function validateCliArgs(argv) {
  const [action, id, ...tokens] = argv
  const help = ['help', '-h', '--help']
  if (action === undefined || help.includes(action)) return
  if (!['list', 'find', 'add', 'use', 'remove'].includes(action)) throw new HarnessUsageError(`Unknown Harness command ${JSON.stringify(action)}`)
  if (id === '--help' || id === '-h') return
  if (action === 'add') {
    if (id === undefined || id === 'help' || (tokens.length === 1 && help.includes(tokens[0]))) return
    parseHarnessAdd(id, tokens)
  } else if (action === 'list' && id !== undefined) throw new HarnessUsageError('Usage: martty harness list')
  else if (action === 'use' && (!id || tokens.length)) throw new HarnessUsageError('Usage: martty harness use <id>')
  else if (action === 'find') {
    for (const token of argv.slice(1)) if (token.startsWith('-') && token !== '--refresh') throw new HarnessUsageError(`Unknown find option ${JSON.stringify(token)}`)
  } else if (action === 'remove') {
    if (!id || !HARNESS_ID.test(id) || tokens.some(token => !['--cleanup', '--yes', '--dry-run'].includes(token))) {
      throw new HarnessUsageError('Usage: martty harness remove <id> [--cleanup] [--yes] [--dry-run]')
    }
  }
}

/** CLI setup persists recipes only; runtime/session/auth belong to explicit launch. */
export async function runHarnessCommandAsync(argv, options = {}) {
  validateCliArgs(argv)
  const action = argv[0]
  if (action !== 'add' && ['--help', '-h'].includes(argv[1])) return runHarnessCommand(['help'], options)
  if (action === 'remove') {
    const { planHarnessRemoval, removeHarness } = await import('./harness-removal.js')
    const id = argv[1], cleanup = argv.includes('--cleanup')
    const plan = planHarnessRemoval(options.settingsPath, id, options)
    if (cleanup && plan.cleanupReason) throw new HarnessUsageError(plan.cleanupReason)
    const preview = `Remove ${plan.entry.label} (${id})\nConfiguration: ${options.settingsPath}\n`
      + (cleanup ? `Delete private installation:\n${plan.resources.map(resource => `  ${resource}`).join('\n')}\n` : 'Installed files will be kept.\n')
      + 'Its saved default reference will be cleared. History, credentials and shared caches are kept.\n'
    if (argv.includes('--dry-run')) return { code: 0, stdout: preview, stderr: '' }
    if (!argv.includes('--yes')) {
      if (typeof options.confirmRemoval !== 'function') return { code: 2, stdout: preview, stderr: 'Confirmation required. Rerun with --yes, or use an interactive terminal.\n' }
      if (!await options.confirmRemoval(preview)) return { code: 0, stdout: 'Removal cancelled; nothing changed.\n', stderr: '' }
    }
    options.signal?.throwIfAborted()
    const result = removeHarness(options.settingsPath, plan, { ...options, cleanup })
    return { code: 0, stdout: preview + `Removed ${id}.` + (result.removed.length ? '\nPrivate installation deleted.\n' : '\n'), stderr: '' }
  }
  const addId = argv[1]
  const addHelp = addId === undefined || ['help', '-h', '--help'].includes(addId)
    || (argv.length === 3 && ['-h', '--help'].includes(argv[2]))
  const saved = action === 'add' && savedHarnesses(options.settingsPath).find(entry => entry.id === addId)
  const needsRegistry = action === 'find'
    || (action === 'add' && !addHelp && !saved && !argv.slice(2).includes('--command'))
  if (!needsRegistry) {
    if (action === 'add' && saved && !addHelp) {
      const harness = configureHarness(options.settingsPath, addId, argv.slice(2), options, () => ({ ...saved, status: 'Installed' }))
      return { code: 0, stdout: savedHarnessOutput(harness, options.color === true), stderr: '' }
    }
    return runHarnessCommand(argv, options)
  }
  let registry = options.registry ?? readAcpRegistrySnapshot(options)
  let warning = ''
  if (options.registry === undefined && (argv.includes('--refresh')
      || (action === 'add' && !registry.some(entry => entry.id === addId)))) {
    try { registry = await resolveRegistry(options) }
    catch (error) {
      options.signal?.throwIfAborted()
      warning = `warning: Registry refresh failed; using local catalog. ${error instanceof Error ? error.message : String(error)}\n`
      if (action === 'add' && !registry.some(entry => entry.id === addId)) throw new Error(warning.trim())
    }
  }
  const scoped = { ...options, registry }
  if (action === 'add') {
    const [_, id, ...tokens] = argv
    const color = options.color === true
    const wantsAddHelp = id === undefined
      || ['help', '-h', '--help'].includes(id)
      || (tokens.length === 1 && ['-h', '--help'].includes(tokens[0]))
    if (wantsAddHelp) return runHarnessCommand(argv, scoped)
    const harness = await addHarnessAsync(options.settingsPath, id, tokens, scoped)
    return { code: 0, stdout: savedHarnessOutput(harness, color), stderr: warning }
  }
  const result = runHarnessCommand(argv.filter(token => token !== '--refresh'), scoped)
  return { ...result, stderr: warning + result.stderr }
}

export function runHarnessCommand(argv, options) {
  validateCliArgs(argv)
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

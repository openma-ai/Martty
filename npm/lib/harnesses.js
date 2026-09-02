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
  if (source === 'builtin') return 'Bundled'
  if (source === 'path') return 'PATH'
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
  list              Show saved, bundled, and PATH Harnesses
  add <id>          Save a named Harness command
  use <id>          Select a Harness for the next standalone launch
  help              Show this help

${section('Add options')}
  --command <cmd>   ACP command to launch (required)
  --label <label>   Human-readable name
  --arg <arg>       Command argument; repeat as needed

${section('Examples')}
  ${command('martty harness list')}
  ${command('martty harness add codex')}
  ${command('martty harness add local --label "Local ACP" --command local-acp --arg --stdio')}
  ${command('martty harness use local')}

Switching Harnesses takes effect on the next standalone launch and starts a
new ACP session. Sessions are never carried across Harnesses.
`
}

function addHarnessHelp(color = false) {
  const section = (value) => paint(value, 'bold', color)
  const command = (value) => paint(value, 'cyan', color)
  return `${section('Add a Harness')}

Martty connects to ACP servers, not directly to agent CLIs.

${section('Quick setup')}
  ${command('martty harness add codex')}
  Saves the Codex ACP adapter recipe. No command flags needed.

${section('Custom ACP command')}
  ${command('martty harness add <id> --command <cmd> [options]')}

${section('Options')}
  --command <cmd>   ACP server command to launch
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

function formatHarnessList(entries, active, options = {}) {
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
    const selected = entry.id === active
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
  const activeLabel = active ?? 'none (bundled default on next launch)'
  lines.push(fieldLine('Active', activeLabel, columns, color, 2))
  lines.push(fieldLine('Switch', 'martty harness use <id>', columns, color, 2))
  lines.push(fieldLine('In TUI', '/harness', columns, color, 2))
  return `${lines.join('\n')}\n`
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

export function upsertHarness(settingsPath, harness) {
  const next = validateHarness(harness)
  const settings = readSettings(settingsPath)
  const harnesses = configuredHarnesses(settings)
  const index = harnesses.findIndex(({ id }) => id === next.id)
  if (index === -1) harnesses.push(next)
  else harnesses[index] = next
  writeSettings(settingsPath, { ...settings, harnesses })
  return next
}

export function activateHarness(settingsPath, id) {
  const settings = readSettings(settingsPath)
  const harnesses = configuredHarnesses(settings)
  if (!harnesses.some((harness) => harness.id === id)) {
    throw new Error(`unknown harness ${JSON.stringify(id)}`)
  }
  writeSettings(settingsPath, { ...settings, harnesses, activeHarness: id })
}

export function selectedHarness(settingsPath) {
  const settings = readSettings(settingsPath)
  if (typeof settings.activeHarness !== 'string') return undefined
  return configuredHarnesses(settings).find(({ id }) => id === settings.activeHarness)
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
  const seen = new Set()
  return [...configured, ...defaults, ...pathEntries].filter((entry) => {
    if (seen.has(entry.id)) return false
    seen.add(entry.id)
    return true
  })
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
  if (action === 'list') {
    const settings = readSettings(settingsPath)
    const active = typeof settings.activeHarness === 'string' ? settings.activeHarness : undefined
    const entries = discoverHarnesses(settingsPath, options)
    return { code: 0, stdout: formatHarnessList(entries, active, options), stderr: '' }
  }
  if (action === 'use') {
    const harness = discoverHarnesses(settingsPath, options).find((entry) => entry.id === id)
    if (harness === undefined) throw new Error(`unknown harness ${JSON.stringify(id ?? '')}`)
    upsertHarness(settingsPath, harness)
    activateHarness(settingsPath, id)
    return {
      code: 0,
      stdout: `active harness ${id}; next standalone launch starts a new session\n`,
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
  if (!HARNESS_ID.test(id)) {
    throw new HarnessUsageError(
      `Invalid Harness id ${JSON.stringify(id)}. Use lowercase letters, numbers, and hyphens.`,
    )
  }
  if (id === 'codex' && tokens.length === 0) {
    const harness = upsertHarness(settingsPath, {
      id: 'codex',
      label: 'Codex Harness',
      command: 'npx',
      args: ['--yes', '--prefer-offline', '@agentclientprotocol/codex-acp'],
    })
    return { code: 0, stdout: savedHarnessOutput(harness, color), stderr: '' }
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
    throw new HarnessUsageError(
      `Missing --command for custom Harness ${JSON.stringify(id)}.\n\n`
      + `  martty harness add ${id} --command <cmd>\n\n`
      + 'The command must start an ACP-compatible server on stdin/stdout.',
    )
  }
  const harness = upsertHarness(settingsPath, { id, label, command, args })
  return { code: 0, stdout: savedHarnessOutput(harness, color), stderr: '' }
}

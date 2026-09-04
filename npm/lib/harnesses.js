import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'

const HARNESS_ID = /^[a-z0-9][a-z0-9-]*$/
const HARNESS_HELP = `martty harness — configure standalone ACP harnesses

USAGE:
  martty harness list
  martty harness add <id> --command <cmd> [--label <label>] [--arg <arg>]...
  martty harness use <id>

list discovers executable *-acp and *_acp commands on PATH. The active harness
is saved in $MARTTY_HOME/settings.json. The next standalone launch starts that
harness with a new ACP session; sessions are never carried across harnesses.
`

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
  if (action === 'help' || action === undefined) {
    return { code: 0, stdout: HARNESS_HELP, stderr: '' }
  }
  if (action === 'list') {
    const settings = readSettings(settingsPath)
    const active = typeof settings.activeHarness === 'string' ? settings.activeHarness : undefined
    const stdout = discoverHarnesses(settingsPath, options).map((entry) => {
      const marker = entry.id === active ? '*' : ' '
      const command = [entry.command, ...entry.args]
        .map((part) => /\s/.test(part) ? JSON.stringify(part) : part)
        .join(' ')
      return `${marker} ${entry.id}\t${entry.source}\t${entry.label}\t${command}`
    }).join('\n')
    return { code: 0, stdout: stdout.length > 0 ? `${stdout}\n` : '', stderr: '' }
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
  let command
  let label
  const args = []
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index]
    const value = tokens[index + 1]
    if (token === '--command') command = value
    else if (token === '--label') label = value
    else if (token === '--arg') args.push(value ?? '')
    else throw new Error(`unknown harness add option ${JSON.stringify(token)}`)
    index += 1
  }
  upsertHarness(settingsPath, { id, label, command, args })
  return { code: 0, stdout: `saved harness ${id}\n`, stderr: '' }
}

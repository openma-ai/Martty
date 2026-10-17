/** Resolve the embeddable ACP plugin from TUI's own dependency graph. */

import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { DSH_FLOOR, isUntested, requirementOf, satisfiesRange, untestedWarning } from './dsh-compat.js'
import { createHostNoticeServer } from './host-notice-agent.js'

const OWN_VERSION = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version

export const name = 'dsh-tui-acp-host'
export const inject = ['loader', 'userQuestions', 'permissionPresets']

function resolvedModule(ctx, specifier) {
  for (const anchor of [ctx.baseUrl, import.meta.url]) {
    if (typeof anchor !== 'string') continue
    try {
      return pathToFileURL(createRequire(anchor).resolve(specifier)).href
    } catch {
      // Try TUI's dependency graph after the active profile.
    }
  }
  return specifier
}

function resolvedHostModule(ctx, specifier) {
  if (typeof ctx.baseUrl !== 'string') return undefined
  try {
    return pathToFileURL(createRequire(ctx.baseUrl).resolve(specifier)).href
  } catch {
    return undefined
  }
}

/**
 * dsh 0.1.2 replaced the single user-question provider slot with a scoped
 * waterfall. Keep the ACP dependency's older provider seam working while it
 * remains protocol-compatible with both Host generations.
 */
export function installUserQuestionsCompatibility(ctx) {
  const service = ctx.get?.('userQuestions')
  if (service === undefined || typeof service.registerProvider === 'function') return false
  const registerProvider = (provider) => {
    if (provider === null || typeof provider !== 'object' || typeof provider.ask !== 'function') {
      throw new TypeError('userQuestions.registerProvider: provider must expose ask()')
    }
    return ctx.on('user-questions/request', (request) => provider.ask(request))
  }
  Object.defineProperty(service, 'registerProvider', {
    configurable: true,
    value: registerProvider,
  })
  ctx.effect(() => () => {
    if (service.registerProvider === registerProvider) delete service.registerProvider
  }, 'dsh-tui.user-questions-compatibility')
  return true
}

/**
 * dsh 0.1.2 replaced Session.events with snapshotEvents(). ACP 0.4.x only
 * reads the old immutable snapshot, so restore that getter until ACP can
 * require the new Host API directly.
 */
export function installSessionEventsCompatibility(Session) {
  const prototype = Session?.prototype
  if (prototype === undefined || 'events' in prototype) return false
  if (typeof prototype.snapshotEvents !== 'function') return false
  Object.defineProperty(prototype, 'events', {
    configurable: true,
    get() {
      if (typeof this.snapshotEvents === 'function') return this.snapshotEvents()
      // Cordis may hand an older plugin a remote Session reference whose
      // declared surface predates snapshotEvents(). The 0.1.2 reference still
      // exposes the scalar seq/eventAt pair, which yields the same snapshot.
      if (typeof this.eventAt === 'function' && Number.isInteger(this.seq)) {
        const events = []
        for (let seq = 0; seq < this.seq; seq += 1) {
          const event = this.eventAt(seq)
          if (event !== undefined) events.push(event)
        }
        return Object.freeze(events)
      }
      return Object.freeze([])
    },
  })
  return true
}

/** Accept ACP 0.4.x's event-array read alongside dsh 0.1.2's Session read. */
export function installPermissionPresetsCompatibility(permissionPresets) {
  if (permissionPresets === undefined) return false
  const current = permissionPresets.current
  if (typeof current !== 'function' || current.dshTuiAcceptsEventArrays === true) return false
  const compatibleCurrent = function (target) {
    if (!Array.isArray(target)) return current.call(this, target)
    let state = { preset: null, sandbox: null, approval: null, seeded: false }
    for (const event of target) {
      switch (event?.type) {
        case 'permission/preset':
          state = { ...state, preset: event.data?.preset ?? null }
          break
        case 'sandbox/mode':
          state = { ...state, sandbox: event.data?.mode ?? null }
          break
        case 'approval/policy':
          state = { ...state, approval: event.data?.policy ?? null }
          break
        case 'session/end-seed':
          state = { ...state, seeded: true }
          break
      }
    }
    return this.derive(state)
  }
  Object.defineProperty(compatibleCurrent, 'dshTuiAcceptsEventArrays', { value: true })
  Object.defineProperty(permissionPresets, 'current', {
    configurable: true,
    value: compatibleCurrent,
  })
  return true
}

function snapshotRowHeader(row) {
  if (row === null || typeof row !== 'object') return undefined
  const header = row.header
  if (header === null || typeof header !== 'object' || typeof header.id !== 'string') {
    return undefined
  }
  return header
}

/**
 * One legacy `list()` row: the header itself, plus the snapshot fields a newer
 * Host consumer reads off the same row (`header`, `revision`, sizes).
 */
function legacyListRow(snapshot) {
  const header = snapshotRowHeader(snapshot)
  if (header === undefined) return undefined
  const row = { ...header }
  for (const key of ['header', 'revision', 'eventCount', 'sizeBytes']) {
    if (snapshot[key] === undefined) continue
    Object.defineProperty(row, key, {
      configurable: true,
      value: key === 'header' ? header : snapshot[key],
    })
  }
  return row
}

/**
 * dsh 0.1.5 folded `listSnapshots()` into `list()` and replaced `inspect()`
 * with per-session handles: `list()` now resolves to `{ header, revision }`
 * snapshots, and one stored log is read through `open(id, 'read')`. ACP 0.4.x
 * still filters listed rows by the row's own `cwd` and replays titles and
 * resume history through `inspect()`, so project the removed header/inspection
 * surface back onto the live service until ACP can require the handle API.
 *
 * Rows stay snapshot-compatible, so a newer Host consumer reading `.header`
 * keeps working while ACP reads the header fields directly.
 */
export function installSessionPersistenceCompatibility(ctx) {
  const service = ctx?.get?.('sessionPersistence')
  if (service === undefined || service === null) return false
  const list = service.list
  if (typeof list !== 'function' || typeof service.open !== 'function') return false
  if (list.dshTuiReturnsHeaders === true) return false

  const compatibleList = async function (options) {
    const listed = await list.call(this, options)
    if (!Array.isArray(listed)) return listed
    return listed.map((row) => legacyListRow(row) ?? row)
  }
  Object.defineProperty(compatibleList, 'dshTuiReturnsHeaders', { value: true })

  // The class methods live on the prototype: restore by deleting the own
  // property when the patch shadowed one instead of replacing an own value.
  const listDescriptor = Object.getOwnPropertyDescriptor(service, 'list')
  Object.defineProperty(service, 'list', { configurable: true, value: compatibleList })

  let compatibleInspect
  if (typeof service.inspect !== 'function') {
    compatibleInspect = async function (id, signal) {
      const options = signal === undefined ? undefined : { signal }
      const handle = await service.open(id, 'read', options)
      try {
        const { events } = await handle.read(0, undefined, options)
        const header = handle.header
        // `meta` is the old coordinator's field; `header` is what ACP reads.
        return Object.freeze({
          meta: header,
          header,
          inheritedEventCount: handle.inheritedEventCount,
          events,
        })
      } finally {
        await handle.close().catch(() => {})
      }
    }
    Object.defineProperty(service, 'inspect', { configurable: true, value: compatibleInspect })
  }

  ctx.effect?.(() => () => {
    if (service.list !== compatibleList) return
    if (listDescriptor === undefined) delete service.list
    else Object.defineProperty(service, 'list', listDescriptor)
    if (service.inspect === compatibleInspect) delete service.inspect
  }, 'dsh-tui.session-persistence-compatibility')
  return true
}

function readManifest(anchor, specifier) {
  if (typeof anchor !== 'string') return undefined
  try {
    const file = createRequire(anchor).resolve(`${specifier}/package.json`)
    return { file, manifest: JSON.parse(readFileSync(file, 'utf8')) }
  } catch {
    return undefined
  }
}

/**
 * The hosting dsh and the ACP plugin this entry is about to mount, resolved
 * the way dsh-acp resolves them: the Host anchor is `dshAcpHostBaseUrl` or
 * the profile base; the plugin comes from the profile, then from Martty's
 * own dependency graph.
 */
export function hostCompatibility(ctx) {
  const hostBase = ctx.get?.('dshAcpHostBaseUrl')
  const host = readManifest(typeof hostBase === 'string' ? hostBase : ctx.baseUrl, '@deepseek-ai/dsh')
  let acp
  for (const anchor of [ctx.baseUrl, import.meta.url]) {
    acp = readManifest(anchor, '@openma/deepseek-harness-acp')
    if (acp !== undefined) break
  }
  const dsh = host?.manifest?.version
  const checks = [{ name: 'martty', version: OWN_VERSION, range: DSH_FLOOR }]
  const acpRange = requirementOf(acp?.manifest)
  if (acpRange !== undefined) {
    checks.push({ name: '@openma/deepseek-harness-acp', version: acp.manifest.version, range: acpRange })
  }
  const unmet = typeof dsh === 'string' ? checks.filter(({ range }) => !satisfiesRange(dsh, range)) : []
  return { dsh, dshPath: host?.file, unmet, untested: isUntested(dsh) }
}

/** The in-TUI explanation when the ACP plugin cannot run on this Host. */
export function hostNoticeMessage({ dsh, dshPath, reason, unmet = [] }) {
  const lines = ['**Martty cannot attach to this dsh profile.**', '']
  if (reason !== undefined) lines.push(reason, '')
  if (dsh !== undefined) lines.push(`Installed dsh: \`${dsh}\`${dshPath ? ` (${dshPath})` : ''}`)
  for (const { name, version, range } of unmet) {
    lines.push(`\`${name}\` ${version} supports dsh \`${range}\``)
  }
  lines.push(
    '',
    'To continue:',
    '- run Martty with its bundled harness instead: `npx martty`',
    '- or install a supported dsh: `npm install -g @deepseek-ai/dsh@latest`',
    '- or update this profile\'s plugins: `dsh plugin --profile <name> update`',
  )
  return lines.join('\n')
}

function provideHostNotice(ctx, details) {
  if (ctx.get?.('acpServer') !== undefined) return
  const message = hostNoticeMessage(details)
  process.stderr.write(`${message.replace(/\*\*|`/g, '')}\n`)
  ctx.provide?.('acpServer', createHostNoticeServer(message, { version: OWN_VERSION }))
}

// How long the ACP plugin may wait for a Host service before Martty names
// the missing capability in the TUI instead of leaving the runner pending.
export const REQUIRED_SERVICE_WAIT_MS = 10_000

async function mountHostCompatibility(ctx) {
  // The service can come from the active Host even when module resolution
  // below lands on ACP's older peer copy, so adapt the live instance directly.
  installPermissionPresetsCompatibility(ctx.permissionPresets ?? ctx.get?.('permissionPresets'))
  // Not injected here on purpose: the profile's base bundle owns persistence,
  // and `ctx.get` reads the live service without adding a mount dependency.
  installSessionPersistenceCompatibility(ctx)

  const sessionModule = resolvedHostModule(ctx, '@deepseek-ai/dsh-session')
  if (sessionModule !== undefined) {
    const exports = await ctx.loader.import(sessionModule)
    installSessionEventsCompatibility(exports.Session)
  }

  // New dsh presets opt the delegation tool into Host-owned model selection.
  // Older hosts neither export nor require this service, so resolution is
  // deliberately anchored only at the active Host profile.
  if (ctx.get?.('subagentModelSelection') !== undefined) return
  const resolved = resolvedHostModule(
    ctx,
    '@deepseek-ai/dsh-tool-subagent/model-selection-settings',
  )
  if (resolved === undefined) return
  const exports = await ctx.loader.import(resolved)
  await ctx.plugin(ctx.loader.unwrapExports(exports))
}

export async function apply(ctx, config) {
  const compatibility = hostCompatibility(ctx)
  if (compatibility.unmet.length > 0) {
    // Below a declared floor (or an older dsh-acp's exact list): show why in
    // the TUI rather than mounting a plugin that will refuse the Host.
    provideHostNotice(ctx, compatibility)
    return
  }
  if (compatibility.untested) {
    process.stderr.write(`${untestedWarning(compatibility.dsh, compatibility.dshPath)}\n`)
  }
  await mountHostCompatibility(ctx)
  installUserQuestionsCompatibility(ctx)
  const specifier = '@openma/deepseek-harness-acp/plugin'
  const resolved = resolvedModule(ctx, specifier)
  let exports
  try {
    exports = await import(resolved)
  } catch (cause) {
    throw new Error(`dsh-tui: failed to import ${specifier} from ${resolved}`, { cause })
  }
  const plugin = ctx.loader.unwrapExports(exports)
  let fiber
  try {
    fiber = ctx.plugin(plugin, config)
    await fiber
  } catch (cause) {
    provideHostNotice(ctx, { ...compatibility, reason: `The ACP plugin failed to mount: ${errorText(cause)}` })
    return
  }
  watchAcpPlugin(ctx, fiber, plugin, compatibility)
}

function errorText(error) {
  return error instanceof Error ? error.message : String(error)
}

/**
 * The ACP plugin activates asynchronously (it waits for its injected Host
 * services) and reports a refusal from its own fiber, which dsh's
 * activation report never sees. Surface either outcome in the TUI: a
 * failure names its error; a service still missing after
 * {@link REQUIRED_SERVICE_WAIT_MS} is named as the absent capability.
 */
function watchAcpPlugin(ctx, fiber, plugin, compatibility) {
  if (ctx.get?.('acpServer') !== undefined) return
  let settled = false
  fiber?.await?.().then(
    () => { settled = true },
    (error) => {
      settled = true
      provideHostNotice(ctx, { ...compatibility, reason: `The ACP plugin did not start: ${errorText(error)}` })
    },
  )
  const timer = setTimeout(() => {
    if (settled || ctx.get?.('acpServer') !== undefined) return
    const missing = (Array.isArray(plugin?.inject) ? plugin.inject : [])
      .filter((service) => ctx.get?.(service) === undefined)
    if (missing.length === 0) return
    provideHostNotice(ctx, {
      ...compatibility,
      reason: `This dsh does not provide ${missing.length === 1 ? 'a service' : 'services'} the ACP plugin requires: ${missing.map((name) => `\`${name}\``).join(', ')}.`,
    })
  }, REQUIRED_SERVICE_WAIT_MS)
  timer.unref?.()
  ctx.effect?.(() => () => clearTimeout(timer), 'martty.acp-plugin-watch')
}

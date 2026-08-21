import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import test from 'node:test'
import { Context } from '../npm/node_modules/@deepseek-ai/cordis/lib/index.js'
import { applyClientHalf } from '../npm/lib/client-run.js'
import { CORDIS_METHODS } from '../npm/lib/cordis-protocol.js'
import * as inspectPlugin from '../npm/lib/inspect.js'
import { advertiseCordis, isHostMessage, muxAcpAndCompositor } from '../npm/lib/mux.js'
import * as slotsPlugin from '../npm/lib/tui-slots.js'
import * as themePlugin from '../npm/lib/tui-theme.js'
import * as commandsPlugin from '../npm/lib/tui-commands.js'
import * as overlayPlugin from '../npm/lib/tui-overlay.js'
import * as presetsPlugin from '../npm/lib/tui-presets.js'
import * as configPlugin from '../npm/lib/acp-session-config.js'
import * as planPlugin from '../npm/lib/acp-session-plan.js'
import * as statsPlugin from '../npm/lib/acp-session-stats.js'
import * as statusPlugin from '../npm/lib/acp-session-status.js'
import * as eventsPlugin from '../npm/lib/acp-client-events.js'

const { installAcpSessionConfig } = configPlugin
const { installAcpSessionPlan } = planPlugin
const { installAcpSessionStats } = statsPlugin

const {
  attachTuiClient,
  commandInspectProvider,
  configOptionsInspectProvider,
  overlayInspectProvider,
  planInspectProvider,
  slotInspectProvider,
  statsInspectProvider,
  statusInspectProvider,
  themeInspectProvider,
  uiPresetInspectProvider,
} = inspectPlugin
const { installTuiSlots } = slotsPlugin
const { installTuiCommands } = commandsPlugin
const { installTuiPresets } = presetsPlugin
const { installTuiTheme, TOKEN_NAMES } = themePlugin
const { installTuiOverlay } = overlayPlugin

function makeCtx() {
  return {
    effect(fn) {
      return fn()
    },
    get() {},
    on() {
      return () => {}
    },
  }
}

test('Cordis client capability uses namespaced ACP _meta without dropping auth caps', () => {
  const stamped = advertiseCordis({
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: { clientCapabilities: { _meta: { 'terminal-auth': true } } },
  })
  assert.equal(stamped.params.clientCapabilities._meta['terminal-auth'], true)
  assert.deepEqual(stamped.params.clientCapabilities._meta.dsh.cordis, { protocol: 0 })
  assert.equal(stamped.params.clientCapabilities._meta.tuiInspect, undefined)
  assert.equal(advertiseCordis({ method: 'session/new' }).method, 'session/new')
})

test('Theme.listTokens reports the closed palette contract and /theme Plugin seat', () => {
  const theme = installTuiTheme(makeCtx())
  const listed = themeInspectProvider(theme).query('listTokens')
  assert.equal(listed.tokens.length, TOKEN_NAMES.length)
  assert.deepEqual(listed.tokens.map((token) => token.name), [...TOKEN_NAMES])
  assert.equal(listed.tokens[0].valueType, '#RRGGBB')
  assert.equal(listed.tokens[0].requiresLightAndDark, true)
  assert.deepEqual(listed.palettes, [{ id: 'default', label: 'Default', active: true }])
  assert.deepEqual(listed.apply.inject, ['tuiTheme'])
  assert.deepEqual(listed.apply.register.options.required, ['palette'])
  assert.equal(listed.apply.register.options.properties.activate.type, 'boolean')
  assert.deepEqual(listed.apply.register.palette.optional, ['background'])
  assert.deepEqual(listed.apply.register.palette.background.fit.enum, [
    'cover', 'contain', 'stretch',
  ])
  assert.equal(listed.apply.register.palette.background.source.variants[0].kind, 'file')
  assert.equal(listed.apply.register.returns.properties.update.role, 'update-background')
  assert.equal(listed.apply.register.returns.properties.dispose.role, 'dispose')
  assert.equal(listed.apply.register.options.properties.replace, undefined)
  assert.equal(listed.apply.activate, undefined)
  assert.equal(listed.apply.active, undefined)
  assert.equal(listed.apply.subscribe, undefined)
  assert.equal(listed.selection.command, '/theme <id>')
  assert.equal(listed.selection.unit, 'Client Plugin')
  assert.equal(listed.selection.unload, 'all Plugin contributions')
})

test('dynamic Theme Plugins can update their owned background through the registration', async () => {
  const sent = []
  const theme = installTuiTheme(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiTheme'],
      apply(ctx) {
        const registration = ctx.tuiTheme.register(${JSON.stringify({
          id: 'liang-live',
          label: 'Liang live',
          dark: Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#111111'])),
          light: Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#EEEEEE'])),
          background: {
            source: { kind: 'file', path: '/opt/liang/00.png' },
            fit: 'cover',
          },
        })}, { activate: true })
        registration.update({
          background: {
            source: { kind: 'file', path: '/opt/liang/01.png' },
            fit: 'cover',
          },
        })
      },
    }`,
    { pluginId: 'liang-plugin', tuiTheme: theme },
  )

  assert.equal(sent.at(-1).params.palette.background.source.path, '/opt/liang/01.png')
  applied.dispose()
  assert.equal(theme.active(), 'default')
})

test('Slots.list reports root shell contracts for dynamic Client plugins', () => {
  assert.equal(typeof slotInspectProvider, 'function')
  if (typeof slotInspectProvider !== 'function') return
  const slots = installTuiSlots(makeCtx())

  const listed = slotInspectProvider(slots).query('list')

  assert.deepEqual(listed.slots, [
    { name: 'welcome.hero', kind: 'single', scope: 'root', occupants: [] },
    { name: 'welcome.info', kind: 'single', scope: 'root', occupants: [] },
    { name: 'chrome.right', kind: 'list', scope: 'root', occupants: [] },
    { name: 'conversation.input.dock', kind: 'list', scope: 'session', occupants: [] },
    { name: 'conversation.composer.dock', kind: 'list', scope: 'session', occupants: [] },
  ])
  assert.deepEqual(listed.apply.inject, ['tuiSlots'])
  assert.match(listed.apply.register, /slotName/)
  assert.match(listed.apply.note, /conversation\.input\.dock/)
  assert.deepEqual(listed.runtime.inject, ['timer'])
  assert.match(listed.runtime.poll, /ctx\.interval/)
  assert.match(listed.runtime.callHost, /host\.call/)
  assert.ok(listed.nodeKinds.includes('markdown'))
  assert.ok(listed.nodeKinds.includes('terminal'))
  assert.ok(listed.nodeKinds.includes('group'))
})

test('UiPresets.list reports the composition registration contract', () => {
  assert.equal(typeof uiPresetInspectProvider, 'function')
  const commands = installTuiCommands(makeCtx())
  const presets = installTuiPresets({ tuiCommands: commands })
  presets.register({ id: 'default', label: 'Martty' }, () => () => {})

  const listed = uiPresetInspectProvider(presets).query('list')
  assert.equal(listed.active, 'default')
  assert.deepEqual(listed.presets, [{ id: 'default', label: 'Martty' }])
  assert.deepEqual(listed.apply.inject, ['tuiPresets'])
  assert.equal(listed.apply.register.call, 'ctx.tuiPresets.register({ id, label }, mount)')
  assert.match(listed.apply.register.mount, /multiple UI contributions/i)
  presets.dispose()
})

test('Commands.list reports current commands and the dynamic Client registration contract', () => {
  assert.equal(typeof commandInspectProvider, 'function')
  if (typeof commandInspectProvider !== 'function') return
  const commands = installTuiCommands(makeCtx())
  commands.register({
    name: 'threshold',
    description: 'adjust threshold',
  }, () => {})

  const listed = commandInspectProvider(commands).query('list')

  assert.deepEqual(listed.commands, [{
    name: 'threshold',
    description: 'adjust threshold',
  }])
  assert.deepEqual(listed.api.inject, ['tuiCommands'])
  assert.deepEqual(listed.api.register.options.required, ['name', 'description'])
  assert.deepEqual(
    Object.keys(listed.api.register.options.properties),
    ['name', 'description'],
  )
  assert.equal(listed.api.lifecycle.whenTheme, undefined)
  assert.deepEqual(listed.api.register.handler.arguments, [
    { name: 'args', type: 'string', default: '' },
    {
      name: 'command',
      type: 'object',
      properties: { name: { type: 'string' } },
    },
  ])
  assert.equal(listed.api.register.returns.role, 'dispose')
  assert.equal(listed.api.list.returns.items, 'Command')
})

test('Overlay.active reports the slider options, handlers, and ownership contract', () => {
  assert.equal(typeof overlayInspectProvider, 'function')
  if (typeof overlayInspectProvider !== 'function') return
  const overlay = installTuiOverlay(makeCtx())

  const described = overlayInspectProvider(overlay).query('active')

  assert.equal(described.active, null)
  assert.deepEqual(described.api.inject, ['tuiOverlay'])
  assert.deepEqual(described.api.openSlider.options.required, [
    'id', 'title', 'min', 'max', 'step', 'value',
  ])
  assert.deepEqual(
    Object.keys(described.api.openSlider.options.properties),
    ['id', 'title', 'min', 'max', 'step', 'value', 'marks', 'snapToMarks'],
  )
  assert.deepEqual(described.api.openSlider.handlers.onChange.arguments, [
    { name: 'value', type: 'number' },
  ])
  assert.deepEqual(described.api.openSlider.handlers.onSubmit.arguments.map(({ name }) => name), [
    'value', 'mark', 'controller',
  ])
  assert.deepEqual(described.api.openSlider.handlers.onCancel.arguments.map(({ name }) => name), [
    'value', 'mark', 'controller',
  ])
  assert.equal(described.api.openSlider.returns.properties.close.role, 'close')
  assert.deepEqual(described.api.openView.options.required, ['id', 'title', 'nodes'])
  assert.equal(described.api.active.returns, 'Slider | View | null')
})

test('Plans.current reports standard ACP structured plans to Client plugins', () => {
  assert.equal(typeof planInspectProvider, 'function')
  if (typeof planInspectProvider !== 'function') return
  const plans = installAcpSessionPlan(makeCtx())
  plans.observeClient({ jsonrpc: '2.0', id: 41, method: 'session/new', params: {} })
  plans.observeAgent({ jsonrpc: '2.0', id: 41, result: { sessionId: 's-41' } })
  plans.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-41',
      update: {
        sessionUpdate: 'plan',
        entries: [{ content: 'Ship', status: 'in_progress' }],
      },
    },
  })

  const described = planInspectProvider(plans).query('current')
  assert.equal(described.current.entries[0].content, 'Ship')
  assert.deepEqual(described.api.inject, ['acpSessionPlan'])
  assert.equal(described.api.subscribe.returns.role, 'dispose')
})

test('Stats.current reports the standard ACP usage and timing projection', () => {
  assert.equal(typeof statsInspectProvider, 'function')
  if (typeof statsInspectProvider !== 'function') return
  const stats = installAcpSessionStats(makeCtx(), { now: () => 0 })
  const described = statsInspectProvider(stats).query('current')
  assert.equal(described.current.usage.input, 0)
  assert.deepEqual(described.api.inject, ['acpSessionStats'])
  assert.equal(described.api.current.call, 'ctx.acpSessionStats.current()')
  assert.match(described.api.subscribe.call, /subscribe/)
})

test('Status.current reports the non-statistics ACP run-state facts', () => {
  assert.equal(typeof statusInspectProvider, 'function')
  if (typeof statusInspectProvider !== 'function') return
  const status = statusPlugin.installAcpSessionStatus(makeCtx(), {
    events: { register: () => () => {} },
    sessionConfig: {
      list: () => [],
      subscribe: () => () => {},
    },
  })
  const described = statusInspectProvider(status).query('current')
  assert.equal(described.current.state, 'idle')
  assert.equal(described.current.connection, 'connecting')
  assert.deepEqual(described.api.inject, ['acpSessionStatus'])
  assert.equal(described.api.current.call, 'ctx.acpSessionStatus.current()')
  assert.match(described.api.subscribe.call, /subscribe/)
})

test('ConfigOptions.list reports the live ACP Session options and mutation contract', () => {
  assert.equal(typeof configOptionsInspectProvider, 'function')
  if (typeof configOptionsInspectProvider !== 'function') return
  const config = installAcpSessionConfig(makeCtx())
  config.observeClient({ jsonrpc: '2.0', id: 31, method: 'session/new', params: {} })
  config.observeAgent({
    jsonrpc: '2.0',
    id: 31,
    result: {
      sessionId: 'session-31',
      configOptions: [{
        type: 'select',
        id: 'depth',
        name: 'Depth',
        category: 'thought_level',
        currentValue: 'balanced',
        options: [
          { value: 'quick', name: 'Quick' },
          { value: 'balanced', name: 'Balanced' },
        ],
      }],
    },
  })

  const listed = configOptionsInspectProvider(config).query('list')

  assert.deepEqual(listed.options, [{
    type: 'select',
    id: 'depth',
    name: 'Depth',
    category: 'thought_level',
    currentValue: 'balanced',
    options: [
      { value: 'quick', name: 'Quick' },
      { value: 'balanced', name: 'Balanced' },
    ],
  }])
  assert.deepEqual(listed.api.inject, ['acpSessionConfig'])
  assert.equal(
    listed.api.byCategory.call,
    'ctx.acpSessionConfig.byCategory(category)',
  )
  assert.deepEqual(listed.api.byCategory.standardCategories, [
    'mode', 'model', 'model_config', 'thought_level',
  ])
  assert.match(listed.api.byCategory.runtimeIdentity, /current live snapshot/i)
  assert.equal(
    listed.api.transaction.call,
    'ctx.acpSessionConfig.transaction({ category })',
  )
  assert.match(listed.api.transaction.preview, /deduplicated/i)
  assert.match(listed.api.transaction.rollback, /Plugin run/i)
  assert.match(listed.api.transaction.commit, /keep/i)
  assert.equal(listed.api.current.call, 'ctx.acpSessionConfig.current(id)')
  assert.equal(listed.api.set.transport, 'standard ACP session/set_config_option')
  assert.equal(listed.api.subscribe.returns.role, 'dispose')
  assert.match(listed.api.lifecycle.values, /current Session snapshot/)
  assert.match(listed.api.lifecycle.semanticSelection, /category/i)
  assert.match(listed.api.lifecycle.semanticSelection, /exactly one/i)
})

test('dynamic Client halves can mount, update, and dispose chrome.right content', async () => {
  const sent = []
  const slots = installTuiSlots(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })

  const applied = await applyClientHalf(
    `return {
      inject: ['tuiSlots'],
      apply(ctx) {
        const panel = ctx.tuiSlots.register(
          { name: 'chrome.right', id: 'dynamic' },
          [{ id: 'hello', kind: 'markdown', text: '**hello from plugin**' }],
        )
        panel.update([{ id: 'ready', kind: 'notice', level: 'info', text: 'ready' }])
      },
    }`,
    { tuiSlots: slots, effect: (fn) => fn() },
  )

  assert.deepEqual(sent.at(-1).params.nodes, [
    { id: 'dynamic:ready', kind: 'notice', level: 'info', text: 'ready' },
  ])
  applied.dispose()
  assert.deepEqual(sent.at(-1).params.nodes, [])
})

test('dynamic UI plugins can register a composed UI preset with owned lifecycle', async () => {
  const ctx = makeCtx()
  const slots = installTuiSlots(ctx)
  const commands = installTuiCommands(ctx)
  const presets = installTuiPresets({ tuiCommands: commands })
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiPresets', 'tuiSlots'],
      apply(ctx) {
        ctx.tuiPresets.register({ id: 'ocean', label: 'Ocean' }, () => {
          const panel = ctx.tuiSlots.register(
            { name: 'welcome.info', id: 'ocean-info' },
            [{ id: 'copy', kind: 'text', text: 'Ocean preset' }],
          )
          return () => panel.dispose()
        })
      },
    }`,
    { tuiPresets: presets, tuiSlots: slots },
  )

  assert.deepEqual(presets.list(), [{ id: 'ocean', label: 'Ocean' }])
  assert.equal(presets.active(), 'ocean')
  assert.deepEqual(
    slots.list().find((slot) => slot.name === 'welcome.info').occupants,
    [{ id: 'ocean-info', order: 0 }],
  )
  applied.dispose()
  assert.deepEqual(presets.list(), [])
  assert.deepEqual(
    slots.list().find((slot) => slot.name === 'welcome.info').occupants,
    [],
  )
  presets.dispose()
})

test('dynamic Client commands and overlays share the Plugin lifecycle', async () => {
  const commands = installTuiCommands(makeCtx())
  const overlay = installTuiOverlay(makeCtx())
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiCommands', 'tuiOverlay'],
      apply(ctx) {
        ctx.tuiCommands.register(
          { name: 'threshold', description: 'adjust threshold' },
          () => ctx.tuiOverlay.openSlider({
            id: 'threshold', title: 'Threshold', min: 0, max: 100, step: 5, value: 40,
          }),
        )
      },
    }`,
    { tuiCommands: commands, tuiOverlay: overlay },
  )

  assert.deepEqual(commands.list().map((command) => command.name), ['threshold'])
  await commands.dispatch({ protocol: 0, name: 'threshold', args: '' })
  assert.equal(overlay.active().id, 'threshold')

  applied.dispose()
  assert.deepEqual(commands.list(), [])
  assert.equal(overlay.active(), null)
})

test('dynamic Client halves can read ACP plans and open a generic node view', async () => {
  const plans = installAcpSessionPlan(makeCtx())
  const overlay = installTuiOverlay(makeCtx())
  plans.observeClient({ jsonrpc: '2.0', id: 51, method: 'session/new', params: {} })
  plans.observeAgent({ jsonrpc: '2.0', id: 51, result: { sessionId: 's-51' } })
  plans.observeAgent({
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 's-51',
      update: { sessionUpdate: 'plan', entries: [{ content: 'Inspect', status: 'pending' }] },
    },
  })

  const applied = await applyClientHalf(
    `return {
      inject: ['acpSessionPlan', 'tuiOverlay'],
      apply(ctx) {
        const plan = ctx.acpSessionPlan.current()
        ctx.tuiOverlay.openView({
          id: 'plan', title: 'Plan',
          nodes: plan.entries.map((entry, index) => ({
            id: String(index), kind: 'generic', title: entry.content, body: '',
          })),
        })
      },
    }`,
    { acpSessionPlan: plans, tuiOverlay: overlay },
  )

  assert.equal(overlay.active().kind, 'view')
  assert.equal(overlay.active().nodes[0].title, 'Inspect')
  applied.dispose()
  assert.equal(overlay.active(), null)
})

test('dynamic Client halves can subscribe to ACP session stats', async () => {
  const stats = installAcpSessionStats(makeCtx(), { now: () => 0 })
  let observed
  const applied = await applyClientHalf(
    `return {
      inject: ['acpSessionStats'],
      apply(ctx) {
        globalThis.__dshStatsCurrent = ctx.acpSessionStats.current()
        ctx.acpSessionStats.subscribe((snapshot) => {
          globalThis.__dshStatsLatest = snapshot
        })
      },
    }`,
    { acpSessionStats: stats },
  )
  observed = globalThis.__dshStatsCurrent
  assert.equal(observed.usage.input, 0)
  stats.observeClient({ jsonrpc: '2.0', id: 1, method: 'session/new', params: {} })
  stats.observeAgent({ jsonrpc: '2.0', id: 1, result: { sessionId: 's-1' } })
  assert.equal(globalThis.__dshStatsLatest.sessionId, 's-1')
  applied.dispose()
  delete globalThis.__dshStatsCurrent
  delete globalThis.__dshStatsLatest
})

test('/theme replaces one dynamic Client Plugin with another as a single lifecycle', async () => {
  const theme = installTuiTheme(makeCtx())
  const commands = installTuiCommands(makeCtx())
  const colors = Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#224466']))
  const code = (id) => `return {
    inject: ['tuiTheme', 'tuiCommands'],
    apply(ctx) {
      ctx.tuiTheme.register(${JSON.stringify({
        id: 'PLUGIN_ID', label: 'PLUGIN_LABEL', dark: colors, light: colors,
      }).replaceAll('PLUGIN_ID', id).replaceAll('PLUGIN_LABEL', id.toUpperCase())}, { activate: true })
      ctx.tuiCommands.register(
        { name: '${id}-action', description: '${id} action' },
        () => {},
      )
    },
  }`
  const sources = { a: code('a'), b: code('b') }
  const lifecycle = []
  let client
  const requestAgent = async (method, params = {}) => {
    if (method === CORDIS_METHODS.inspectSync) return { ok: true }
    if (method === CORDIS_METHODS.getClientCode) return { code: sources[params.pluginId] }
    if (method === CORDIS_METHODS.settleUserRun) return { ok: true }
    if (method === CORDIS_METHODS.pluginStop) {
      lifecycle.push(`stop:${params.pluginId}`)
      await client.onHost({
        method: CORDIS_METHODS.pluginRetract,
        params: { pluginId: params.pluginId },
      })
      return { ok: true }
    }
    if (method === CORDIS_METHODS.pluginStart) {
      lifecycle.push(`start:${params.pluginId}`)
      await client.onHost({
        method: CORDIS_METHODS.userRun,
        params: {
          agentId: params.agentId,
          pluginId: params.pluginId,
          packageId: `pkg-${params.pluginId}`,
          pluginRunId: `run-${params.pluginId}-restored`,
          startedHere: true,
          mode: 'run',
          hasClientHalf: true,
        },
      })
      return { ok: true }
    }
    throw new Error(`unexpected request ${method}`)
  }
  client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    tuiCommands: commands,
    requestAgent,
  })
  const mount = (pluginId) => client.onHost({
    method: CORDIS_METHODS.userRun,
    params: {
      agentId: 'agent-1',
      pluginId,
      packageId: `pkg-${pluginId}`,
      pluginRunId: `run-${pluginId}`,
      startedHere: true,
      mode: 'run',
      hasClientHalf: true,
    },
  })

  await mount('b')
  await client.onHost({ method: CORDIS_METHODS.pluginRetract, params: { pluginId: 'b' } })
  await mount('a')
  assert.equal(theme.active(), 'a')
  assert.deepEqual(commands.list().map((command) => command.name), ['a-action'])

  await client.selectTheme({ protocol: 0, agentId: 'agent-1', id: 'b' })

  assert.deepEqual(lifecycle, ['start:b', 'stop:a'])
  assert.equal(theme.active(), 'b')
  assert.deepEqual(commands.list().map((command) => command.name), ['b-action'])
})

test('/theme keeps the current Plugin intact when the target Plugin cannot start', async () => {
  const theme = installTuiTheme(makeCtx())
  const commands = installTuiCommands(makeCtx())
  const colors = Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#224466']))
  const code = (id) => `return {
    inject: ['tuiTheme', 'tuiCommands'],
    apply(ctx) {
      ctx.tuiTheme.register(${JSON.stringify({
        id: 'PLUGIN_ID', label: 'PLUGIN_LABEL', dark: colors, light: colors,
      }).replaceAll('PLUGIN_ID', id).replaceAll('PLUGIN_LABEL', id.toUpperCase())}, { activate: true })
      ctx.tuiCommands.register(
        { name: '${id}-action', description: '${id} action' },
        () => {},
      )
    },
  }`
  const sources = { a: code('a'), b: code('b') }
  const lifecycle = []
  let client
  const requestAgent = async (method, params = {}) => {
    if (method === CORDIS_METHODS.inspectSync) return { ok: true }
    if (method === CORDIS_METHODS.getClientCode) return { code: sources[params.pluginId] }
    if (method === CORDIS_METHODS.settleUserRun) return { ok: true }
    if (method === CORDIS_METHODS.pluginStop) {
      lifecycle.push(`stop:${params.pluginId}`)
      await client.onHost({
        method: CORDIS_METHODS.pluginRetract,
        params: { pluginId: params.pluginId },
      })
      return { ok: true }
    }
    if (method === CORDIS_METHODS.pluginStart) {
      lifecycle.push(`start:${params.pluginId}`)
      return { ok: false, message: 'target Plugin failed to start' }
    }
    throw new Error(`unexpected request ${method}`)
  }
  client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    tuiCommands: commands,
    requestAgent,
  })
  const mount = (pluginId) => client.onHost({
    method: CORDIS_METHODS.userRun,
    params: {
      agentId: 'agent-1',
      pluginId,
      packageId: `pkg-${pluginId}`,
      pluginRunId: `run-${pluginId}`,
      startedHere: true,
      mode: 'run',
      hasClientHalf: true,
    },
  })

  await mount('b')
  await client.onHost({ method: CORDIS_METHODS.pluginRetract, params: { pluginId: 'b' } })
  await mount('a')

  await assert.rejects(
    client.selectTheme({ protocol: 0, agentId: 'agent-1', id: 'b' }),
    /target Plugin failed to start/,
  )

  assert.deepEqual(lifecycle, ['start:b'])
  assert.equal(theme.active(), 'a')
  assert.deepEqual(commands.list().map((command) => command.name), ['a-action'])
})

test('previewing a Theme Plugin replaces the previously selected Plugin lifecycle', async () => {
  const theme = installTuiTheme(makeCtx())
  const commands = installTuiCommands(makeCtx())
  const colors = Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#224466']))
  const code = (id) => `return {
    inject: ['tuiTheme', 'tuiCommands'],
    apply(ctx) {
      ctx.tuiTheme.register(${JSON.stringify({
        id: 'PLUGIN_ID', label: 'PLUGIN_LABEL', dark: colors, light: colors,
      }).replaceAll('PLUGIN_ID', id).replaceAll('PLUGIN_LABEL', id.toUpperCase())}, { activate: true })
      ctx.tuiCommands.register(
        { name: '${id}-action', description: '${id} action' },
        () => {},
      )
    },
  }`
  const sources = { a: code('a'), b: code('b') }
  const lifecycle = []
  let client
  const requestAgent = async (method, params = {}) => {
    if (method === CORDIS_METHODS.inspectSync) return { ok: true }
    if (method === CORDIS_METHODS.getClientCode) return { code: sources[params.pluginId] }
    if (method === CORDIS_METHODS.settleUserRun) return { ok: true }
    if (method === CORDIS_METHODS.pluginStop) {
      lifecycle.push(`stop:${params.pluginId}`)
      await client.onHost({
        method: CORDIS_METHODS.pluginRetract,
        params: { pluginId: params.pluginId },
      })
      return { ok: true }
    }
    throw new Error(`unexpected request ${method}`)
  }
  client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    tuiCommands: commands,
    requestAgent,
  })
  const preview = (pluginId) => client.onHost({
    method: CORDIS_METHODS.userRun,
    params: {
      agentId: 'agent-1',
      pluginId,
      packageId: `pkg-${pluginId}`,
      pluginRunId: `run-${pluginId}`,
      startedHere: true,
      mode: 'preview',
      hasClientHalf: true,
    },
  })

  await preview('a')
  await preview('b')

  assert.deepEqual(lifecycle, ['stop:a'])
  assert.equal(theme.active(), 'b')
  assert.deepEqual(commands.list().map((command) => command.name), ['b-action'])
})

test('a dynamic theme registration claims the /theme Plugin seat without an activation hint', async () => {
  const theme = installTuiTheme(makeCtx())
  const commands = installTuiCommands(makeCtx())
  const colors = Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#224466']))
  let client
  const requestAgent = async (method) => {
    if (method === CORDIS_METHODS.inspectSync) return { ok: true }
    if (method === CORDIS_METHODS.getClientCode) {
      return {
        code: `return {
          inject: ['tuiTheme', 'tuiCommands'],
          apply(ctx) {
            ctx.tuiTheme.register(${JSON.stringify({
              id: 'smoke', label: 'Smoke', dark: colors, light: colors,
            })})
            ctx.tuiCommands.register(
              { name: 'smoke-test', description: 'Smoke test' },
              () => {},
            )
          },
        }`,
      }
    }
    if (method === CORDIS_METHODS.settleUserRun) return { ok: true }
    if (method === CORDIS_METHODS.pluginStop) {
      await client.onHost({
        method: CORDIS_METHODS.pluginRetract,
        params: { pluginId: 'smoke-plugin' },
      })
      return { ok: true }
    }
    throw new Error(`unexpected request ${method}`)
  }
  client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    tuiCommands: commands,
    requestAgent,
  })

  await client.onHost({
    method: CORDIS_METHODS.userRun,
    params: {
      agentId: 'agent-1',
      pluginId: 'smoke-plugin',
      packageId: 'smoke-package',
      pluginRunId: 'smoke-run',
      startedHere: true,
      mode: 'run',
      hasClientHalf: true,
    },
  })

  assert.equal(theme.active(), 'smoke')
  assert.deepEqual(commands.list().map((command) => command.name), ['smoke-test'])

  await client.selectTheme({ protocol: 0, agentId: 'agent-1', id: 'default' })

  assert.equal(theme.active(), 'default')
  assert.deepEqual(commands.list(), [])
})

test('a failed Theme Plugin replacement rolls back the new Plugin as one lifecycle', async () => {
  const theme = installTuiTheme(makeCtx())
  const commands = installTuiCommands(makeCtx())
  const colors = Object.fromEntries(TOKEN_NAMES.map((name) => [name, '#224466']))
  const code = (id) => `return {
    inject: ['tuiTheme', 'tuiCommands'],
    apply(ctx) {
      ctx.tuiTheme.register(${JSON.stringify({
        id: 'PLUGIN_ID', label: 'PLUGIN_LABEL', dark: colors, light: colors,
      }).replaceAll('PLUGIN_ID', id).replaceAll('PLUGIN_LABEL', id.toUpperCase())}, { activate: true })
      ctx.tuiCommands.register(
        { name: '${id}-action', description: '${id} action' },
        () => {},
      )
    },
  }`
  const sources = { a: code('a'), b: code('b') }
  const resolutions = []
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    tuiCommands: commands,
    requestAgent: async (method, params = {}) => {
      if (method === CORDIS_METHODS.inspectSync) return { ok: true }
      if (method === CORDIS_METHODS.getClientCode) return { code: sources[params.pluginId] }
      if (method === CORDIS_METHODS.pluginStop) {
        return { ok: false, message: 'previous Plugin refused to stop' }
      }
      if (method === CORDIS_METHODS.settleUserRun) {
        resolutions.push(params.resolution)
        return { ok: true }
      }
      throw new Error(`unexpected request ${method}`)
    },
  })
  const preview = (pluginId) => client.onHost({
    method: CORDIS_METHODS.userRun,
    params: {
      agentId: 'agent-1',
      pluginId,
      packageId: `pkg-${pluginId}`,
      pluginRunId: `run-${pluginId}`,
      startedHere: true,
      mode: 'preview',
      hasClientHalf: true,
    },
  })

  await preview('a')
  await preview('b')

  assert.equal(resolutions.at(-1)?.ok, false)
  assert.equal(theme.active(), 'a')
  assert.deepEqual(commands.list().map((command) => command.name), ['a-action'])
})

test('dynamic Theme Plugins cannot bypass the /theme Plugin replacement seat', async () => {
  const theme = installTuiTheme(makeCtx())
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiTheme'],
      apply(ctx) {
        for (const method of ['activate', 'active', 'subscribe']) {
          if (ctx.tuiTheme[method] !== undefined) {
            throw new Error('dynamic tuiTheme unexpectedly exposes ' + method)
          }
        }
      },
    }`,
    { tuiTheme: theme },
  )

  applied.dispose()
})

test('Cordis run injects command and overlay services into a dynamic Client half', async () => {
  const commands = installTuiCommands(makeCtx())
  const overlay = installTuiOverlay(makeCtx())
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: installTuiTheme(makeCtx()),
    tuiSlots: installTuiSlots(makeCtx()),
    tuiCommands: commands,
    tuiOverlay: overlay,
    requestAgent(method) {
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-controls' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({
          code: `return {
            inject: ['tuiCommands', 'tuiOverlay'],
            apply(ctx) {
              ctx.tuiCommands.register(
                { name: 'threshold', description: 'adjust threshold' },
                () => ctx.tuiOverlay.openSlider({
                  id: 'threshold', title: 'Threshold', min: 0, max: 100, step: 5, value: 40,
                }),
              )
            },
          }`,
        })
      }
      return Promise.resolve({ accepted: true })
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-controls',
      agentId: 'agent-1',
      pluginId: 'controls-1',
      packageId: 'pkg-controls',
      mode: 'run',
    },
  })

  assert.deepEqual(commands.list().map((command) => command.name), ['threshold'])
  await commands.dispatch({ protocol: 0, name: 'threshold', args: '' })
  assert.equal(overlay.active().id, 'threshold')
  client.dispose()
  assert.deepEqual(commands.list(), [])
  assert.equal(overlay.active(), null)
})

test('dynamic Client host.call invokes its matching Host run and renders the result', async () => {
  const sent = []
  const calls = []
  const slots = installTuiSlots(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: installTuiTheme(makeCtx()),
    tuiSlots: slots,
    requestAgent(method, params) {
      calls.push({ method, params })
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-state' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({
          code: `return {
            inject: ['tuiSlots'],
            async apply(ctx) {
              const state = await host.call('read-state', { key: 'status' })
              ctx.tuiSlots.register(
                { name: 'chrome.right', id: 'state' },
                [{ id: 'value', kind: 'notice', level: 'info', text: state.value }],
              )
            },
          }`,
        })
      }
      if (method === '_dsh/cordis/plugin/invoke') {
        return Promise.resolve({ ok: true, value: { value: 'ready' } })
      }
      return Promise.resolve({ accepted: true })
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-state',
      agentId: 'agent-1',
      pluginId: 'state-1',
      packageId: 'pkg-state',
      mode: 'run',
    },
  })

  const invoked = calls.find((call) => call.method === '_dsh/cordis/plugin/invoke')
  assert.deepEqual(invoked.params, {
    agentId: 'agent-1',
    pluginId: 'state-1',
    pluginRunId: 'run-state',
    method: 'read-state',
    args: { key: 'status' },
  })
  assert.deepEqual(sent.at(-1).params.nodes, [
    { id: 'state:value', kind: 'notice', level: 'info', text: 'ready' },
  ])
})

test('dynamic Client timer polls the Host and stops with its Plugin run', async () => {
  let invokes = 0
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: installTuiTheme(makeCtx()),
    requestAgent(method) {
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-poll' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({
          code: `return {
            inject: ['timer'],
            apply(ctx) {
              const refresh = () => host.call('read-state')
              void refresh()
              ctx.interval(() => void refresh(), 10)
            },
          }`,
        })
      }
      if (method === '_dsh/cordis/plugin/invoke') {
        invokes += 1
        return Promise.resolve({ ok: true, value: null })
      }
      return Promise.resolve({ accepted: true })
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-poll',
      agentId: 'agent-1',
      pluginId: 'poll-1',
      packageId: 'pkg-poll',
      mode: 'run',
    },
  })
  await new Promise((resolve) => setTimeout(resolve, 45))
  assert.ok(invokes >= 2, `expected repeated Host calls, received ${invokes}`)

  client.onHost({
    method: '_dsh/cordis/plugins/retract',
    params: { pluginId: 'poll-1', pluginRunId: 'run-poll' },
  })
  const stoppedAt = invokes
  await new Promise((resolve) => setTimeout(resolve, 30))
  assert.equal(invokes, stoppedAt)
})

test('restoring a stopped plugin uses the Web direct user-run lifecycle', async () => {
  const calls = []
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: installTuiTheme(makeCtx()),
    tuiSlots: installTuiSlots(makeCtx()),
    requestAgent(method, params) {
      calls.push({ method, params })
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-restored' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({ code: `return { inject: ['tuiSlots'], apply() {} }` })
      }
      if (method === '_dsh/cordis/run/settle') return Promise.resolve({ ok: true })
      return Promise.resolve(null)
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/user',
    params: {
      agentId: 'agent-1',
      pluginId: 'panel-1',
      packageId: 'pkg-current',
      pluginRunId: 'run-restored',
      mode: 'run',
      hasClientHalf: true,
    },
  })

  assert.equal(calls.some((call) => call.method === '_dsh/cordis/run/host'), false)
  assert.deepEqual(calls.find((call) => call.method === '_dsh/cordis/run/settle').params, {
    agentId: 'agent-1',
    pluginId: 'panel-1',
    resolution: { ok: true, pluginRunId: 'run-restored', waitingFor: [] },
  })
  assert.equal(calls.some((call) => call.method === '_dsh/cordis/run/resolve'), false)
})

test('a failed Client attach does not retract a Host run this page did not start', async () => {
  const calls = []
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: installTuiTheme(makeCtx()),
    tuiSlots: installTuiSlots(makeCtx()),
    requestAgent(method, params) {
      calls.push({ method, params })
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/client-code') return Promise.resolve({ code: 'return {' })
      if (method === '_dsh/cordis/run/settle') return Promise.resolve({ ok: false })
      return Promise.resolve(null)
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/user',
    params: {
      agentId: 'agent-1',
      pluginId: 'panel-1',
      packageId: 'pkg-current',
      pluginRunId: 'run-existing',
      startedHere: false,
      mode: 'run',
      hasClientHalf: true,
    },
  })

  const resolution = calls.find((call) => call.method === '_dsh/cordis/run/settle').params.resolution
  assert.equal(resolution.ok, false)
  assert.equal(resolution.reason, 'client-half-failed')
  assert.equal(resolution.pluginRunId, 'run-existing')
  assert.equal(resolution.startedHere, false)
})

test('the TUI Cordis Client runner mounts as a sibling service and answers Client inspect', async () => {
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  await ctx.plugin(slotsPlugin)
  await ctx.plugin(commandsPlugin)
  await ctx.plugin(presetsPlugin)
  await ctx.plugin(overlayPlugin)
  await ctx.plugin(configPlugin)
  await ctx.plugin(planPlugin)
  await ctx.plugin(statsPlugin)
  await ctx.plugin(eventsPlugin)
  await ctx.plugin(statusPlugin)
  await ctx.plugin(inspectPlugin)

  const runner = ctx.get('tuiCordisClientRunner')
  assert.equal(typeof runner?.bindTransport, 'function')
  assert.equal(typeof runner?.onHost, 'function')
  assert.equal(typeof runner?.sync, 'function')

  const calls = []
  runner.bindTransport((method, params) => {
    calls.push({ method, params })
    return Promise.resolve({ accepted: true })
  })
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(calls, [])
  runner.sync()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(calls[0].method, '_dsh/cordis/inspect/sync')
  assert.deepEqual(calls[0].params.providers.map((provider) => provider.id), [
    'Theme', 'UiPresets', 'Slots', 'Commands', 'Overlay', 'ConfigOptions', 'Plans', 'Stats', 'Status',
  ])

  await runner.onHost({
    method: '_dsh/cordis/inspect/query',
    params: { requestId: 'inspect-plugin-1', agentId: 'agent-1', provider: 'Theme', method: 'listTokens' },
  })
  const resolved = calls.find((call) => call.method === '_dsh/cordis/inspect/resolve')
  assert.equal(resolved.params.resolution.ok, true)
  assert.equal(resolved.params.resolution.data.tokens.length, TOKEN_NAMES.length)
})

test('runner waits for sibling ACP Session services and opens them to dynamic Client halves', async () => {
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  await ctx.plugin(slotsPlugin)
  await ctx.plugin(commandsPlugin)
  await ctx.plugin(presetsPlugin)
  await ctx.plugin(overlayPlugin)
  const inspectFiber = ctx.plugin(inspectPlugin)
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(
    ctx.get('tuiCordisClientRunner'),
    undefined,
    'the inspect runner must not start before the sibling ACP Session service exists',
  )
  await ctx.plugin(configPlugin)
  assert.equal(
    ctx.get('tuiCordisClientRunner'),
    undefined,
    'the inspect runner must also wait for standard ACP Plan state',
  )
  await ctx.plugin(planPlugin)
  assert.equal(
    ctx.get('tuiCordisClientRunner'),
    undefined,
    'the inspect runner must also wait for standard ACP Session statistics',
  )
  await ctx.plugin(statsPlugin)
  assert.equal(
    ctx.get('tuiCordisClientRunner'),
    undefined,
    'the inspect runner must also wait for the run-state status service',
  )
  await ctx.plugin(eventsPlugin)
  await ctx.plugin(statusPlugin)
  await inspectFiber
  const config = ctx.get('acpSessionConfig')
  config.observeClient({ jsonrpc: '2.0', id: 71, method: 'session/new', params: {} })
  config.observeAgent({
    jsonrpc: '2.0',
    id: 71,
    result: {
      sessionId: 'session-71',
      configOptions: [{
        type: 'select',
        id: 'depth',
        name: 'Depth',
        currentValue: 'balanced',
        options: [
          { value: 'quick', name: 'Quick' },
          { value: 'balanced', name: 'Balanced' },
        ],
      }],
    },
  })

  const calls = []
  const runner = ctx.get('tuiCordisClientRunner')
  runner.bindTransport((method, params) => {
    calls.push({ method, params })
    if (method === CORDIS_METHODS.inspectSync) return Promise.resolve({ ok: true })
    if (method === CORDIS_METHODS.runHost) {
      return Promise.resolve({ ok: true, pluginRunId: 'run-config' })
    }
    if (method === CORDIS_METHODS.getClientCode) {
      return Promise.resolve({
        code: `return {
          inject: ['acpSessionConfig'],
          apply(ctx) {
            if (ctx.acpSessionConfig.current('depth') !== 'balanced') {
              throw new Error('current Session config was not injected')
            }
          },
        }`,
      })
    }
    return Promise.resolve({ accepted: true })
  })
  runner.sync()
  await new Promise((resolve) => setTimeout(resolve, 0))

  assert.deepEqual(calls[0].params.providers.map((provider) => provider.id), [
    'Theme', 'UiPresets', 'Slots', 'Commands', 'Overlay', 'ConfigOptions', 'Plans', 'Stats', 'Status',
  ])

  await runner.onHost({
    method: CORDIS_METHODS.inspectQuery,
    params: {
      requestId: 'inspect-config',
      agentId: 'agent-1',
      provider: 'ConfigOptions',
      method: 'list',
    },
  })
  const inspected = calls.find((call) => (
    call.method === CORDIS_METHODS.inspectResolve
    && call.params.requestId === 'inspect-config'
  ))
  assert.equal(inspected.params.resolution.ok, true)
  assert.equal(inspected.params.resolution.data.options[0].id, 'depth')

  await runner.onHost({
    method: CORDIS_METHODS.requestRun,
    params: {
      requestId: 'approval-config',
      agentId: 'agent-1',
      pluginId: 'config-1',
      packageId: 'pkg-config',
      mode: 'run',
    },
  })
  const resolved = calls.find((call) => (
    call.method === CORDIS_METHODS.resolveRequestRun
    && call.params.requestId === 'approval-config'
  ))
  assert.deepEqual(resolved.params.resolution, {
    ok: true,
    pluginRunId: 'run-config',
    waitingFor: [],
  })
})

test('disposing the sibling runner unloads its running Client halves', async () => {
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  await ctx.plugin(slotsPlugin)
  await ctx.plugin(commandsPlugin)
  await ctx.plugin(presetsPlugin)
  await ctx.plugin(overlayPlugin)
  await ctx.plugin(configPlugin)
  await ctx.plugin(planPlugin)
  await ctx.plugin(statsPlugin)
  await ctx.plugin(eventsPlugin)
  await ctx.plugin(statusPlugin)
  const fiber = ctx.plugin(inspectPlugin)
  await fiber
  const runner = ctx.get('tuiCordisClientRunner')
  runner.bindTransport((method) => {
    if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
    if (method === '_dsh/cordis/run/host') {
      return Promise.resolve({ ok: true, pluginRunId: 'run-fiber' })
    }
    if (method === '_dsh/cordis/run/client-code') {
      return Promise.resolve({
        code: `return {
          inject: ['tuiTheme'],
          apply(ctx) {
            ctx.tuiTheme.register({
              id: 'fiber-owned',
              label: 'Fiber owned',
              dark: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#221100'])),
              light: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#FFEEDD'])),
            }, { activate: true })
          },
        }`,
      })
    }
    return Promise.resolve({ accepted: true })
  })

  await runner.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-fiber',
      agentId: 'agent-1',
      pluginId: 'fiber-1',
      packageId: 'pkg-fiber',
      mode: 'run',
    },
  })
  assert.equal(ctx.tuiTheme.active(), 'fiber-owned')

  await fiber.dispose()
  assert.equal(ctx.tuiTheme.active(), 'default')
  assert.deepEqual(ctx.tuiTheme.list(), [
    { id: 'default', label: 'Default' },
    { id: 'fiber-owned', label: 'Fiber owned' },
  ])
  assert.equal(ctx.tuiTheme.isLoaded('fiber-owned'), false)
})

test('approved code.client mounts as a child Cordis fiber', async () => {
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  await ctx.plugin(slotsPlugin)
  await ctx.plugin(commandsPlugin)
  await ctx.plugin(presetsPlugin)
  await ctx.plugin(overlayPlugin)
  await ctx.plugin(configPlugin)
  await ctx.plugin(planPlugin)
  await ctx.plugin(statsPlugin)
  await ctx.plugin(eventsPlugin)
  await ctx.plugin(statusPlugin)
  await ctx.plugin(inspectPlugin)
  const mounted = []
  ctx.on('internal/plugin', (fiber) => mounted.push(fiber.name))
  const runner = ctx.get('tuiCordisClientRunner')
  runner.bindTransport((method) => {
    if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
    if (method === '_dsh/cordis/run/host') {
      return Promise.resolve({ ok: true, pluginRunId: 'run-child' })
    }
    if (method === '_dsh/cordis/run/client-code') {
      return Promise.resolve({
        code: `return { inject: ['tuiTheme'], apply() {} }`,
      })
    }
    return Promise.resolve({ accepted: true })
  })

  await runner.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-child',
      agentId: 'agent-1',
      pluginId: 'dyn-child',
      packageId: 'pkg-child',
      mode: 'run',
    },
  })

  assert.equal(mounted.includes('tui-dynamic:dyn-child'), true)
})

test('mux keeps inspect extras off the painter and round-trips host requests', async () => {
  const agentIn = new PassThrough()
  const agentOut = new PassThrough()
  const tuiIn = new PassThrough()
  const tuiOut = new PassThrough()
  const toAgent = []
  const toTui = []
  const host = []
  agentIn.on('data', (chunk) => toAgent.push(String(chunk)))
  tuiOut.on('data', (chunk) => toTui.push(String(chunk)))

  const mux = muxAcpAndCompositor({
    agent: { stdin: agentIn, stdout: agentOut },
    tui: { input: tuiIn, output: tuiOut },
    onHost(message) {
      host.push(message)
    },
  })

  assert.equal(isHostMessage({ method: '_dsh/cordis/inspect/query' }), true)
  assert.equal(isHostMessage({ method: '_dsh/cordis/run/user' }), true)
  assert.equal(isHostMessage({ method: '_dsh/cordis/tui/theme/update' }), false)

  tuiIn.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} })}\n`)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', method: '_dsh/cordis/inspect/query', params: { requestId: 'too-early' } })}\n`)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 's' } })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))
  assert.equal(host.some((message) => message.params?.requestId === 'too-early'), false)
  agentOut.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    result: {
      protocolVersion: 1,
      agentCapabilities: { _meta: { dsh: { cordis: { protocol: 0 } } } },
    },
  })}\n`)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', method: '_dsh/cordis/inspect/query', params: { requestId: 'q1' } })}\n`)
  await new Promise((resolve) => setTimeout(resolve, 10))

  const result = mux.requestAgent('_dsh/cordis/inspect/sync', { providers: [] })
  await new Promise((resolve) => setTimeout(resolve, 20))
  const syncLine = toAgent.find((line) => line.includes('_dsh/cordis/inspect/sync'))
  assert.ok(syncLine)
  const sync = JSON.parse(syncLine)
  agentOut.write(`${JSON.stringify({ jsonrpc: '2.0', id: sync.id, result: { ok: true } })}\n`)
  assert.deepEqual(await result, { ok: true })

  await new Promise((resolve) => setTimeout(resolve, 20))
  const initialized = toAgent
    .flatMap((line) => line.trim().split('\n').filter(Boolean))
    .map((line) => JSON.parse(line))
    .find((message) => message.method === 'initialize')
  assert.deepEqual(initialized.params.clientCapabilities._meta.dsh.cordis, { protocol: 0 })
  assert.equal(initialized.params.clientCapabilities._meta.tuiInspect, undefined)
  assert.equal(toTui.some((line) => line.includes('_dsh/cordis/inspect/query')), false)
  assert.equal(toTui.some((line) => line.includes('session/update')), true)
  assert.equal(host.some((message) => message.method === '_dsh/cordis/inspect/query'), true)
})

test('attachTuiClient answers Theme.listTokens over inspect-resolve', async () => {
  const calls = []
  const theme = installTuiTheme(makeCtx())
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    requestAgent(method, params) {
      calls.push({ method, params })
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/inspect/resolve') return Promise.resolve({ accepted: true })
      return Promise.resolve(null)
    },
  })
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(calls, [], 'binding the transport must not speak Cordis before negotiation')
  client.sync()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(calls[0].method, '_dsh/cordis/inspect/sync')
  assert.equal(calls[0].params.providers[0].id, 'Theme')

  await client.onHost({
    method: '_dsh/cordis/inspect/query',
    params: { requestId: 'inspect-1', agentId: 'agent-1', provider: 'Theme', method: 'listTokens' },
  })
  const resolve = calls.find((call) => call.method === '_dsh/cordis/inspect/resolve')
  assert.equal(resolve.params.requestId, 'inspect-1')
  assert.equal(resolve.params.resolution.ok, true)
  assert.equal(resolve.params.resolution.data.tokens.length, TOKEN_NAMES.length)
})

test('Client half previews through register while browser theme inject is refused', async () => {
  const theme = installTuiTheme(makeCtx())
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiTheme'],
      apply(ctx) {
        const dispose = ctx.tuiTheme.register({
          id: 'clay',
          label: 'Clay',
          dark: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#3A2418'])),
          light: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#F4E6D4'])),
        }, { activate: true })
        return dispose
      },
    }`,
    { tuiTheme: theme, effect: (fn) => fn() },
  )
  assert.deepEqual(applied.waitingFor, [])
  assert.equal(theme.active(), 'clay')
  applied.dispose()
  assert.equal(theme.active(), 'default')

  await assert.rejects(
    applyClientHalf(`return { inject: ['theme'], apply() {} }`, { tuiTheme: theme }),
    /tuiTheme/,
  )
})

test('dynamic ctx.effect invokes setup without synthetic arguments', async () => {
  const theme = installTuiTheme(makeCtx())
  const applied = await applyClientHalf(
    `return {
      inject: ['tuiTheme'],
      apply(ctx) {
        ctx.effect(function () {
          if (arguments.length !== 0) throw new Error('effect setup received an argument')
          return () => {}
        }, 'probe')
      },
    }`,
    { tuiTheme: theme },
  )
  applied.dispose()
})

test('_dsh/cordis/run/request loads a tuiTheme Client half and resolves', async () => {
  const calls = []
  const theme = installTuiTheme(makeCtx())
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    requestAgent(method, params) {
      calls.push({ method, params })
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-1' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({
          code: `return {
            inject: ['tuiTheme'],
            apply(ctx) {
              ctx.tuiTheme.register({
                id: 'clay',
                label: 'Clay',
                dark: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#3A2418'])),
                light: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#F4E6D4'])),
              }, { activate: true })
            },
          }`,
        })
      }
      if (method === '_dsh/cordis/run/resolve') return Promise.resolve({ accepted: true })
      return Promise.resolve(null)
    },
  })
  await client.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-1',
      agentId: 'agent-1',
      pluginId: 'clay-1',
      packageId: 'pkg-1',
      mode: 'run',
    },
  })
  assert.equal(theme.active(), 'clay')
  const started = calls.find((call) => call.method === '_dsh/cordis/run/host')
  assert.equal(started.params.approveFutureVersions, false)
  const resolved = calls.find((call) => call.method === '_dsh/cordis/run/resolve')
  assert.equal(resolved.params.resolution.ok, true)
  assert.equal(resolved.params.resolution.pluginRunId, 'run-1')
})

test('_dsh/cordis/plugins/retract unloads Client contributions but keeps the Theme Plugin selectable', async () => {
  const theme = installTuiTheme(makeCtx())
  const client = attachTuiClient({
    ctx: makeCtx(),
    tuiTheme: theme,
    requestAgent(method) {
      if (method === '_dsh/cordis/inspect/sync') return Promise.resolve({ ok: true })
      if (method === '_dsh/cordis/run/host') {
        return Promise.resolve({ ok: true, pluginRunId: 'run-owned' })
      }
      if (method === '_dsh/cordis/run/client-code') {
        return Promise.resolve({
          code: `return {
            inject: ['tuiTheme'],
            apply(ctx) {
              ctx.tuiTheme.register({
                id: 'owned',
                label: 'Owned',
                dark: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#332211'])),
                light: Object.fromEntries(${JSON.stringify(TOKEN_NAMES)}.map((name) => [name, '#EEDDBB'])),
              }, { activate: true })
            },
          }`,
        })
      }
      return Promise.resolve({ accepted: true })
    },
  })

  await client.onHost({
    method: '_dsh/cordis/run/request',
    params: {
      requestId: 'approval-owned',
      agentId: 'agent-1',
      pluginId: 'owned-1',
      packageId: 'pkg-owned',
      mode: 'run',
    },
  })
  assert.equal(theme.active(), 'owned')

  client.onHost({ method: '_dsh/cordis/plugins/retract', params: { pluginId: 'owned-1' } })
  assert.equal(theme.active(), 'default')
  assert.deepEqual(theme.list(), [
    { id: 'default', label: 'Default' },
    { id: 'owned', label: 'Owned' },
  ])
  assert.equal(theme.isLoaded('owned'), false)
})

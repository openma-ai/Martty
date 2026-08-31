/**
 * TUI Client inspect + run plane: advertise Theme, answer queries, load
 * `code.client` that injects `tuiTheme` / `tuiSlots`. Transport is ACP extras, not a
 * cordis-acp plugin-id sync.
 */

import { applyClientHalf, createClientTimer } from './client-run.js'
import { CORDIS_METHODS } from './cordis-protocol.js'

export const name = 'tui-cordis-client-runner'
export const inject = [
  'tuiTheme', 'tuiPresets', 'tuiSlots', 'tuiCommands', 'tuiOverlay', 'acpSessionConfig',
  'acpSessionPlan', 'acpSessionStats', 'acpSessionStatus',
]

const EMPTY_INPUT = Object.freeze({ type: 'object', properties: {}, additionalProperties: false })
const ANY_OUTPUT = Object.freeze({ description: 'JSON data owned by this inspect provider.' })

/**
 * @param {{ list: Function, active: Function, exportInspectTokens: Function }} tuiTheme
 * @returns {{ manifest: object, query: Function }}
 */
export function themeInspectProvider(tuiTheme) {
  return {
    manifest: {
      id: 'Theme',
      description:
        'TUI Theme Plugin seat and closed palette tokens. A Client Plugin registers its palette; '
        + '/theme loads or replaces that whole Plugin, including its other contributions.',
      methods: [{
        name: 'listTokens',
        description:
          'Return the closed token directory, mounted palettes, and how a Client half covers the TUI.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'listTokens') throw new Error(`unknown Theme inspect method "${method}"`)
      const active = tuiTheme.active()
      return {
        tokens: tuiTheme.exportInspectTokens(),
        palettes: tuiTheme.list().map((palette) => ({
          id: palette.id,
          label: palette.label,
          active: palette.id === active,
        })),
        apply: {
          inject: ['tuiTheme'],
          register: {
            call: 'ctx.tuiTheme.register(palette, options?)',
            palette: {
              required: ['id', 'label', 'dark', 'light'],
              optional: ['background'],
              background: {
                source: {
                  variants: [
                    { kind: 'file', path: 'absolute filesystem path to a PNG' },
                    { kind: 'data', mediaType: 'image/png', base64: 'non-empty string' },
                  ],
                },
                fit: {
                  enum: ['cover', 'contain', 'stretch'],
                  default: 'cover',
                },
                anchor: {
                  x: 'finite number from 0 (left) to 1 (right)',
                  y: 'finite number from 0 (top) to 1 (bottom)',
                  default: { x: 0.5, y: 0.5 },
                },
                opacity: 'finite number from 0 to 1; default 1',
              },
            },
            options: {
              type: 'object',
              required: ['palette'],
              additionalProperties: false,
              properties: {
                palette: {
                  type: 'object',
                  required: ['id', 'label', 'dark', 'light'],
                  description: 'Complete validated TUI palette pack.',
                },
                activate: {
                  type: 'boolean',
                  optional: true,
                  default: false,
                  description: 'Cover immediately and restore the actual previous palette on dispose.',
                },
              },
            },
            returns: {
              type: 'callable ThemeRegistration',
              legacyCall: 'registration() disposes the registration',
              properties: {
                update: {
                  call: 'registration.update({ background })',
                  role: 'update-background',
                  effect: 'Updates this owned theme in place; null removes its background.',
                },
                dispose: {
                  call: 'registration.dispose()',
                  role: 'dispose',
                  idempotent: true,
                },
              },
            },
          },
          lifecycle: {
            ownership:
              'The palette and every command, overlay, slot, timer, transaction, and Host RPC registered '
              + 'by the same Client Plugin share one Cordis Fiber lifetime.',
            preview:
              'register(palette, { activate: true }) selects this Plugin run for immediate preview. '
              + 'The runner replaces the previous Theme Plugin as one lifecycle.',
          },
          note:
            'Every pack supplies all 18 token names as #RRGGBB in both dark and light. '
            + 'A pack may additionally own one PNG background and update it through its registration. '
            + 'Register all features unconditionally in one Plugin. Do not inspect the active palette, '
            + 'subscribe to theme changes, add theme conditions to commands, call theme.overrideTokens, '
            + 'or write CSS variables.',
        },
        selection: {
          command: '/theme <id>',
          unit: 'Client Plugin',
          unload: 'all Plugin contributions',
          behavior:
            'Selecting an entry starts its owning Theme Plugin and stops the previously selected one. '
            + 'Selecting default stops the current dynamic Theme Plugin.',
        },
        referencedTypes: [],
      }
    },
  }
}

/** Describe saved compositions of several independent TUI contributions. */
export function uiPresetInspectProvider(tuiPresets) {
  return {
    manifest: {
      id: 'UiPresets',
      description:
        'Saved UI compositions. One preset mounts several UI contributions as one lifecycle.',
      methods: [{
        name: 'list',
        description: 'List registered UI Presets and the composition registration contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'list') throw new Error(`unknown UiPresets inspect method "${method}"`)
      return {
        active: tuiPresets.active(),
        presets: tuiPresets.list(),
        apply: {
          inject: ['tuiPresets'],
          register: {
            call: 'ctx.tuiPresets.register({ id, label }, mount)',
            mount:
              'A synchronous callback that mounts multiple UI contributions and returns one disposer.',
          },
          selection: '/ui <id>',
          persistence: 'The selected id survives restart in $MARTTY_HOME/settings.json.',
        },
      }
    },
  }
}

const TUI_NODE_KINDS = Object.freeze([
  'ascii', 'logo', 'welcomeinfo', 'text', 'group', 'markdown', 'reasoning',
  'user', 'generic', 'terminal', 'diff', 'image', 'notice', 'unknown',
])

/** Describe the root TUI shell slots available to a dynamic Client half. */
export function slotInspectProvider(tuiSlots) {
  return {
    manifest: {
      id: 'Slots',
      description:
        'Terminal-native root shell slots. Plugins contribute validated TuiNode trees; '
        + 'they do not receive React, the TTY, screen coordinates, or Ratatui.',
      methods: [{
        name: 'list',
        description: 'List declared TUI seats, current occupants, the registration API, and node kinds.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'list') throw new Error(`unknown Slots inspect method "${method}"`)
      return {
        slots: tuiSlots.list(),
        nodeKinds: [...TUI_NODE_KINDS],
        apply: {
          inject: ['tuiSlots'],
          wait: "ctx.tuiSlots.inject(slotName, () => { ... })",
          register:
            'ctx.tuiSlots.register({ name: slotName, id, order? }, TuiNode[])',
          update: 'const panel = register(...); panel.update(nextNodes)',
          dispose: 'Return panel.dispose from inject; plugin unload removes the pane immediately.',
          note:
            'welcome.hero and welcome.info are independent single root regions; '
            + 'chrome.right is a root list slot; conversation.input.dock is the additive row above '
            + 'the composer; conversation.navigation.dock is compact session navigation inside '
            + 'the composer above its metadata; conversation.composer.dock is the outer compact telemetry row. '
            + 'Node ids are stable inside one contribution; '
            + 'the compositor namespaces them by contribution id. Compose group/markdown/reasoning/'
            + 'user/generic/terminal/diff/image/notice/unknown nodes. Colors use theme tokens only.',
        },
        runtime: {
          inject: ['timer'],
          callHost: "await host.call('method', jsonArgs)",
          poll: 'ctx.interval(() => void refresh(), milliseconds)',
          note:
            'Host code registers Package-private JSON handlers with harness.handle. '
            + 'Client code calls them with host.call. For polling, inject timer and use ctx.interval; '
            + 'the interval is cancelled automatically when the Plugin run stops.',
        },
        referencedTypes: ['TuiNode'],
      }
    },
  }
}

/** Describe lifecycle-owned local slash commands available to dynamic Client halves. */
export function commandInspectProvider(tuiCommands) {
  return {
    manifest: {
      id: 'Commands',
      description:
        'TUI-local slash commands. Inspection is read-only; Client halves inject tuiCommands '
        + 'to register commands that never enter the agent prompt.',
      methods: [{
        name: 'list',
        description:
          'Return the current local command catalog and the register/list contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'list') throw new Error(`unknown Commands inspect method "${method}"`)
      return {
        commands: tuiCommands.list(),
        api: {
          service: 'tuiCommands',
          inject: ['tuiCommands'],
          register: {
            call: 'ctx.tuiCommands.register(options, handler)',
            options: {
              type: 'object',
              required: ['name', 'description'],
              additionalProperties: false,
              properties: {
                name: {
                  type: 'string',
                  pattern: '^[a-z0-9][a-z0-9-]*$',
                  slashPrefix: false,
                },
                description: { type: 'string', minLength: 1 },
                input: {
                  type: 'object',
                  required: ['hint'],
                  additionalProperties: false,
                  properties: {
                    hint: { type: 'string', minLength: 1 },
                    options: {
                      type: 'array',
                      items: {
                        type: 'object',
                        required: ['value'],
                        additionalProperties: false,
                        properties: {
                          value: { type: 'string', minLength: 1 },
                          label: { type: 'string' },
                          description: { type: 'string' },
                        },
                      },
                    },
                  },
                },
              },
            },
            handler: {
              arguments: [
                { name: 'args', type: 'string', default: '' },
                {
                  name: 'command',
                  type: 'object',
                  properties: { name: { type: 'string' } },
                },
              ],
              async: true,
            },
            returns: {
              type: 'function',
              role: 'dispose',
              idempotent: true,
              methods: {
                update: {
                  arguments: [{ name: 'patch', type: 'object' }],
                  description: 'Refresh description or input completion metadata',
                },
              },
            },
          },
          list: {
            call: 'ctx.tuiCommands.list()',
            returns: { type: 'array', items: 'Command' },
          },
          lifecycle: {
            ownership:
              'The registration belongs to the Client Plugin run and is removed when that run stops, '
              + 'even if the returned disposer is ignored.',
            invocation:
              'Users type /<name> followed by optional text. The handler receives that text as args; '
              + 'the command is handled locally and is not sent as an ACP prompt.',
          },
        },
        referencedTypes: ['Command'],
      }
    },
  }
}

/** Describe the native semantic overlay surface without opening visible UI. */
export function overlayInspectProvider(tuiOverlay) {
  return {
    manifest: {
      id: 'Overlay',
      description:
        'Native semantic TUI overlays. Inspection only returns the active snapshot; '
        + 'Client halves inject tuiOverlay to open user-requested controls.',
      methods: [{
        name: 'active',
        description: 'Return the active overlay snapshot and the slider lifecycle contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'active') throw new Error(`unknown Overlay inspect method "${method}"`)
      return {
        active: tuiOverlay.active(),
        api: {
          service: 'tuiOverlay',
          inject: ['tuiOverlay'],
          openSlider: {
            call: 'ctx.tuiOverlay.openSlider(options, handlers?)',
            options: {
              type: 'object',
              required: ['id', 'title', 'min', 'max', 'step', 'value'],
              additionalProperties: false,
              properties: {
                id: { type: 'string', minLength: 1 },
                title: { type: 'string', minLength: 1 },
                min: { type: 'number', finite: true },
                max: { type: 'number', finite: true, greaterThan: 'min' },
                step: { type: 'number', finite: true, exclusiveMinimum: 0 },
                value: { type: 'number', finite: true, clampedTo: ['min', 'max'] },
                marks: {
                  type: 'array',
                  optional: true,
                  default: [],
                  items: {
                    type: 'object',
                    required: ['value', 'label'],
                    additionalProperties: false,
                    properties: {
                      value: { type: 'number', finite: true, range: ['min', 'max'] },
                      id: { type: 'string', minLength: 1, optional: true },
                      label: { type: 'string', minLength: 1 },
                    },
                  },
                },
                snapToMarks: {
                  type: 'boolean',
                  optional: true,
                  default: false,
                  requires: 'at least one mark when true',
                },
              },
            },
            handlers: {
              onChange: {
                optional: true,
                async: true,
                arguments: [{ name: 'value', type: 'number' }],
                lifecycle: 'Runs after left/right changes; the slider remains open.',
              },
              onSubmit: {
                optional: true,
                async: true,
                arguments: [
                  { name: 'value', type: 'number' },
                  { name: 'mark', type: 'SliderMark | undefined' },
                  { name: 'controller', type: 'SliderController' },
                ],
                lifecycle: 'Enter closes the slider before this handler runs.',
              },
              onCancel: {
                optional: true,
                async: true,
                arguments: [
                  { name: 'value', type: 'number' },
                  { name: 'mark', type: 'SliderMark | undefined' },
                  { name: 'controller', type: 'SliderController' },
                ],
                lifecycle: 'Escape closes the slider before this handler runs.',
              },
            },
            returns: {
              type: 'SliderController',
              properties: {
                close: {
                  type: 'function',
                  role: 'close',
                  idempotent: true,
                  effect: 'Closes the active slider immediately.',
                },
              },
            },
          },
          openView: {
            call: 'ctx.tuiOverlay.openView(options, handlers?)',
            options: {
              type: 'object',
              required: ['id', 'title', 'nodes'],
              additionalProperties: false,
              properties: {
                id: { type: 'string', minLength: 1 },
                title: { type: 'string', minLength: 1 },
                nodes: { type: 'array', items: 'TuiNode' },
              },
            },
            handlers: {
              onSubmit: { optional: true, async: true, arguments: [] },
              onCancel: { optional: true, async: true, arguments: [] },
            },
            returns: {
              type: 'ViewController',
              properties: {
                close: { type: 'function', role: 'close', idempotent: true },
              },
            },
          },
          active: {
            call: 'ctx.tuiOverlay.active()',
            returns: 'Slider | View | null',
          },
          lifecycle: {
            concurrency: 'Only one overlay may be active at a time.',
            ownership:
              'The overlay belongs to the Client Plugin run and closes automatically when that run stops, '
              + 'even if the returned controller is ignored.',
          },
        },
        referencedTypes: ['Slider', 'SliderMark', 'SliderController', 'View', 'ViewController', 'TuiNode'],
      }
    },
  }
}

/** Describe the current standard ACP Session config-option snapshot and setter. */
export function configOptionsInspectProvider(acpSessionConfig) {
  return {
    manifest: {
      id: 'ConfigOptions',
      description:
        'Current ACP Session config options. Values and choices come from the connected agent; '
        + 'Client halves inject acpSessionConfig to read, set, and subscribe without raw ACP access.',
      methods: [{
        name: 'list',
        description:
          'Return the live Session option snapshot and the ACP-backed read/set/subscribe contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'list') throw new Error(`unknown ConfigOptions inspect method "${method}"`)
      return {
        options: acpSessionConfig.list(),
        api: {
          service: 'acpSessionConfig',
          inject: ['acpSessionConfig'],
          list: {
            call: 'ctx.acpSessionConfig.list()',
            returns: 'ConfigOption[]',
          },
          byCategory: {
            call: 'ctx.acpSessionConfig.byCategory(category)',
            parameters: [{ name: 'category', type: 'string', minLength: 1 }],
            returns: 'ConfigOption[]',
            standardCategories: ['mode', 'model', 'model_config', 'thought_level'],
            runtimeIdentity:
              'SessionConfigOption.category is the standard ACP semantic identity. Resolve it '
              + 'against the current live snapshot on every invocation; ids remain agent-owned.',
          },
          transaction: {
            call: 'ctx.acpSessionConfig.transaction({ category })',
            selectors: [
              { shape: '{ category: string }', match: 'exactly one live standard category' },
              { shape: '{ id: string }', match: 'exactly one live agent-owned id' },
            ],
            returns: {
              option: 'the matched live ConfigOption snapshot',
              original: 'the value captured when the transaction starts',
              value: 'transaction.value() returns the latest requested value',
              preview: 'await transaction.preview(value): serialized and deduplicated ACP writes',
              commit: 'await transaction.commit(value?): keep the final value; first finalizer wins',
              rollback: 'await transaction.rollback(): restore original; first finalizer wins',
            },
            preview:
              'preview(value) serializes ACP writes and is deduplicated when value is unchanged.',
            commit:
              'commit(value?) keeps the requested value and prevents later lifecycle rollback.',
            rollback:
              'rollback() restores original. An unfinished transaction rolls back automatically '
              + 'when its owning Client Plugin run stops, updates, fails, or unloads.',
          },
          current: {
            call: 'ctx.acpSessionConfig.current(id)',
            parameters: [{ name: 'id', type: 'string', minLength: 1 }],
            returns: 'string | boolean | undefined',
          },
          set: {
            call: 'await ctx.acpSessionConfig.set(id, value)',
            parameters: [
              { name: 'id', type: 'string', minLength: 1 },
              { name: 'value', type: 'string | boolean' },
            ],
            returns: 'Promise<ConfigOption[]>',
            transport: 'standard ACP session/set_config_option',
            validation:
              'The id must exist in the current snapshot. Select values must be advertised choices; '
              + 'boolean options require booleans.',
          },
          subscribe: {
            call: 'ctx.acpSessionConfig.subscribe((snapshot) => { ... })',
            listener: {
              arguments: [{
                name: 'snapshot',
                type: '{ sessionId?: string, options: ConfigOption[] }',
              }],
            },
            returns: { type: 'function', role: 'dispose', idempotent: true },
          },
          lifecycle: {
            values:
              'Read ids, kinds, current values, and legal choices from the current Session snapshot; '
              + 'never infer them from model metadata or Host defaults.',
            semanticSelection:
              'When the requested behavior matches an advertised standard ACP category, call '
              + 'byCategory(category) at runtime and require exactly one match. Use that live option id '
              + 'for current/set; never compile an inspect-time agent-owned id into portable Plugin code.',
            ownership:
              'Subscriptions belong to the Client Plugin run and are removed automatically on '
              + 'stop, update, rollback, or unload.',
            updates:
              'Successful set responses and later ACP config_option_update snapshots both refresh '
              + 'list/current and notify subscribers.',
          },
        },
        referencedTypes: ['ConfigOption'],
      }
    },
  }
}

/** Describe structured Plan snapshots folded from standard ACP Session updates. */
export function planInspectProvider(acpSessionPlan) {
  return {
    manifest: {
      id: 'Plans',
      description:
        'Current structured ACP Session plans. Client halves can render or react to plan updates '
        + 'without parsing raw ACP messages.',
      methods: [{
        name: 'current',
        description: 'Return the active plan and the read/subscribe contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'current') throw new Error(`unknown Plans inspect method "${method}"`)
      return {
        current: acpSessionPlan.current(),
        plans: acpSessionPlan.list(),
        api: {
          service: 'acpSessionPlan',
          inject: ['acpSessionPlan'],
          current: { call: 'ctx.acpSessionPlan.current()', returns: 'Plan | null' },
          list: { call: 'ctx.acpSessionPlan.list()', returns: 'Plan[]' },
          subscribe: {
            call: 'ctx.acpSessionPlan.subscribe((snapshot) => { ... })',
            listener: {
              arguments: [{
                name: 'snapshot',
                type: '{ sessionId?: string, plans: Plan[] }',
              }],
            },
            returns: { type: 'function', role: 'dispose', idempotent: true },
          },
          transport: 'standard ACP session/update plan, plan_update, and plan_removed',
        },
        referencedTypes: ['Plan'],
      }
    },
  }
}

/** Describe token totals and live prompt timings folded from standard ACP. */
export function statsInspectProvider(acpSessionStats) {
  return {
    manifest: {
      id: 'Stats',
      description:
        'Current standard ACP token usage and client-observed prompt timings. '
        + 'Client halves can render stats without parsing raw ACP messages.',
      methods: [{
        name: 'current',
        description: 'Return the current stats snapshot and the read/subscribe contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'current') throw new Error(`unknown Stats inspect method "${method}"`)
      return {
        current: acpSessionStats.current(),
        api: {
          service: 'acpSessionStats',
          inject: ['acpSessionStats'],
          current: {
            call: 'ctx.acpSessionStats.current()',
            returns: 'SessionStatsSnapshot',
          },
          subscribe: {
            call: 'ctx.acpSessionStats.subscribe((snapshot) => { ... })',
            returns: { type: 'function', role: 'dispose', idempotent: true },
          },
          transport: 'standard ACP prompt responses and session/update notifications',
        },
        referencedTypes: ['SessionStatsSnapshot'],
      }
    },
  }
}

/** Describe non-statistics run-state facts folded from standard ACP. */
export function statusInspectProvider(acpSessionStatus) {
  return {
    manifest: {
      id: 'Status',
      description:
        'Current ACP run-state facts (connection, server, auth, session, model, '
        + 'effort, permission, plan, agent) folded from standard ACP traffic. '
        + 'Statistics stay in the Stats provider.',
      methods: [{
        name: 'current',
        description: 'Return the current status snapshot and the read/subscribe contract.',
        inputSchema: EMPTY_INPUT,
        outputSchema: ANY_OUTPUT,
      }],
    },
    query(method) {
      if (method !== 'current') throw new Error(`unknown Status inspect method "${method}"`)
      return {
        current: acpSessionStatus.current(),
        api: {
          service: 'acpSessionStatus',
          inject: ['acpSessionStatus'],
          current: {
            call: 'ctx.acpSessionStatus.current()',
            returns: 'SessionStatusSnapshot',
          },
          subscribe: {
            call: 'ctx.acpSessionStatus.subscribe((snapshot) => { ... })',
            returns: { type: 'function', role: 'dispose', idempotent: true },
          },
          transport: 'standard ACP initialize, authenticate, session, session/prompt response, '
            + 'and session/update messages, plus optional Martty session.status/session.event '
            + 'extensions',
        },
        referencedTypes: ['SessionStatusSnapshot'],
      }
    },
  }
}

async function mountClientHalf(
  ctx,
  pluginId,
  clientCode,
  tuiTheme,
  tuiPresets,
  tuiSlots,
  tuiCommands,
  tuiOverlay,
  acpSessionConfig,
  acpSessionPlan,
  acpSessionStats,
  acpSessionStatus,
  timer,
  invoke,
) {
  if (typeof ctx.plugin !== 'function') {
    return applyClientHalf(clientCode, {
      pluginId,
      tuiTheme, tuiPresets, tuiSlots, tuiCommands, tuiOverlay, acpSessionConfig, acpSessionPlan,
      acpSessionStats, acpSessionStatus, timer, invoke,
    })
  }

  let applied
  const fiber = ctx.plugin({
    name: `tui-dynamic:${pluginId}`,
    inject: [],
    apply: async () => {
      applied = await applyClientHalf(clientCode, {
        pluginId,
        tuiTheme, tuiPresets, tuiSlots, tuiCommands, tuiOverlay, acpSessionConfig, acpSessionPlan,
        acpSessionStats, acpSessionStatus, timer, invoke,
      })
      return applied.dispose
    },
  })
  try {
    await fiber
  } catch (error) {
    await fiber.dispose()
    throw error
  }
  return {
    waitingFor: applied.waitingFor,
    dispose() {
      applied.dispose()
      return fiber.dispose()
    },
  }
}

/**
 * Bind inspect/run to the mux after the painter is up.
 * @param {{
 *   ctx: object,
 *   tuiTheme: object,
 *   tuiSlots?: object,
 *   tuiCommands?: object,
 *   tuiOverlay?: object,
 *   acpSessionConfig?: object,
 *   acpSessionPlan?: object,
 *   acpSessionStats?: object,
 *   acpSessionStatus?: object,
 *   localPlugins?: object,
 *   requestAgent: (method: string, params?: object) => Promise<unknown>,
 *   notifyTui?: (method: string, params?: object) => void,
 * }} opts
 * @returns {{ onHost: (message: object) => void, respondApproval: (params: object) => Promise<void> }}
 */
export function attachTuiClient(opts) {
  const {
    ctx, tuiTheme, tuiPresets, tuiSlots, tuiCommands, tuiOverlay, acpSessionConfig, acpSessionPlan,
    acpSessionStats, acpSessionStatus, localPlugins,
    requestAgent, notifyTui = () => {},
  } = opts
  const providers = [
    themeInspectProvider(tuiTheme),
    ...(tuiPresets === undefined ? [] : [uiPresetInspectProvider(tuiPresets)]),
    ...(tuiSlots === undefined ? [] : [slotInspectProvider(tuiSlots)]),
    ...(tuiCommands === undefined ? [] : [commandInspectProvider(tuiCommands)]),
    ...(tuiOverlay === undefined ? [] : [overlayInspectProvider(tuiOverlay)]),
    ...(acpSessionConfig === undefined ? [] : [configOptionsInspectProvider(acpSessionConfig)]),
    ...(acpSessionPlan === undefined ? [] : [planInspectProvider(acpSessionPlan)]),
    ...(acpSessionStats === undefined ? [] : [statsInspectProvider(acpSessionStats)]),
    ...(acpSessionStatus === undefined ? [] : [statusInspectProvider(acpSessionStatus)]),
  ]
  const timer = createClientTimer()
  /** @type {Map<string, () => void>} */
  const loaded = new Map()
  const pendingThemeSelections = new Map()
  const pendingUiSelections = new Map()
  const pendingApprovals = new Map()

  function publishApprovals() {
    notifyTui(CORDIS_METHODS.approvalsUpdate, {
      protocol: 0,
      approvals: [...pendingApprovals.values()].map((request) => ({
        requestId: request.requestId,
        agentId: request.agentId,
        pluginId: request.pluginId,
        packageId: request.packageId,
        mode: request.mode,
        name: request.name,
        purpose: request.purpose,
      })),
    })
  }

  async function stopOwner(agentId, pluginId) {
    if (localPlugins?.has?.(pluginId) === true) {
      await localPlugins.stop(pluginId)
      return
    }
    const stopped = await requestAgent(CORDIS_METHODS.pluginStop, { agentId, pluginId })
    if (stopped?.ok === false) {
      throw new Error(typeof stopped.message === 'string'
        ? stopped.message
        : `failed to stop theme Plugin "${pluginId}"`)
    }
  }

  function sync() {
    void requestAgent(CORDIS_METHODS.inspectSync, { providers: providers.map((provider) => provider.manifest) }).catch(() => {
      // Agent has no TUI Client extras (Zed / non-dsh ACP). Stay a standard client.
    })
  }

  async function answerQuery(params) {
    const request = params !== null && typeof params === 'object' ? params : {}
    const requestId = request.requestId
    const agentId = request.agentId
    if (typeof requestId !== 'string' || typeof agentId !== 'string') return
    let resolution
    try {
      const provider = providers.find((candidate) => candidate.manifest.id === request.provider)
      if (provider === undefined) {
        resolution = {
          ok: false,
          reason: 'provider-missing',
          message: `Client inspect provider "${String(request.provider)}" is unavailable`,
        }
      } else {
        const data = await provider.query(request.method, request.input)
        resolution = { ok: true, data }
      }
    } catch (error) {
      resolution = {
        ok: false,
        reason: 'provider-error',
        message: error instanceof Error ? error.message : String(error),
      }
    }
    await requestAgent(CORDIS_METHODS.inspectResolve, { agentId, requestId, resolution }).catch(() => {})
  }

  async function runClient(params, direct = false, approveFutureVersions = false) {
    const request = params !== null && typeof params === 'object' ? params : {}
    const requestId = request.requestId
    if (!direct && typeof requestId !== 'string') return
    const settle = (resolution) => requestAgent(
      direct ? CORDIS_METHODS.settleUserRun : CORDIS_METHODS.resolveRequestRun,
      direct
        ? { agentId: request.agentId, pluginId: request.pluginId, resolution }
        : { requestId, resolution },
    )
    let host = direct && typeof request.pluginRunId === 'string'
      ? { ok: true, pluginRunId: request.pluginRunId }
      : undefined
    const previousThemeOwner = tuiTheme.owner?.(tuiTheme.active())
    const previousUiOwner = tuiPresets?.owner?.(tuiPresets.active?.())
    try {
      if (!direct) {
        host = await requestAgent(CORDIS_METHODS.runHost, {
          agentId: request.agentId,
          pluginId: request.pluginId,
          packageId: request.packageId,
          mode: request.mode,
          requestId,
          approveFutureVersions,
        })
      }
      if (host === null || typeof host !== 'object' || host.ok !== true) {
        if (!direct) {
          await settle({
            ok: false,
            reason: 'host-half-failed',
            message: host && typeof host === 'object' && typeof host.message === 'string'
              ? host.message
              : 'Host half failed',
            ...(typeof host?.pluginRunId === 'string' ? { pluginRunId: host.pluginRunId } : {}),
          })
        }
        return
      }
      if (direct && request.hasClientHalf !== true) return
      const source = await requestAgent(CORDIS_METHODS.getClientCode, {
        agentId: request.agentId,
        pluginId: request.pluginId,
        pluginRunId: host.pluginRunId,
      })
      const code = source && typeof source === 'object' ? source.code : undefined
      loaded.get(request.pluginId)?.()
      const applied = await mountClientHalf(
        ctx,
        request.pluginId,
        typeof code === 'string' ? code : '',
        tuiTheme,
        tuiPresets,
        tuiSlots,
        tuiCommands,
        tuiOverlay,
        acpSessionConfig,
        acpSessionPlan,
        acpSessionStats,
        acpSessionStatus,
        timer,
        async (method, args) => {
          const answered = await requestAgent(CORDIS_METHODS.pluginInvoke, {
            agentId: request.agentId,
            pluginId: request.pluginId,
            pluginRunId: host.pluginRunId,
            method,
            args,
          })
          if (answered !== null && typeof answered === 'object' && answered.ok === true) {
            return answered.value
          }
          const code = answered !== null && typeof answered === 'object' ? answered.code : undefined
          const message = answered !== null && typeof answered === 'object' ? answered.message : undefined
          throw new Error(
            `host.call("${String(method)}") failed`
            + `${typeof code === 'string' ? ` (${code})` : ''}: `
            + `${typeof message === 'string' ? message : 'invalid Host response'}`,
          )
        },
      )
      loaded.set(request.pluginId, applied.dispose)
      const selectedUi = pendingUiSelections.get(request.pluginId)
      if (selectedUi !== undefined && tuiPresets?.isLoaded?.(selectedUi) === true) {
        pendingUiSelections.delete(request.pluginId)
        tuiPresets.activate(selectedUi)
        if (previousUiOwner !== undefined && previousUiOwner !== request.pluginId
          && tuiTheme.owner?.(tuiTheme.active()) !== previousUiOwner) {
          await stopOwner(request.agentId, previousUiOwner)
        }
      }
      const selectedTheme = pendingThemeSelections.get(request.pluginId)
      if (selectedTheme !== undefined && tuiTheme.isLoaded?.(selectedTheme) === true) {
        pendingThemeSelections.delete(request.pluginId)
        tuiTheme.activate(selectedTheme)
      }
      const selectedThemeOwner = tuiTheme.owner?.(tuiTheme.active())
      if (selectedThemeOwner === request.pluginId
        && previousThemeOwner !== undefined
        && previousThemeOwner !== selectedThemeOwner) {
        try {
          await stopOwner(request.agentId, previousThemeOwner)
        } catch (error) {
          if (loaded.get(request.pluginId) === applied.dispose) {
            loaded.delete(request.pluginId)
            applied.dispose()
          }
          throw error
        }
      }
      await settle({
        ok: true,
        pluginRunId: host.pluginRunId,
        waitingFor: applied.waitingFor,
      })
    } catch (error) {
      if (!direct || (host !== null && typeof host === 'object' && host.ok === true)) {
        await settle({
          ok: false,
          reason: 'client-half-failed',
          message: error instanceof Error ? error.message : String(error),
          ...(typeof host?.pluginRunId === 'string' ? { pluginRunId: host.pluginRunId } : {}),
          ...(direct && typeof request.startedHere === 'boolean'
            ? { startedHere: request.startedHere }
            : {}),
        }).catch(() => {})
      }
    }
  }

  function onHost(message) {
    switch (message.method) {
      case CORDIS_METHODS.inspectQuery:
        return answerQuery(message.params)
      case CORDIS_METHODS.requestRun:
        if (message.params !== null && typeof message.params === 'object'
          && message.params.requiresApproval === true
          && typeof message.params.requestId === 'string') {
          pendingApprovals.set(message.params.requestId, { ...message.params })
          publishApprovals()
          return
        }
        return runClient(message.params)
      case CORDIS_METHODS.requestRunResolved: {
        const requestId = message.params !== null && typeof message.params === 'object'
          ? message.params.requestId
          : undefined
        if (typeof requestId === 'string' && pendingApprovals.delete(requestId)) publishApprovals()
        return
      }
      case CORDIS_METHODS.userRun:
        return runClient(message.params, true)
      case CORDIS_METHODS.pluginRetract: {
        const pluginId = message.params && typeof message.params === 'object'
          ? message.params.pluginId
          : undefined
        if (typeof pluginId === 'string') {
          loaded.get(pluginId)?.()
          loaded.delete(pluginId)
        }
        return
      }
      default:
    }
  }

  async function respondApproval(params) {
    const request = params !== null && typeof params === 'object' ? params : {}
    if (request.protocol !== 0) throw new Error('unsupported approval response payload')
    if (typeof request.requestId !== 'string' || request.requestId.length === 0) {
      throw new Error('approval response needs requestId')
    }
    if (!['allow-version', 'allow-future', 'reject'].includes(request.decision)) {
      throw new Error('approval response has an invalid decision')
    }
    const pending = pendingApprovals.get(request.requestId)
    if (pending === undefined) return
    pendingApprovals.delete(request.requestId)
    publishApprovals()
    if (request.decision === 'reject') {
      await requestAgent(CORDIS_METHODS.resolveRequestRun, {
        requestId: request.requestId,
        resolution: { ok: false, reason: 'rejected' },
      })
      return
    }
    await runClient(pending, false, request.decision === 'allow-future')
  }

  async function selectTheme(params) {
    const request = params !== null && typeof params === 'object' ? params : {}
    if (request.protocol !== 0) throw new Error('unsupported theme selection payload')
    if (typeof request.agentId !== 'string' || request.agentId.length === 0) {
      throw new Error('theme selection needs agentId')
    }
    if (typeof request.id !== 'string' || request.id.length === 0) {
      throw new Error('theme selection needs id')
    }
    if (!tuiTheme.list().some((entry) => entry.id === request.id)) {
      throw new Error(`theme Plugin "${request.id}" is not registered`)
    }

    const previousId = tuiTheme.active()
    const previousOwner = tuiTheme.owner?.(previousId)
    const targetOwner = tuiTheme.owner?.(request.id)

    if (targetOwner !== undefined && localPlugins?.has?.(targetOwner) === true) {
      if (localPlugins.isLoaded?.(targetOwner) !== true) {
        await localPlugins.start(targetOwner)
      }
      if (tuiTheme.isLoaded?.(request.id) !== true) {
        throw new Error(`local theme Plugin "${targetOwner}" did not load palette "${request.id}"`)
      }
      tuiTheme.activate(request.id)
      if (previousOwner !== undefined && previousOwner !== targetOwner) {
        try {
          await stopOwner(request.agentId, previousOwner)
        } catch (error) {
          await localPlugins.stop(targetOwner)
          if (tuiTheme.isLoaded?.(previousId) === true) tuiTheme.activate(previousId)
          throw error
        }
      }
      return { ok: true, status: 'selected', id: request.id }
    }

    if (targetOwner === undefined || loaded.has(targetOwner)) {
      tuiTheme.activate(request.id)
      if (previousOwner !== undefined && previousOwner !== targetOwner) {
        try {
          await stopOwner(request.agentId, previousOwner)
        } catch (error) {
          if (tuiTheme.isLoaded?.(previousId) === true) tuiTheme.activate(previousId)
          throw error
        }
      }
      return { ok: true, status: 'selected', id: request.id }
    }

    pendingThemeSelections.set(targetOwner, request.id)
    try {
      const started = await requestAgent(CORDIS_METHODS.pluginStart, {
        agentId: request.agentId,
        pluginId: targetOwner,
      })
      if (started?.ok === false) {
        throw new Error(typeof started.message === 'string'
          ? started.message
          : `failed to start theme Plugin "${targetOwner}"`)
      }
      if (loaded.has(targetOwner) && tuiTheme.isLoaded?.(request.id) === true) {
        pendingThemeSelections.delete(targetOwner)
        tuiTheme.activate(request.id)
      }
      return { ok: true, status: 'starting', id: request.id }
    } catch (error) {
      pendingThemeSelections.delete(targetOwner)
      throw error
    }
  }

  async function selectUi(params) {
    const request = params !== null && typeof params === 'object' ? params : {}
    if (request.protocol !== 0) throw new Error('unsupported UI selection payload')
    if (typeof request.agentId !== 'string' || request.agentId.length === 0) {
      throw new Error('UI selection needs agentId')
    }
    if (typeof request.id !== 'string' || request.id.length === 0) {
      throw new Error('UI selection needs id')
    }
    if (!tuiPresets?.list?.().some((entry) => entry.id === request.id)) {
      throw new Error(`UI Plugin "${request.id}" is not registered`)
    }
    const previousId = tuiPresets.active()
    const previousOwner = tuiPresets.owner?.(previousId)
    const targetOwner = tuiPresets.owner?.(request.id)

    if (targetOwner === undefined || tuiPresets.isLoaded?.(request.id) === true) {
      tuiPresets.activate(request.id)
      if (previousOwner !== undefined && previousOwner !== targetOwner
        && tuiTheme.owner?.(tuiTheme.active()) !== previousOwner) {
        await stopOwner(request.agentId, previousOwner)
      }
      return { ok: true, status: 'selected', id: request.id }
    }
    if (localPlugins?.has?.(targetOwner) === true) {
      await localPlugins.start(targetOwner)
      if (tuiPresets.isLoaded?.(request.id) !== true) {
        throw new Error(`local UI Plugin "${targetOwner}" did not load UI "${request.id}"`)
      }
      tuiPresets.activate(request.id)
      if (previousOwner !== undefined && previousOwner !== targetOwner
        && tuiTheme.owner?.(tuiTheme.active()) !== previousOwner) {
        await stopOwner(request.agentId, previousOwner)
      }
      return { ok: true, status: 'selected', id: request.id }
    }

    pendingUiSelections.set(targetOwner, request.id)
    try {
      const started = await requestAgent(CORDIS_METHODS.pluginStart, {
        agentId: request.agentId,
        pluginId: targetOwner,
      })
      if (started?.ok === false) {
        throw new Error(typeof started.message === 'string'
          ? started.message
          : `failed to start UI Plugin "${targetOwner}"`)
      }
      return { ok: true, status: 'starting', id: request.id }
    } catch (error) {
      pendingUiSelections.delete(targetOwner)
      throw error
    }
  }

  function dispose() {
    for (const release of loaded.values()) release()
    loaded.clear()
    if (pendingApprovals.size > 0) {
      pendingApprovals.clear()
      publishApprovals()
    }
  }

  return { onHost, respondApproval, selectTheme, selectUi, sync, dispose }
}

/** Mount the TUI counterpart of the Web Cordis Client runner. */
export function apply(ctx) {
  const tuiTheme = ctx.tuiTheme ?? ctx.get?.('tuiTheme')
  const tuiPresets = ctx.tuiPresets ?? ctx.get?.('tuiPresets')
  const tuiSlots = ctx.tuiSlots ?? ctx.get?.('tuiSlots')
  const tuiCommands = ctx.tuiCommands ?? ctx.get?.('tuiCommands')
  const tuiOverlay = ctx.tuiOverlay ?? ctx.get?.('tuiOverlay')
  const acpSessionConfig = ctx.acpSessionConfig ?? ctx.get?.('acpSessionConfig')
  const acpSessionPlan = ctx.acpSessionPlan ?? ctx.get?.('acpSessionPlan')
  const acpSessionStats = ctx.acpSessionStats ?? ctx.get?.('acpSessionStats')
  const acpSessionStatus = ctx.acpSessionStatus ?? ctx.get?.('acpSessionStatus')
  const localPlugins = ctx.get?.('tuiLocalPlugins')
  let client
  const service = {
    bindTransport(requestAgent, notifyTui) {
      client?.dispose()
      client = attachTuiClient({
        ctx, tuiTheme, tuiPresets, tuiSlots, tuiCommands, tuiOverlay, acpSessionConfig, acpSessionPlan,
        acpSessionStats, acpSessionStatus, localPlugins,
        requestAgent, notifyTui,
      })
      const attached = client
      return () => {
        if (client !== attached) return
        attached.dispose()
        client = undefined
      }
    },
    onHost(message) {
      return client?.onHost(message)
    },
    selectTheme(params) {
      if (client === undefined) throw new Error('TUI Cordis Client runner is not attached')
      return client.selectTheme(params)
    },
    selectUi(params) {
      if (client === undefined) throw new Error('TUI Cordis Client runner is not attached')
      return client.selectUi(params)
    },
    respondApproval(params) {
      if (client === undefined) throw new Error('TUI Cordis Client runner is not attached')
      return client.respondApproval(params)
    },
    sync() {
      client?.sync()
    },
  }
  if (typeof ctx.provide === 'function') {
    ctx.provide('tuiCordisClientRunner', service)
  } else {
    ctx.tuiCordisClientRunner = service
  }
  if (typeof ctx.effect === 'function') {
    ctx.effect(() => () => client?.dispose(), 'tui-cordis-client-runner')
  }
}

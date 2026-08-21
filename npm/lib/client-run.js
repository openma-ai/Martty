/**
 * Evaluate a Cordis `code.client` function body on the TUI client tree.
 *
 * Same closure convention as the web runner: the source is an async function
 * body that returns a plugin. Open services include `tuiTheme`, `tuiPresets`, `tuiSlots`,
 * `acpSessionConfig`, `acpSessionPlan`, `acpSessionStats`, `acpSessionStatus`,
 * and lifecycle-owned `timer`; `host.call` reaches this
 * Package's Host half.
 * No React, browser slots, TTY, or raw Host service names.
 */

const ALLOWED_INJECT = new Set([
  'tuiTheme',
  'tuiPresets',
  'tuiSlots',
  'tuiCommands',
  'tuiOverlay',
  'acpSessionConfig',
  'acpSessionPlan',
  'acpSessionStats',
  'acpSessionStatus',
  'timer',
])

const THEME_TEACHING =
  'TUI Client Theme is ctx.tuiTheme, not ctx.theme. '
  + 'register a complete 18-token palette with tuiTheme.register({ id, label, dark, light }) '
  + 'inside one Theme Plugin; /theme loads or replaces that whole Plugin. '
  + 'Token names are closed #RRGGBB keys, not CSS variables.'

const SLOT_TEACHING =
  'TUI shell slots are ctx.tuiSlots, not browser ctx.slots. '
  + 'Inspect tuiSlots.list(), then use tuiSlots.inject(name, () => tuiSlots.register('
  + '{ name, id }, nodes)).'

/**
 * @param {string} clientCode
 * @param {{ pluginId?: string, tuiTheme?: object, tuiSlots?: object, tuiCommands?: object, tuiOverlay?: object, acpSessionConfig?: object, acpSessionPlan?: object, acpSessionStats?: object, acpSessionStatus?: object, timer?: object, invoke?: Function }} env
 * @returns {Promise<{ waitingFor: string[], dispose: () => void }>}
 */
export async function applyClientHalf(clientCode, env) {
  if (typeof clientCode !== 'string' || clientCode.trim().length === 0) {
    throw new Error('TUI Client half is empty')
  }
  let plugin
  try {
    const factory = new Function(
      'React',
      'styles',
      'host',
      'harness',
      'require',
      'process',
      'Buffer',
      'setTimeout',
      'setInterval',
      `return (async () => {\n${clientCode}\n})()`,
    )
    plugin = await factory(
      reactTrap,
      stylesTrap,
      hostFor(env),
      harnessTrap,
      requireTrap,
      processTrap,
      bufferTrap,
      timerGlobalTrap,
      timerGlobalTrap,
    )
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new Error(
        `TUI Client half failed to parse: ${error.message}\n`
        + 'The Client half is plain JavaScript (no JSX, no TypeScript).',
      )
    }
    throw error
  }
  const evaluated = normalizePlugin(plugin)
  const inject = evaluated.inject ?? []
  for (const name of inject) {
    if (name === 'theme') {
      throw new Error(THEME_TEACHING)
    }
    if (name === 'slots') {
      throw new Error(SLOT_TEACHING)
    }
    if (!ALLOWED_INJECT.has(name)) {
      throw new Error(
        `TUI Client inject "${name}" is not open. Open services: tuiTheme, tuiPresets, tuiSlots, tuiCommands, tuiOverlay, acpSessionConfig, acpSessionPlan, acpSessionStats, acpSessionStatus, timer.`,
      )
    }
  }
  const waitingFor = inject.filter((name) => env[name] === undefined)
  if (waitingFor.length > 0) return { waitingFor, dispose() {} }

  const owned = new Set()
  const own = (dispose) => {
    if (typeof dispose !== 'function') return dispose
    let active = true
    const release = () => {
      if (!active) return
      active = false
      owned.delete(release)
      return dispose()
    }
    owned.add(release)
    return release
  }
  const ctx = restrictedCtx(env, own, new Set(inject))
  const result = await evaluated.apply(ctx)
  if (typeof result === 'function' && !owned.has(result)) own(result)
  return {
    waitingFor: [],
    dispose() {
      for (const release of [...owned].reverse()) release()
    },
  }
}

function normalizePlugin(value) {
  if (typeof value === 'function') {
    return { inject: [], apply: value }
  }
  if (value !== null && typeof value === 'object' && typeof value.apply === 'function') {
    const inject = Array.isArray(value.inject) ? value.inject.map(String) : []
    return { inject, apply: value.apply.bind(value) }
  }
  throw new Error(
    'TUI Client half must return a plugin object `{ inject, apply }` or an apply function. '
    + THEME_TEACHING,
  )
}

function restrictedCtx(env, own, inject) {
  const sourceTheme = env.tuiTheme
  const sourcePresets = env.tuiPresets
  const sourceSlots = env.tuiSlots
  const sourceCommands = env.tuiCommands
  const sourceOverlay = env.tuiOverlay
  const sourceSessionConfig = env.acpSessionConfig
  const sourceSessionPlan = env.acpSessionPlan
  const sourceSessionStats = env.acpSessionStats
  const sourceSessionStatus = env.acpSessionStatus
  const sourceTimer = env.timer
  const tuiTheme = sourceTheme === undefined
    ? undefined
    : {
        register(palette, options) {
          const register = typeof env.pluginId === 'string'
            && typeof sourceTheme.registerOwned === 'function'
            ? sourceTheme.registerOwned.bind(sourceTheme, env.pluginId)
            : sourceTheme.register.bind(sourceTheme)
          const registration = register(palette, options)
          const dispose = own(
            typeof registration?.dispose === 'function'
              ? registration.dispose.bind(registration)
              : registration,
          )
          if (typeof dispose !== 'function') return dispose
          dispose.dispose = dispose
          if (typeof registration?.update === 'function') {
            dispose.update = (patch) => registration.update(patch)
          }
          return dispose
        },
      }
  const tuiSlots = sourceSlots === undefined
    ? undefined
    : {
        inject(name, callback) {
          return own(sourceSlots.inject(name, callback))
        },
        register(options, nodes) {
          const panel = sourceSlots.register(options, nodes)
          const dispose = own(() => panel.dispose())
          return {
            update(nextNodes) {
              return panel.update(nextNodes)
            },
            dispose,
          }
        },
        list() {
          return sourceSlots.list()
        },
      }
  const tuiPresets = sourcePresets === undefined
    ? undefined
    : {
        register(options, mount) {
          return own(sourcePresets.register(options, mount))
        },
        list() {
          return sourcePresets.list()
        },
        active() {
          return sourcePresets.active()
        },
      }
  const tuiCommands = sourceCommands === undefined
    ? undefined
    : {
        register(options, handler) {
          return own(sourceCommands.register(options, handler))
        },
        list() {
          return sourceCommands.list()
        },
      }
  const tuiOverlay = sourceOverlay === undefined
    ? undefined
    : {
        openSlider(options, handlers) {
          const controller = sourceOverlay.openSlider(options, handlers)
          const close = own(() => controller.close())
          return { close }
        },
        openView(options, handlers) {
          const controller = sourceOverlay.openView(options, handlers)
          const close = own(() => controller.close())
          return { close }
        },
        active() {
          return sourceOverlay.active()
        },
      }
  const acpSessionConfig = sourceSessionConfig === undefined
    ? undefined
    : {
        list() {
          return sourceSessionConfig.list()
        },
        current(id) {
          return sourceSessionConfig.current(id)
        },
        byCategory(category) {
          return sourceSessionConfig.byCategory(category)
        },
        transaction(selector) {
          const transaction = sourceSessionConfig.transaction(selector)
          own(() => transaction.rollback())
          return transaction
        },
        set(id, value) {
          return sourceSessionConfig.set(id, value)
        },
        subscribe(listener) {
          return own(sourceSessionConfig.subscribe(listener))
        },
      }
  const acpSessionPlan = sourceSessionPlan === undefined
    ? undefined
    : {
        list() {
          return sourceSessionPlan.list()
        },
        current() {
          return sourceSessionPlan.current()
        },
        subscribe(listener) {
          return own(sourceSessionPlan.subscribe(listener))
        },
      }
  const acpSessionStats = sourceSessionStats === undefined
    ? undefined
    : {
        current() {
          return sourceSessionStats.current()
        },
        subscribe(listener) {
          return own(sourceSessionStats.subscribe(listener))
        },
      }
  const acpSessionStatus = sourceSessionStatus === undefined
    ? undefined
    : {
        current() {
          return sourceSessionStatus.current()
        },
        subscribe(listener) {
          return own(sourceSessionStatus.subscribe(listener))
        },
      }
  const timer = sourceTimer === undefined || !inject.has('timer')
    ? undefined
    : {
        interval(callback, delay) {
          return own(sourceTimer.interval(callback, delay))
        },
        timeout(callback, delay) {
          return own(sourceTimer.timeout(callback, delay))
        },
      }
  return {
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
    interval: timer?.interval,
    timeout: timer?.timeout,
    get(name) {
      if (name === 'tuiTheme') return tuiTheme
      if (name === 'tuiPresets') return tuiPresets
      if (name === 'tuiSlots') return tuiSlots
      if (name === 'tuiCommands') return tuiCommands
      if (name === 'tuiOverlay') return tuiOverlay
      if (name === 'acpSessionConfig') return acpSessionConfig
      if (name === 'acpSessionPlan') return acpSessionPlan
      if (name === 'acpSessionStats') return acpSessionStats
      if (name === 'acpSessionStatus') return acpSessionStatus
      if (name === 'timer') return timer
      return undefined
    },
    effect(fn) {
      return typeof fn === 'function' ? own(fn()) : undefined
    },
    on() {
      throw new Error('TUI Client events are not open yet')
    },
    provide() {
      throw new Error('TUI Client halves cannot provide services')
    },
    plugin() {
      throw new Error('mount TUI contributions with tuiTheme/tuiSlots, not ctx.plugin')
    },
  }
}

function reactTrap() {
  throw new Error(
    'React is not available on the TUI Client. Use tuiTheme palettes or tuiSlots TuiNode trees.',
  )
}

const stylesTrap = {
  insert() {
    throw new Error('styles.insert is a browser Client symbol. TUI palettes use tuiTheme.register.')
  },
}

function hostFor(env) {
  return {
    call(method, args = null) {
      if (typeof env.invoke !== 'function') {
        return Promise.reject(new Error('host.call is unavailable without an active TUI Cordis Host run'))
      }
      return Promise.resolve().then(() => env.invoke(method, args))
    },
  }
}

function timerGlobalTrap() {
  throw new Error(
    "timer globals are unavailable in dynamic TUI plugins; declare inject: ['timer'] and use ctx.interval/timeout",
  )
}

/** Create lifecycle-neutral timer operations; the dynamic Context owns each returned disposer. */
export function createClientTimer() {
  return {
    interval(callback, delay) {
      assertTimerArgs('interval', callback, delay)
      const handle = globalThis.setInterval(callback, delay)
      return () => globalThis.clearInterval(handle)
    },
    timeout(callback, delay) {
      assertTimerArgs('timeout', callback, delay)
      const handle = globalThis.setTimeout(callback, delay)
      return () => globalThis.clearTimeout(handle)
    },
  }
}

function assertTimerArgs(name, callback, delay) {
  if (typeof callback !== 'function') throw new Error(`ctx.${name}: callback must be a function`)
  if (typeof delay !== 'number' || !Number.isFinite(delay) || delay < 0) {
    throw new Error(`ctx.${name}: delay must be a non-negative finite number`)
  }
}

const harnessTrap = new Proxy({}, {
  get(_target, prop) {
    throw new Error(
      `harness.${String(prop)} belongs to the HOST half (code.host). `
      + 'Register Package-private methods there with harness.handle; call them here with host.call.',
    )
  },
})

function requireTrap() {
  throw new Error('modules cannot be imported in a dynamic Client half')
}

const processTrap = new Proxy({}, {
  get() {
    throw new Error('process is not available in a TUI Client half')
  },
})

const bufferTrap = new Proxy({}, {
  get() {
    throw new Error('Buffer is not available in a TUI Client half')
  },
})

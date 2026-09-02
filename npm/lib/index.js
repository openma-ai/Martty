/**
 * TUI shell Cordis plugin: spawn the native painter and mux ACP + palettes.
 *
 * Injects `acpClient`, `acpSessionConfig`, the TUI registries, and the sibling
 * `tuiCordisClientRunner`.
 * The shell owns the native painter and only binds transport; it does not
 * evaluate dynamic Client code. Does not import dsh or dsh-acp.
 *
 *     dsh-tui --agent dsh-acp
 *     dsh-tui --agent dsh --agent-arg --profile --agent-arg acp
 */

import { nativeBinary, spawnPluginTui } from './spawn-tui.js'
import { CORDIS_METHODS } from './cordis-protocol.js'
import { muxAcpAndCompositor } from './mux.js'

export const name = 'dsh-tui-shell'
export const inject = [
  'acpClient', 'tuiTheme', 'tuiPresets', 'tuiSlots', 'tuiCommands', 'tuiOverlay',
  'acpClientEvents', 'acpSessionConfig', 'tuiQueue', 'tuiAgents', 'tuiCordisClientRunner',
]

const shellStateKey = Symbol.for('@openma/deepseek-harness-tui/shell-state')

/**
 * A module reload must not create a second stdin reader. `globalThis` is the
 * ownership boundary of the Node host process; ESM module locals are not,
 * because Cordis can evaluate a fresh module instance during HMR.
 * @type {{
 *   livePainter: null | ({ muxed: boolean } & Awaited<ReturnType<typeof spawnPluginTui>>),
 *   startingPainter: null | Promise<{ muxed: boolean } & Awaited<ReturnType<typeof spawnPluginTui>>>,
 *   refuseRespawn: boolean,
 *   processExitListener: null | (() => void),
 * }}
 */
const shellState = globalThis[shellStateKey] ??= {
  livePainter: null,
  startingPainter: null,
  refuseRespawn: false,
  processExitListener: null,
}
if (shellState.processExitListener === undefined) shellState.processExitListener = null

/** Drop the sticky painter so tests can apply() more than once in-process. */
export function resetShellForTests() {
  if (shellState.processExitListener !== null) {
    process.off('exit', shellState.processExitListener)
    shellState.processExitListener = null
  }
  shellState.livePainter = null
  shellState.startingPainter = null
  shellState.refuseRespawn = false
}

/**
 * Fiber dispose (HMR / config-watch recompose) SIGTERM is not a user quit.
 * A normal exit or SIGINT is: the host must give the TTY back.
 * @param {NodeJS.Signals | null} signal
 * @returns {boolean}
 */
export function shouldQuitOnPainterExit(signal) {
  return signal !== 'SIGTERM'
}

/** Child raw-mode / alt-screen is the same TTY. Reset it if the painter died dirty. */
export function restoreHostTty() {
  if (process.stdin.isTTY) {
    try {
      process.stdin.setRawMode(false)
    } catch {
      // stdin already closed
    }
  }
  if (process.stdout.isTTY) {
    process.stdout.write(
      '\x1b[?2004l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l',
    )
  }
}

/**
 * @param {object} ctx
 * @param {{ extraArgs?: string[] }} [options]
 */
export function apply(ctx, options = {}) {
  return applyShell(ctx, options)
}

/**
 * @param {object} ctx
 * @param {{ extraArgs?: string[] }} [options]
 */
export async function applyShell(ctx, options = {}) {
  const handle = ctx.tuiTheme ?? ctx.get?.('tuiTheme')
  if (handle === undefined || typeof handle.bindNotify !== 'function'
    || typeof handle.observeSelected !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiTheme must expose bindNotify and observeSelected')
  }
  const slots = ctx.tuiSlots ?? ctx.get?.('tuiSlots')
  if (slots === undefined || typeof slots.bindNotify !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiSlots must expose bindNotify')
  }
  const commands = ctx.tuiCommands ?? ctx.get?.('tuiCommands')
  if (commands === undefined || typeof commands.bindNotify !== 'function'
    || typeof commands.dispatch !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiCommands must expose bindNotify and dispatch')
  }
  const overlay = ctx.tuiOverlay ?? ctx.get?.('tuiOverlay')
  if (overlay === undefined || typeof overlay.bindNotify !== 'function'
    || typeof overlay.dispatch !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiOverlay must expose bindNotify and dispatch')
  }
  const agent = ctx.acpClient ?? ctx.get?.('acpClient')
  if (agent === undefined || agent.stdin === undefined || agent.stdout === undefined) {
    throw new Error('dsh-tui-shell: ctx.acpClient must expose stdin and stdout')
  }
  const clientRunner = ctx.tuiCordisClientRunner ?? ctx.get?.('tuiCordisClientRunner')
  const presets = ctx.tuiPresets ?? ctx.get?.('tuiPresets')
  if (clientRunner === undefined || typeof clientRunner.bindTransport !== 'function'
    || typeof clientRunner.selectTheme !== 'function') {
    throw new Error(
      'dsh-tui-shell: ctx.tuiCordisClientRunner must expose bindTransport and selectTheme',
    )
  }
  const sessionConfig = ctx.acpSessionConfig ?? ctx.get?.('acpSessionConfig')
  if (sessionConfig === undefined || typeof sessionConfig.bindTransport !== 'function'
    || typeof sessionConfig.observeClient !== 'function'
    || typeof sessionConfig.observeAgent !== 'function') {
    throw new Error(
      'dsh-tui-shell: ctx.acpSessionConfig must expose bindTransport and ACP observers',
    )
  }
  const clientEvents = ctx.acpClientEvents ?? ctx.get?.('acpClientEvents')
  if (clientEvents === undefined || typeof clientEvents.observeClient !== 'function'
    || typeof clientEvents.observeAgent !== 'function') {
    throw new Error(
      'dsh-tui-shell: ctx.acpClientEvents must expose ACP observers',
    )
  }
  const queue = ctx.tuiQueue ?? ctx.get?.('tuiQueue')
  if (queue === undefined || typeof queue.observe !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiQueue must expose observe')
  }
  const agents = ctx.tuiAgents ?? ctx.get?.('tuiAgents')
  if (agents === undefined || typeof agents.observe !== 'function'
    || typeof agents.bindNotify !== 'function') {
    throw new Error('dsh-tui-shell: ctx.tuiAgents must expose observe and bindNotify')
  }

  // Config-watch recomposes the tree at boot and disposes this fiber. Keep
  // one painter; do not kill it from ctx.effect or the screen never appears.
  // After a user quit, do not spawn a second empty TUI on the same TTY.
  if (shellState.refuseRespawn) {
    return
  }
  let connection = shellState.livePainter
  if (connection === null || connection.child.exitCode !== null) {
    if (shellState.startingPainter === null) {
      shellState.startingPainter = (async () => {
        const bin = nativeBinary()
        const started = await spawnPluginTui(
          bin,
          options.extraArgs ?? [],
          options.tty,
        )
        const live = { ...started, muxed: false }
        shellState.livePainter = live
        const { child } = live
        child.on('exit', (code, signal) => {
          if (shellState.processExitListener === processExitListener) {
            process.off('exit', processExitListener)
            shellState.processExitListener = null
          }
          try {
            agent.child?.kill('SIGTERM')
          } catch {
            // already gone
          }
          restoreHostTty()
          if (!shouldQuitOnPainterExit(signal)) return
          shellState.refuseRespawn = true
          if (shellState.livePainter?.child === child) shellState.livePainter = null
          process.exit(code ?? (signal === 'SIGINT' ? 130 : 0))
        })
        child.on('error', (err) => {
          console.error(`dsh-tui: ${err.message}`)
          process.exit(1)
        })
        const processExitListener = () => {
          try {
            child.kill('SIGTERM')
          } catch {
            // already gone
          }
        }
        shellState.processExitListener = processExitListener
        process.once('exit', processExitListener)
        return live
      })()
    }
    const starting = shellState.startingPainter
    try {
      connection = await starting
    } finally {
      if (shellState.startingPainter === starting) shellState.startingPainter = null
    }
  }

  if (!connection.muxed) {
    let republishCompositorState = () => {}
    const mux = muxAcpAndCompositor({
      agent,
      tui: connection,
      onHost(message) {
        clientRunner.onHost(message)
      },
      onCordisReady() {
        republishCompositorState()
        clientRunner.sync()
      },
      onAcp(direction, message) {
        if (direction === 'client') {
          clientEvents.observeClient(message)
        } else {
          clientEvents.observeAgent(message)
        }
      },
      onCompositor(message) {
        if (message.method === CORDIS_METHODS.themeSelected) {
          return clientRunner.selectTheme(message.params)
        }
        if (message.method === CORDIS_METHODS.commandInvoke) {
          return commands.dispatch(message.params)
        }
        if (message.method === CORDIS_METHODS.overlayEvent) {
          return overlay.dispatch(message.params)
        }
        if (message.method === CORDIS_METHODS.queueUpdate) {
          return { ok: queue.observe(message.params) }
        }
        if (message.method === CORDIS_METHODS.agentsUpdate) {
          return { ok: agents.observe(message.params) }
        }
        if (message.method === CORDIS_METHODS.approvalRespond) {
          return clientRunner.respondApproval(message.params)
        }
        if (message.method === CORDIS_METHODS.uiSelected) {
          return clientRunner.selectUi(message.params)
        }
        throw new Error(`unsupported Cordis TUI method: ${String(message.method)}`)
      },
    })
    agent.onSwitch?.(() => mux.resetAgent())
    const notifyTui = (method, params) => mux.notifyTui(method, params)
    republishCompositorState = () => {
      handle.bindNotify(notifyTui)
      presets?.bindNotify?.(notifyTui)
      slots.bindNotify(notifyTui)
      commands.bindNotify(notifyTui)
      overlay.bindNotify(notifyTui)
      agents.bindNotify(notifyTui)
    }
    republishCompositorState()
    clientRunner.bindTransport(mux.requestAgent, notifyTui)
    sessionConfig.bindTransport(mux.requestTui)
    connection.resume?.()
    connection.muxed = true
  }
  // Cordis collects apply's return as an effect. Only a disposer or nothing.
}

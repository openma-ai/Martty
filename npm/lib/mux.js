/**
 * Byte mux between an ACP agent and the Rust ACP client.
 *
 * spawn-tui connection: `output` is Node→TUI (Rust reads fd 3), `input` is
 * TUI→Node (Rust writes fd 4). Agent stdout feeds `output` except Client-plane
 * Cordis Client-plane extensions; Rust outgoing on `input` goes
 * to agent stdin except compositor paint methods. Palette notifications are
 * injected toward Rust only. Node-originated JSON-RPC ids stay
 * off the painter.
 */

import { CORDIS_CAPABILITY, CORDIS_METHODS, readCordisCapability } from './cordis-protocol.js'

const COMPOSITOR_METHODS = Object.freeze(new Set([
  CORDIS_METHODS.themeUpdate,
  CORDIS_METHODS.themeRemove,
  CORDIS_METHODS.themeSelected,
  CORDIS_METHODS.slotsUpdate,
  CORDIS_METHODS.commandsUpdate,
  CORDIS_METHODS.overlayUpdate,
  CORDIS_METHODS.commandInvoke,
  CORDIS_METHODS.overlayEvent,
  CORDIS_METHODS.queueUpdate,
  CORDIS_METHODS.agentsUpdate,
  CORDIS_METHODS.agentsSelect,
  CORDIS_METHODS.agentsNavigate,
  CORDIS_METHODS.sessionConfigSet,
  CORDIS_METHODS.approvalRespond,
  CORDIS_METHODS.uiSelected,
]))

/** Agent → TUI Node extras. Not compositor paint; Rust never sees these. */
export const HOST_METHODS = Object.freeze(new Set([
  CORDIS_METHODS.inspectQuery,
  CORDIS_METHODS.inspectQueryResolved,
  CORDIS_METHODS.requestRun,
  CORDIS_METHODS.requestRunResolved,
  CORDIS_METHODS.userRun,
  CORDIS_METHODS.pluginRetract,
]))

/**
 * @param {{ jsonrpc?: unknown, method?: unknown, id?: unknown }} message
 * @returns {boolean}
 */
export function isCompositorMessage(message) {
  return typeof message.method === 'string' && COMPOSITOR_METHODS.has(message.method)
}

/**
 * @param {{ jsonrpc?: unknown, method?: unknown }} message
 * @returns {boolean}
 */
export function isHostMessage(message) {
  return typeof message.method === 'string' && HOST_METHODS.has(message.method)
}

/**
 * Stamp `clientCapabilities._meta.dsh.cordis` so dsh-acp forwards Client jobs.
 * Zed never sets this; unknown agents ignore the extra field.
 * @param {object} message
 * @returns {object}
 */
export function advertiseCordis(message) {
  if (message.method !== 'initialize') return message
  const params = message.params !== null && typeof message.params === 'object' && !Array.isArray(message.params)
    ? { ...message.params }
    : {}
  const caps = params.clientCapabilities !== null && typeof params.clientCapabilities === 'object'
    && !Array.isArray(params.clientCapabilities)
    ? { ...params.clientCapabilities }
    : {}
  const meta = caps._meta !== null && typeof caps._meta === 'object' && !Array.isArray(caps._meta)
    ? { ...caps._meta }
    : {}
  const dsh = meta.dsh !== null && typeof meta.dsh === 'object' && !Array.isArray(meta.dsh)
    ? { ...meta.dsh }
    : {}
  dsh.cordis = { ...CORDIS_CAPABILITY }
  meta.dsh = dsh
  delete meta.tuiInspect
  caps._meta = meta
  params.clientCapabilities = caps
  return { ...message, params }
}

/**
 * @param {import('node:stream').Writable} dest
 * @param {unknown} value
 */
export function writeJsonLine(dest, value) {
  dest.write(`${JSON.stringify(value)}\n`)
}

/**
 * Pipe NDJSON, invoking `onLine` for each complete line (trimmed, non-empty).
 * @param {import('node:stream').Readable} source
 * @param {(line: string) => void} onLine
 */
export function onJsonLines(source, onLine) {
  let buffer = ''
  source.on('data', (chunk) => {
    buffer += chunk.toString('utf8')
    for (;;) {
      const newline = buffer.indexOf('\n')
      if (newline < 0) break
      const line = buffer.slice(0, newline).trim()
      buffer = buffer.slice(newline + 1)
      if (line.length === 0) continue
      onLine(line)
    }
  })
}

/**
 * @param {{
 *   agent: { stdin: import('node:stream').Writable, stdout: import('node:stream').Readable },
 *   tui: { input: import('node:stream').Writable, output: import('node:stream').Readable },
 *   onCompositor?: (message: object) => unknown | Promise<unknown>,
 *   onHost?: (message: object) => void,
 *   onCordisReady?: (capability: { protocol: number }) => void,
 *   onAcp?: (direction: 'client' | 'agent', message: object) => void,
 * }} opts
 * @returns {{
 *   notifyTui: (method: string, params?: object) => void,
 *   requestAgent: (method: string, params?: object) => Promise<unknown>,
 *   requestTui: (method: string, params?: object) => Promise<unknown>,
 *   resetAgent: () => void,
 * }}
 */
export function muxAcpAndCompositor(opts) {
  const { agent, tui, onCompositor, onHost, onCordisReady, onAcp } = opts
  for (const stream of [agent.stdin, agent.stdout, tui.input, tui.output]) {
    stream.on?.('error', () => {})
  }

  /** @type {Map<string, { resolve: (value: unknown) => void, reject: (error: Error) => void }>} */
  const pendingAgent = new Map()
  const pendingTui = new Map()
  let nextHostId = 0
  let nextTuiId = 0
  /** @type {unknown} */
  let initializeId
  /** @type {{ protocol: number } | null} */
  let agentCordis = null

  onJsonLines(agent.stdout, (line) => {
    let message
    try {
      message = JSON.parse(line)
    } catch {
      tui.output.write(`${line}\n`)
      return
    }
    if (message && typeof message === 'object' && pendingAgent.has(message.id)) {
      const waiter = pendingAgent.get(message.id)
      pendingAgent.delete(message.id)
      if (message.error !== undefined) {
        const detail = message.error && typeof message.error === 'object' ? message.error.message : undefined
        waiter.reject(new Error(typeof detail === 'string' ? detail : 'ACP extension request failed'))
      } else {
        waiter.resolve(message.result)
      }
      return
    }
    if (isHostMessage(message)) {
      if (agentCordis !== null) onHost?.(message)
      return
    }
    onAcp?.('agent', message)
    tui.output.write(`${line}\n`)
    if (initializeId !== undefined && message && typeof message === 'object' && message.id === initializeId) {
      initializeId = undefined
      agentCordis = readCordisCapability(message.result)
      if (agentCordis !== null) onCordisReady?.(agentCordis)
    }
  })

  onJsonLines(tui.input, (line) => {
    let message
    try {
      message = JSON.parse(line)
    } catch {
      agent.stdin.write(`${line}\n`)
      return
    }
    if (message && typeof message === 'object' && pendingTui.has(message.id)) {
      const waiter = pendingTui.get(message.id)
      pendingTui.delete(message.id)
      if (message.error !== undefined) {
        const detail = message.error && typeof message.error === 'object' ? message.error.message : undefined
        waiter.reject(new Error(typeof detail === 'string' ? detail : 'TUI compositor request failed'))
      } else {
        waiter.resolve(message.result)
      }
      return
    }
    if (isCompositorMessage(message)) {
      const hasId = message.id !== undefined
      if (onCompositor === undefined) {
        if (hasId) {
          writeJsonLine(tui.output, {
            jsonrpc: '2.0',
            id: message.id,
            error: { code: -32601, message: 'Method not found' },
          })
        }
        return
      }
      Promise.resolve()
        .then(() => onCompositor(message))
        .then((result) => {
          if (hasId) writeJsonLine(tui.output, { jsonrpc: '2.0', id: message.id, result: result ?? null })
        })
        .catch((error) => {
          if (!hasId) return
          writeJsonLine(tui.output, {
            jsonrpc: '2.0',
            id: message.id,
            error: {
              code: -32000,
              message: error instanceof Error ? error.message : String(error),
            },
          })
        })
      return
    }
    if (
      typeof message.method === 'string'
      && message.method.startsWith('_dsh/cordis/')
      && agentCordis === null
    ) {
      if (message.id !== undefined) {
        writeJsonLine(tui.output, {
          jsonrpc: '2.0',
          id: message.id,
          error: { code: -32601, message: 'agent has not advertised _dsh/cordis' },
        })
      }
      return
    }
    const outgoing = advertiseCordis(message)
    if (outgoing.method === 'initialize' && outgoing.id !== undefined) {
      initializeId = outgoing.id
    }
    onAcp?.('client', outgoing)
    writeJsonLine(agent.stdin, outgoing)
  })

  return {
    resetAgent() {
      initializeId = undefined
      agentCordis = null
      for (const waiter of pendingAgent.values()) {
        waiter.reject(new Error('ACP Agent was replaced'))
      }
      pendingAgent.clear()
    },
    notifyTui(method, params) {
      writeJsonLine(
        tui.output,
        params === undefined ? { jsonrpc: '2.0', method } : { jsonrpc: '2.0', method, params },
      )
    },
    requestAgent(method, params) {
      if (method.startsWith('_dsh/cordis/') && agentCordis === null) {
        return Promise.reject(new Error('agent has not advertised _dsh/cordis'))
      }
      const id = `tui-host-${++nextHostId}`
      return new Promise((resolve, reject) => {
        pendingAgent.set(id, { resolve, reject })
        writeJsonLine(
          agent.stdin,
          params === undefined
            ? { jsonrpc: '2.0', id, method }
            : { jsonrpc: '2.0', id, method, params },
        )
      })
    },
    requestTui(method, params) {
      if (typeof method !== 'string' || !method.startsWith('_dsh/cordis/tui/')) {
        return Promise.reject(new Error('requestTui only accepts _dsh/cordis/tui/* methods'))
      }
      const id = `tui-client-${++nextTuiId}`
      return new Promise((resolve, reject) => {
        pendingTui.set(id, { resolve, reject })
        writeJsonLine(
          tui.output,
          params === undefined
            ? { jsonrpc: '2.0', id, method }
            : { jsonrpc: '2.0', id, method, params },
        )
      })
    },
  }
}

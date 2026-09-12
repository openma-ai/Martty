/** ACP connections live as long as their sessions, independently of the default recipe. */
import { PassThrough } from 'node:stream'
import { AsyncLocalStorage } from 'node:async_hooks'
import { createHash } from 'node:crypto'
import { onJsonLines } from './mux.js'
import { readCordisCapability } from './cordis-protocol.js'

const identity = spec => JSON.stringify([spec.command, spec.args ?? [],
  Object.entries(spec.env ?? {}).sort(([a], [b]) => a.localeCompare(b))])
const namespace = spec => createHash('sha256').update(identity(spec)).digest('hex').slice(0, 24)
const setupMethods = new Set(['session/new', 'session/load', 'session/resume'])

export function createAgentPool(initial, { spawnAgent, resolveAgent, diagnosticError, timeoutMs = 20 * 60_000 }) {
  const input = new PassThrough()
  const output = new PassThrough()
  const connections = new Map()
  const sessions = new Map()
  const callbacks = new Map()
  const authMethods = new Map()
  const context = new AsyncLocalStorage()
  let nextConnection = 0
  let nextRequest = 0
  let defaultAgent = initial
  let initializeParams
  let active
  let lastSetup
  let closed = false

  const write = value => { if (!closed) output.write(JSON.stringify(value) + '\n') }
  const writeAgent = (owner, value) => owner.handle.stdin.write(JSON.stringify(value) + '\n')

  function metadata(owner) {
    return {
      id: owner.id,
      agentInfo: owner.init?.agentInfo,
      agentCapabilities: owner.init?.agentCapabilities ?? {},
      authMethods: (owner.init?.authMethods ?? []).map(method => {
        const id = owner.id === 'h1' ? method.id : `${owner.id}:${method.id}`
        authMethods.set(id, { owner, id: method.id })
        return { ...method, id }
      }),
      command: owner.spec.command,
      args: owner.spec.args ?? [],
    }
  }

  function sessionId(owner, raw) {
    if (typeof raw !== 'string' || !raw) return raw
    if (owner.sessions.has(raw)) return owner.sessions.get(raw)
    let id = owner.id === 'h1' ? raw : `martty:${owner.namespace}:${raw}`
    while (sessions.has(id)) id = `martty:${owner.namespace}:${id}`
    owner.sessions.set(raw, id)
    sessions.set(id, { owner, raw })
    return id
  }

  function incomingParams(owner, params) {
    if (params === null || typeof params !== 'object') return params
    const value = { ...params }
    for (const key of ['sessionId', 'session_id', 'parentSessionId']) {
      if (typeof value[key] === 'string') value[key] = sessionId(owner, value[key])
    }
    return value
  }

  function response(owner, message, pending) {
    message = { ...message, _meta: { ...message._meta, marttyConnectionId: owner.id } }
    if (message.error !== undefined) {
      const error = message.error
      const detail = owner.failed?.message === error.message ? owner.failed
        : diagnosticError(new Error(error.message ?? 'ACP request failed'), owner.handle)
      return { ...message, jsonrpc: '2.0', id: pending.id, error: { ...error, message: detail.message,
        data: { ...(error.data && typeof error.data === 'object' ? error.data : { detail: error.data }),
          marttyConnection: { ...metadata(owner), cwd: pending.params?.cwd } } } }
    }
    let result = message.result
    if (pending.method === 'initialize') {
      owner.init = result
      metadata(owner)
    }
    if (setupMethods.has(pending.method) && result && typeof result === 'object') {
      const raw = result.sessionId ?? pending.params?.sessionId
      result = { ...result, sessionId: sessionId(owner, raw),
        _meta: { ...result._meta, marttyConnection: { ...metadata(owner), cwd: pending.params?.cwd } } }
    }
    if (pending.method === 'session/list' && Array.isArray(result?.sessions)) {
      result = { ...result, sessions: result.sessions.map(session => ({ ...session,
        sessionId: sessionId(owner, session.sessionId) })) }
    }
    return { ...message, jsonrpc: '2.0', id: pending.id, result }
  }

  function fail(owner, error) {
    if (owner.failed || closed) return
    owner.failed = diagnosticError(error, owner.handle)
    for (const pending of owner.pending.values()) {
      if (pending.reject) pending.reject(owner.failed)
      else write(response(owner, { error: { code: -32603, message: owner.failed.message } }, pending))
    }
    owner.pending.clear()
  }

  function connection(spec) {
    const key = identity(spec)
    const existing = connections.get(key)
    if (existing && !existing.failed) return existing
    const owner = { id: `h${++nextConnection}`, namespace: namespace(spec), spec, handle: spawnAgent(spec),
      sessions: new Map(), pending: new Map(), init: undefined, initializing: undefined, failed: undefined }
    connections.set(key, owner)
    owner.handle.child.on('error', error => fail(owner, error))
    owner.handle.child.once('close', (code, signal) => fail(owner,
      new Error(`ACP process exited (${signal ?? code ?? 'unknown'}) before completing the request`)))
    onJsonLines(owner.handle.stdout, line => {
      let message
      try { message = JSON.parse(line) } catch { return }
      if (message.id !== undefined && typeof message.method !== 'string') {
        const pending = owner.pending.get(message.id)
        if (!pending) return
        owner.pending.delete(message.id)
        if (pending.resolve) {
          if (message.error) pending.reject(Object.assign(new Error(message.error.message), { acpError: message.error }))
          else pending.resolve(message.result)
        } else write(response(owner, message, pending))
        return
      }
      const forwarded = { ...message, params: incomingParams(owner, message.params) }
      if (message.id !== undefined) {
        const id = `martty-agent-${++nextRequest}`
        callbacks.set(id, { owner, id: message.id })
        forwarded.id = id
      }
      // Origin metadata is consumed locally by the mux, never sent to an Agent.
      forwarded._meta = { ...forwarded._meta, marttyConnectionId: owner.id }
      write(forwarded)
    })
    return owner
  }

  async function initialize(owner) {
    if (owner.failed) throw owner.failed
    if (owner.init) return
    if (!initializeParams) throw new Error('ACP initialize must precede session/new')
    if (!owner.initializing) {
      owner.initializing = new Promise((resolve, reject) => {
        const id = `martty-initialize-${++nextRequest}`
        const timer = setTimeout(() => {
          owner.pending.delete(id)
          reject(new Error(`ACP setup timed out after ${timeoutMs / 1000}s`))
        }, timeoutMs)
        timer.unref?.()
        const settle = callback => value => { clearTimeout(timer); callback(value) }
        owner.pending.set(id, { resolve: settle(resolve), reject: settle(reject) })
        writeAgent(owner, { jsonrpc: '2.0', id, method: 'initialize', params: initializeParams })
      }).then(value => { owner.init = value; metadata(owner) })
        .catch(error => { owner.initializing = undefined; throw error })
    }
    await owner.initializing
  }

  function ownerFor(message) {
    const sid = message.params?.sessionId ?? message.params?.session_id
    if (typeof sid === 'string') {
      const bound = sessions.get(sid)
      if (bound) return bound.owner
      if (!setupMethods.has(message.method)) throw new Error(`Unknown ACP session: ${sid}`)
      const saved = /^martty:([a-f0-9]{24}):([\s\S]+)$/.exec(sid)
      if (saved) {
        const owner = [...connections.values()].find(owner => owner.namespace === saved[1] && !owner.failed)
          ?? (namespace(defaultAgent) === saved[1] ? connection(defaultAgent) : undefined)
        if (!owner) throw new Error('Select the session’s original Harness before resuming it')
        sessions.set(sid, { owner, raw: saved[2] })
        owner.sessions.set(saved[2], sid)
        return owner
      }
    }
    return context.getStore() ?? active
  }

  async function outgoing(message) {
    if (closed) return
    if (typeof message.method !== 'string') {
      const callback = callbacks.get(message.id)
      if (callback) {
        callbacks.delete(message.id)
        writeAgent(callback.owner, { ...message, id: callback.id })
      }
      return
    }
    let owner
    let params = message.params
    try {
      // Capture the recipe before awaiting initialization. Later default edits
      // cannot retarget a session/new request already in flight.
      if (message.method === 'session/new') {
        owner = authMethods.get(params?._meta?.marttyAuthMethod)?.owner ?? connection(defaultAgent)
        lastSetup = owner
        if (params?._meta?.marttyAuthMethod) {
          params = { ...params, _meta: { ...params._meta } }
          delete params._meta.marttyAuthMethod
        }
        await initialize(owner)
      } else if (message.method === 'initialize') {
        initializeParams = structuredClone(params ?? {})
        owner = active
      } else if (message.method === 'authenticate') {
        const method = authMethods.get(params?.methodId)
        owner = method?.owner ?? lastSetup ?? active
        if (method) params = { ...params, methodId: method.id }
      } else {
        owner = ownerFor(message)
        if (setupMethods.has(message.method)) await initialize(owner)
      }
      if (owner.failed) throw owner.failed
      if (message.method.startsWith('_dsh/cordis/') && readCordisCapability(owner.init) === null) {
        throw new Error('agent has not advertised _dsh/cordis')
      }
      if (params && typeof params === 'object') {
        params = { ...params }
        for (const key of ['sessionId', 'session_id']) {
          const bound = sessions.get(params[key])
          if (bound) params[key] = bound.raw
        }
      }
      const forwarded = { ...message, params }
      if (message.id !== undefined) {
        const id = `martty-client-${++nextRequest}`
        owner.pending.set(id, { id: message.id, method: message.method, params })
        forwarded.id = id
      }
      writeAgent(owner, forwarded)
    } catch (error) {
      if (message.id !== undefined) {
        const failed = { error: { ...error.acpError, code: error.acpError?.code ?? -32603, message: error.message } }
        write(owner ? response(owner, failed, message) : { jsonrpc:'2.0', id:message.id, ...failed })
      }
    }
  }

  active = connection(initial)
  onJsonLines(input, line => {
    let message
    try { message = JSON.parse(line) } catch { return }
    void outgoing(message)
  })

  return {
    kind: 'spawn', stdin: input, stdout: output,
    get child() { return active.handle.child },
    get command() { return active.spec.command },
    get args() { return active.spec.args ?? [] },
    get env() { return active.spec.env },
    diagnostics() { return active.handle.diagnostics() },
    setDefaultAgent(spec) { defaultAgent = resolveAgent({ agent: spec }) },
    hasAgent(spec) { const owner = connections.get(identity(spec)); return !!owner && !owner.failed },
    selectSession(id) {
      const bound = sessions.get(id)
      if (bound) active = bound.owner
      else if (!id && lastSetup) active = lastSetup
    },
    capabilityFor(message) {
      const id = message?._meta?.marttyConnectionId
      const owner = id ? [...connections.values()].find(owner => owner.id === id) : ownerFor(message)
      return readCordisCapability(owner?.init)
    },
    withOrigin(message, callback) {
      const id = message?._meta?.marttyConnectionId
      const owner = [...connections.values()].find(owner => owner.id === id)
      return context.run(owner ?? active, callback)
    },
    close() {
      if (closed) return
      for (const owner of connections.values()) {
        fail(owner, new Error('acpClient is closed'))
        try { owner.handle.child.kill('SIGTERM') } catch { /* already exited */ }
      }
      closed = true
      callbacks.clear()
      input.destroy()
      output.destroy()
    },
  }
}

'use strict'
/**
 * dsh plugin runner for the native dsh-tui terminal UI.
 *
 * Install into a profile and launch:
 *
 *     dsh plugin --profile tui add @deepseek-ai-harness/tui
 *     dsh --profile tui
 *
 * The runner spawns the platform-native `dsh-tui` binary with the host TTY
 * (fds 0/1/2 inherited) and serves the SDK JSON-RPC protocol itself over two
 * extra pipes (child fd 3: server→tui frames, child fd 4: tui→server frames).
 * It replicates the stock `@deepseek-ai/dsh-sdk-jsonrpc-server` behavior
 * (initialize / session/prompt / shutdown plus session.event, session.status,
 * subagent.started, subagent.finished notifications) and adds TUI-only
 * methods on top:
 *
 *   tui/catalog       — providers, models, agent presets, current selection
 *   tui/model-info    — reasoning/context/maxTokens metadata for one model
 *   tui/select-model  — per-session live model switch + future-session default
 *   tui/permission    — apply a permission preset to a live session
 *   tui/preset        — pick the agent preset composed on first prompt
 *
 * The agent, tools, persistence, and providers come from the surrounding dsh
 * profile. Stdout of the host process is the TUI's screen — keep stdout
 * loggers out of the profile (the stock sdk server has the same requirement).
 */

const { spawn } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const { JsonRpcLineTransport } = require('@deepseek-ai/dsh-sdk-protocol')
const { createUserMessage } = require('@deepseek-ai/dsh-llm')
const { SessionId } = require('@deepseek-ai/dsh-session')
const { installModelSelection } = require('@deepseek-ai/dsh-agent')
const { carrierKeyOf } = require('@deepseek-ai/dsh-scope')

const name = 'dsh-tui-runner'
// Only the agent factory is required; the LLM seam, agent presets, and
// permission presets are read optionally with ctx.get() (like the stock server).
const inject = ['agents']

function nativeBinary() {
  const key = `${process.platform}-${process.arch}`
  const exe = process.platform === 'win32' ? 'dsh-tui.exe' : 'dsh-tui'
  const bin = path.join(__dirname, '..', 'vendor', key, exe)
  if (!fs.existsSync(bin)) {
    let have = []
    try {
      have = fs.readdirSync(path.join(__dirname, '..', 'vendor'))
    } catch {}
    throw new Error(
      `@deepseek-ai-harness/tui: no native binary for ${key}` +
        ` (packaged: ${have.join(', ') || 'none'});` +
        ' rebuild the package on this machine with scripts/build-npm.sh',
    )
  }
  return bin
}

/** Deployment status mapping for subagent outcomes (stock default: only 'completed' is ok). */
function successStatus(reason) {
  return reason === 'completed' ? 'ok' : 'error'
}

function apply(ctx) {
  const bin = nativeBinary()

  const child = spawn(bin, ['--attach-fds'], {
    stdio: ['inherit', 'inherit', 'inherit', 'pipe', 'pipe'],
  })

  // child fd 3 reads what we write; child fd 4 writes what we read.
  const transport = new JsonRpcLineTransport(child.stdio[4], child.stdio[3])

  // ---------------------------------------------------------------- state --
  const defaults = {
    cwd: process.cwd(),
    provider: 'deepseek-official',
    model: 'deepseek-official',
    maxTokens: undefined,
    reasoningEffort: undefined,
  }
  let llmFiber // fallback adapter fiber mounted by initialize, if any
  /** sessionId → { handle, selection } for sessions this server created. */
  const sessions = new Map()
  /** sessionId → in-flight creation promise (dedupes concurrent prompts). */
  const sessionCreations = new Map()
  /** sessionId → agent preset id to compose on first prompt. */
  const pendingPresets = new Map()
  /** sessionId → permission preset staged before the session exists. */
  const pendingPermissions = new Map()
  /** sessionId → live Session object (every session in the runtime). */
  const sessionObjects = new Map()
  const disposers = []
  let shuttingDown = false
  let shutdownTask

  const requireLlm = () => {
    const llm = ctx.get('llm')
    if (llm === undefined) throw new Error('no llm service is composed in this profile')
    return llm
  }

  const currentSelection = () => ({
    provider: defaults.provider,
    model: defaults.model,
    ...(defaults.reasoningEffort === undefined ? {} : { reasoningEffort: defaults.reasoningEffort }),
  })

  // ----------------------------------------- notification forwarding ------
  // Replicates the stock HarnessSdkJsonRpcServer constructor subscriptions.
  disposers.push(ctx.on('session/event', (session, event) => {
    transport.notify('session.event', { sessionId: String(session.id), event })
  }))
  disposers.push(ctx.on('agent/status', ({ agent, status }) => {
    transport.notify('session.status', { sessionId: String(agent.session.id), status })
  }))
  disposers.push(ctx.on('session/created', (session) => {
    sessionObjects.set(String(session.id), session)
    const parentSession = session.header.parentSession
    if (parentSession === undefined) return
    transport.notify('subagent.started', {
      parentSessionId: String(parentSession),
      childSessionId: String(session.id),
    })
  }))
  disposers.push(ctx.on('session/disposed', (session) => {
    if (sessionObjects.get(String(session.id)) === session) sessionObjects.delete(String(session.id))
  }))
  disposers.push(ctx.on('subagent/end', function (info) {
    // `this` is the service-owned scoped carrier; its key is the delegating agent.
    const parent = carrierKeyOf(this)
    // This protocol reports only in-process child sessions (like the stock server).
    if (!info.local || parent === undefined) return
    transport.notify('subagent.finished', {
      provider: info.provider,
      agentId: String(info.id),
      parentSessionId: String(parent.session.id),
      childSessionId: String(info.id),
      status: successStatus(info.stopReason),
      stopReason: info.stopReason,
      ...(info.lastAssistantMessage === undefined ? {} : { lastAssistantMessage: info.lastAssistantMessage }),
    })
  }))

  // ------------------------------------------------------------ sessions --
  async function createSession(sessionId) {
    const preset = pendingPresets.get(sessionId)
    const selection = { current: currentSelection(), assembled: undefined }
    const handle = await ctx.agents.create({
      sessionId: SessionId(sessionId),
      meta: { cwd: defaults.cwd, ...(preset === undefined ? {} : { agentPreset: preset }) },
      agentOptions: {
        provider: defaults.provider,
        model: defaults.model,
        ...(defaults.maxTokens === undefined ? {} : { maxTokens: defaults.maxTokens }),
      },
      // Pre-publication composition (the api-proxy pattern): selection first,
      // then the preset mount; a mount failure rolls the whole creation back.
      setup: async (agentCtx) => {
        installModelSelection(agentCtx, selection)
        if (preset !== undefined) {
          const presets = ctx.get('agentPresets')
          if (presets === undefined) throw new Error('agent presets unavailable')
          await presets.mount(agentCtx, preset)
        }
      },
    })
    pendingPresets.delete(sessionId)
    const rec = { handle, selection }
    sessions.set(sessionId, rec)
    applyPendingPermission(sessionId, handle)
    return rec
  }

  /** Apply a permission preset staged before the session existed (shift+tab
   * / \/permission ahead of the first prompt). Never fails the prompt: the
   * preset name was validated when it was staged; on a late failure re-echo
   * the session's real preset so the TUI chip stays truthful. */
  function applyPendingPermission(sessionId, handle) {
    const preset = pendingPermissions.get(sessionId)
    if (preset === undefined) return
    pendingPermissions.delete(sessionId)
    const svc = ctx.get('permissionPresets')
    const session = sessionObjects.get(sessionId) ?? handle.agent.session
    if (svc === undefined || session === undefined) return
    try {
      svc.set(session, preset)
    } catch {
      try {
        transport.notify('session.event', {
          sessionId,
          event: { type: 'permission/preset', data: { preset: svc.current(session.events) } },
        })
      } catch {}
    }
  }

  async function getOrCreateSession(sessionId) {
    if (shuttingDown) throw new Error('SDK server is shutting down')
    const existing = sessions.get(sessionId)
    if (existing) return existing
    const pending = sessionCreations.get(sessionId)
    if (pending) return pending
    const creation = createSession(sessionId)
    sessionCreations.set(sessionId, creation)
    creation.then(
      () => { sessionCreations.delete(sessionId) },
      () => { sessionCreations.delete(sessionId) },
    )
    return creation
  }

  // ------------------------------------------------------------- methods --
  async function initialize(params) {
    if (params.maxTokens !== undefined
      && (!Number.isSafeInteger(params.maxTokens) || params.maxTokens <= 0)) {
      throw new TypeError('initialize maxTokens must be a positive safe integer')
    }
    defaults.cwd = path.resolve(String(params.cwd))
    defaults.provider = String(params.provider)
    defaults.model = String(params.model)
    defaults.maxTokens = params.maxTokens
    const llm = ctx.get('llm')
    const hasAdapter = llm !== undefined && llm.listProviders().some((p) => p.id === defaults.provider)
    if (!hasAdapter) {
      if (defaults.provider !== 'deepseek-official') {
        throw new Error(`no adapter registered for provider "${defaults.provider}"`)
      }
      llmFiber = await ctx.plugin(require('@deepseek-ai/dsh-llm-deepseek'), {})
    }
    return { serverInfo: { name: 'dsh-tui-shim', version: '0.2.1' } }
  }

  async function prompt(params) {
    const sessionId = String(params.sessionId)
    const rec = await getOrCreateSession(sessionId)
    // An agent-loop-only reload disposes the loop's agents while this record
    // survives; a retained agent accepts followup() silently, so validate the
    // record against the live registry before delivery (as the stock server does).
    if (ctx.agents.get(rec.handle.agent.id) !== rec.handle.agent) {
      throw new Error(`session agent was disposed outside the server: ${sessionId}`)
    }
    const message = createUserMessage({ content: params.contentBlocks, source: { kind: 'user' } })
    rec.handle.agent.followup(message)
    return { messageId: message.id }
  }

  async function tuiCatalog() {
    const llm = requireLlm()
    const providers = llm.listProviders()
    const models = []
    await Promise.all(providers.map(async (p) => {
      try {
        for (const m of await llm.listModels(p.id)) models.push(m)
      } catch {} // tolerate one provider's listing failure
    }))
    let presets
    const presetService = ctx.get('agentPresets')
    if (presetService !== undefined) {
      try {
        presets = (await presetService.list()).map((p) => ({
          id: p.id,
          name: p.name ?? p.id,
          ...(p.description === undefined ? {} : { description: p.description }),
          ...(p.broken === undefined ? {} : { broken: p.broken }),
        }))
      } catch {} // rosterless/broken discovery → omit presets
    }
    return {
      providers,
      models: models.map((m) => ({
        provider: m.provider,
        id: m.id,
        name: m.name,
        vision: !!(m.inputModalities || []).includes('image'),
      })),
      ...(presets === undefined ? {} : { presets }),
      current: currentSelection(),
    }
  }

  async function tuiModelInfo(params) {
    const info = await requireLlm().resolveModelInfo(String(params.provider), String(params.model))
    return {
      reasoning: info.reasoning ?? null,
      context: info.context ?? null,
      defaultMaxTokens: info.defaultMaxTokens ?? null,
    }
  }

  async function tuiSelectModel(params) {
    const sessionId = String(params.sessionId)
    const rec = sessions.get(sessionId)
    const base = rec !== undefined && rec.selection.current !== undefined
      ? rec.selection.current
      : currentSelection()
    const effort = 'reasoningEffort' in params
      ? (params.reasoningEffort ?? undefined)
      : base.reasoningEffort
    const next = {
      provider: params.provider === undefined ? base.provider : String(params.provider),
      model: params.model === undefined ? base.model : String(params.model),
      ...(effort === undefined ? {} : { reasoningEffort: effort }),
    }
    // Validate the route + effort against the adapter before adopting it; a
    // rejection propagates to the caller as a JSON-RPC error.
    await requireLlm().resolveCallConfig({ ...next })
    if (rec !== undefined) rec.selection.current = next
    defaults.provider = next.provider
    defaults.model = next.model
    defaults.reasoningEffort = next.reasoningEffort
    return { ok: true, current: next }
  }

  async function tuiPermission(params) {
    const svc = ctx.get('permissionPresets')
    if (svc === undefined) {
      throw new Error('no permission-presets service in this profile — compose @deepseek-ai/dsh-permission-presets')
    }
    const sessionId = String(params.sessionId)
    const preset = String(params.preset)
    const session = sessionObjects.get(sessionId)
    if (session !== undefined) {
      svc.set(session, preset) // unknown presets throw with the known list
      return { ok: true, applied: 'live' }
    }
    // The session is created on the first prompt (getOrCreateSession); before
    // that, stage the switch instead of failing. Validate the name now so the
    // user hears about a typo immediately, and echo the staged fact as a
    // session event so the client folds its permission chip right away.
    const names = Array.isArray(svc.names) ? svc.names : undefined
    if (names !== undefined && !names.includes(preset)) {
      throw new Error(`unknown permission preset "${preset}" (known: ${names.join(', ')})`)
    }
    if (pendingPermissions.get(sessionId) !== preset) {
      pendingPermissions.set(sessionId, preset)
      transport.notify('session.event', {
        sessionId,
        event: { type: 'permission/preset', data: { preset } },
      })
    }
    return { ok: true, applied: 'on-first-prompt' }
  }

  function tuiPreset(params) {
    const sessionId = String(params.sessionId)
    if (sessions.has(sessionId) || sessionCreations.has(sessionId)) {
      throw new Error('preset locked after first prompt')
    }
    pendingPresets.set(sessionId, String(params.agentPreset))
    return { ok: true, applied: 'on-first-prompt' }
  }

  // ------------------------------------------------------------ shutdown --
  async function performShutdown() {
    shuttingDown = true
    await Promise.allSettled([...sessionCreations.values()])
    sessionCreations.clear()
    const records = [...sessions.values()]
    sessions.clear()
    while (disposers.length > 0) {
      try {
        disposers.pop()?.()
      } catch {}
    }
    await Promise.allSettled([
      ...records.map((rec) => Promise.resolve().then(() => rec.handle.dispose())),
      ...(llmFiber === undefined ? [] : [Promise.resolve().then(() => llmFiber.dispose())]),
    ])
    llmFiber = undefined
    return {}
  }

  function shutdown() {
    shutdownTask ??= performShutdown()
    return shutdownTask
  }

  // The TUI owns the product lifetime in this profile: after the shutdown
  // response is written, flush, dispose the complete root runtime, and exit
  // (the stock sdk server plugin's disposeAndExit).
  let closing = false
  let exitTask
  const disposeAndExit = (code) => {
    exitTask ??= (async () => {
      closing = true
      await Promise.allSettled([Promise.resolve().then(() => transport.flush())])
      await Promise.allSettled([Promise.resolve().then(() => ctx.root.fiber.dispose())])
      process.exit(code)
    })()
    return exitTask
  }

  // ------------------------------------------------------------ dispatch --
  transport.onRequest(async (method, params) => {
    let result
    switch (method) {
      case 'initialize':
        result = await initialize(params)
        break
      case 'session/prompt':
        result = await prompt(params)
        break
      case 'shutdown':
        result = await shutdown()
        // Run after the handler result is written; the task then flushes,
        // disposes the root runtime, and exits.
        setImmediate(() => { void disposeAndExit(0) })
        break
      case 'tui/catalog':
        result = await tuiCatalog(params)
        break
      case 'tui/model-info':
        result = await tuiModelInfo(params)
        break
      case 'tui/select-model':
        result = await tuiSelectModel(params)
        break
      case 'tui/permission':
        result = await tuiPermission(params)
        break
      case 'tui/preset':
        result = tuiPreset(params)
        break
      default:
        throw new Error(`unknown method: ${method}`)
    }
    return result
  })

  // ------------------------------------------------------ child lifecycle --
  child.on('exit', (code) => {
    if (closing) return
    void disposeAndExit(code === null ? 0 : code)
  })
  child.on('error', (err) => {
    if (closing) return
    closing = true
    console.error(`dsh-tui runner failed: ${err.message}`)
    process.exit(1)
  })

  ctx.effect(() => {
    transport.start()
    return async () => {
      closing = true
      try {
        child.kill('SIGTERM')
      } catch {}
      await shutdown().catch(() => {})
      transport.close()
    }
  }, 'dsh-tui.serve')
}

module.exports = { name, inject, apply }

#!/usr/bin/env node
/**
 * Regression test for the npm runner's tui/permission staging (shift+tab /
 * /permission before the first prompt).
 *
 * Sessions are created lazily on the first session/prompt, so a permission
 * switch used to fail with "permission presets unavailable" until then. The
 * runner now stages the preset, echoes a synthetic permission/preset session
 * event (so the TUI chip folds immediately), and applies it via
 * permissionPresets.set() when the session is created.
 *
 * Runs npm/lib/index.js for real with the @deepseek-ai deps and child_process
 * stubbed through Module._load.
 */

import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const require = createRequire(import.meta.url)
const Module = require('node:module')

// ------------------------------------------------------------------ stubs --
const state = {
  transport: null,
  notifications: [],
  setCalls: [],
}

class FakeTransport {
  constructor() {
    state.transport = this
  }
  onRequest(fn) {
    this.handler = fn
  }
  notify(method, params) {
    state.notifications.push({ method, params })
  }
  start() {}
  flush() {}
  close() {}
}

const stubs = {
  '@deepseek-ai/dsh-sdk-protocol': { JsonRpcLineTransport: FakeTransport },
  '@deepseek-ai/dsh-llm': { createUserMessage: (m) => ({ id: 'msg-1', ...m }) },
  '@deepseek-ai/dsh-session': { SessionId: (s) => s },
  '@deepseek-ai/dsh-agent': { installModelSelection: () => {} },
  '@deepseek-ai/dsh-scope': { carrierKeyOf: () => undefined },
}
const fakeChild = {
  stdio: [null, null, null, {}, {}],
  on() {},
  kill() {},
}
const origLoad = Module._load
Module._load = function (request, ...rest) {
  if (request in stubs) return stubs[request]
  if (request === 'node:child_process' || request === 'child_process') {
    return { spawn: () => fakeChild }
  }
  return origLoad.call(this, request, ...rest)
}

// ------------------------------------------------------------- mock cordis --
function makeCtx({ withPermissionService = true } = {}) {
  const handlers = new Map() // event → [fn]
  const agents = new Map()
  const permissionSvc = {
    get names() {
      return ['workspace-write', 'danger-full-access']
    },
    set(session, name) {
      if (!this.names.includes(name)) {
        throw new Error(`permission: unknown preset "${name}" (known: ${this.names.join(', ')})`)
      }
      state.setCalls.push({ sessionId: session.id, preset: name })
    },
    current() {
      return 'workspace-write'
    },
  }
  const ctx = {
    get(name) {
      if (name === 'permissionPresets') return withPermissionService ? permissionSvc : undefined
      return undefined
    },
    on(event, fn) {
      const list = handlers.get(event) ?? []
      list.push(fn)
      handlers.set(event, list)
      return () => {}
    },
    effect(fn) {
      fn()
    },
    agents: {
      async create({ sessionId, setup }) {
        const session = { id: sessionId, events: [], header: {} }
        const agent = { id: `agent-${sessionId}`, session, followup() {} }
        agents.set(agent.id, agent)
        await setup({})
        // the runtime publishes session/created during creation
        for (const fn of handlers.get('session/created') ?? []) fn(session)
        return { agent, dispose() {} }
      },
      get(id) {
        return agents.get(id)
      },
    },
    root: { fiber: { dispose() {} } },
    plugin: async () => ({ dispose() {} }),
  }
  return ctx
}

// -------------------------------------------------------------------- test --
const runner = require(path.join(root, 'npm', 'lib', 'index.js'))

// 1) staged switch before the session exists
runner.apply(makeCtx())
const request = (method, params) => state.transport.handler(method, params)

let res = await request('tui/permission', { sessionId: 's1', preset: 'danger-full-access' })
assert.deepEqual(res, { ok: true, applied: 'on-first-prompt' })
assert.equal(state.setCalls.length, 0, 'nothing applied yet — no session')
assert.deepEqual(state.notifications, [
  {
    method: 'session.event',
    params: { sessionId: 's1', event: { type: 'permission/preset', data: { preset: 'danger-full-access' } } },
  },
], 'staged fact echoed so the TUI chip folds immediately')

// 2) restaging the same preset does not duplicate the echo
res = await request('tui/permission', { sessionId: 's1', preset: 'danger-full-access' })
assert.deepEqual(res, { ok: true, applied: 'on-first-prompt' })
assert.equal(state.notifications.length, 1)

// 3) unknown presets are rejected at staging time, listing the table
await assert.rejects(
  request('tui/permission', { sessionId: 's1', preset: 'bogus' }),
  /unknown permission preset "bogus" \(known: workspace-write, danger-full-access\)/,
)

// 4) first prompt creates the session and applies the staged preset
res = await request('session/prompt', { sessionId: 's1', contentBlocks: [{ type: 'text', text: 'hi' }] })
assert.equal(res.messageId, 'msg-1')
assert.deepEqual(state.setCalls, [{ sessionId: 's1', preset: 'danger-full-access' }])

// 5) once the session is live, switches apply directly
res = await request('tui/permission', { sessionId: 's1', preset: 'workspace-write' })
assert.deepEqual(res, { ok: true, applied: 'live' })
assert.deepEqual(state.setCalls.at(-1), { sessionId: 's1', preset: 'workspace-write' })

// 6) a profile without the service says so, instead of "unavailable"
state.notifications.length = 0
runner.apply(makeCtx({ withPermissionService: false }))
await assert.rejects(
  request('tui/permission', { sessionId: 's2', preset: 'workspace-write' }),
  /no permission-presets service in this profile/,
)

console.log('ok — tui/permission staging behaves')

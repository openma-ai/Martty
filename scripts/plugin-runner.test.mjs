import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { registerHooks } from 'node:module'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'
import { state } from './plugin-runner-harness.mjs'

const repoRoot = path.resolve(import.meta.dirname, '..')
const packageJsonPath = path.join(repoRoot, 'npm', 'package.json')
const runnerPath = path.join(repoRoot, 'npm', 'lib', 'index.js')
const harnessUrl = pathToFileURL(path.join(repoRoot, 'scripts', 'plugin-runner-harness.mjs')).href

const stubSources = {
  '@deepseek-ai/dsh-sdk-protocol': `import { FakeTransport } from ${JSON.stringify(harnessUrl)}; export { FakeTransport as JsonRpcLineTransport }`,
  '@deepseek-ai/dsh-llm': `export function createUserMessage(m) { return { id: 'msg-1', ...m } }`,
  '@deepseek-ai/dsh-session': `export function SessionId(s) { return s }`,
  '@deepseek-ai/dsh-agent': `export function installModelSelection() {}`,
  '@deepseek-ai/dsh-scope': `export function carrierKeyOf() { return undefined }`,
  '@deepseek-ai/dsh-llm-deepseek': `export const name = 'llm-deepseek'; export function apply() {}`,
  'node:child_process': `import { fakeChild } from ${JSON.stringify(harnessUrl)}; export function spawn() { return fakeChild }`,
}

registerHooks({
  resolve(specifier, context, nextResolve) {
    const fromRunner = context.parentURL?.includes('/npm/lib/index.js')
    if (fromRunner && specifier in stubSources) {
      return {
        url: `data:text/javascript,${encodeURIComponent(stubSources[specifier])}`,
        shortCircuit: true,
      }
    }
    return nextResolve(specifier, context)
  },
})

const runner = await import(pathToFileURL(runnerPath).href)

test('the published plugin is ESM so Cordis can import() it in parallel', () => {
  const pkg = JSON.parse(readFileSync(packageJsonPath, 'utf8'))
  assert.equal(pkg.type, 'module')
  const source = readFileSync(runnerPath, 'utf8')
  assert.match(source, /^import /m)
  assert.match(source, /export function apply/)
  assert.doesNotMatch(source, /module\.exports/)
  assert.doesNotMatch(source, /require\(['"]@deepseek-ai\//)
})

function makeCtx({ withPermissionService = true } = {}) {
  const handlers = new Map()
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
  return {
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
}

function ensureTestNative() {
  const key = `${process.platform}-${process.arch}`
  const exe = process.platform === 'win32' ? 'dsh-tui.exe' : 'dsh-tui'
  const dir = path.join(repoRoot, 'npm', 'vendor', key)
  const bin = path.join(dir, exe)
  if (existsSync(bin)) return () => {}
  mkdirSync(dir, { recursive: true })
  writeFileSync(bin, '')
  return () => rmSync(bin, { force: true })
}

test('tui/permission stages before the first prompt and applies on create', async () => {
  const restore = ensureTestNative()
  try {
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
  ])

  res = await request('tui/permission', { sessionId: 's1', preset: 'danger-full-access' })
  assert.deepEqual(res, { ok: true, applied: 'on-first-prompt' })
  assert.equal(state.notifications.length, 1)

  await assert.rejects(
    request('tui/permission', { sessionId: 's1', preset: 'bogus' }),
    /unknown permission preset "bogus" \(known: workspace-write, danger-full-access\)/,
  )

  res = await request('session/prompt', { sessionId: 's1', contentBlocks: [{ type: 'text', text: 'hi' }] })
  assert.equal(res.messageId, 'msg-1')
  assert.deepEqual(state.setCalls, [{ sessionId: 's1', preset: 'danger-full-access' }])

  res = await request('tui/permission', { sessionId: 's1', preset: 'workspace-write' })
  assert.deepEqual(res, { ok: true, applied: 'live' })
  assert.deepEqual(state.setCalls.at(-1), { sessionId: 's1', preset: 'workspace-write' })

  state.notifications.length = 0
  runner.apply(makeCtx({ withPermissionService: false }))
  await assert.rejects(
    request('tui/permission', { sessionId: 's2', preset: 'workspace-write' }),
    /no permission-presets service in this profile/,
  )
  } finally {
    restore()
  }
})

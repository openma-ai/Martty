// Phase 0 smoke (issue #94): verify the dsh-acp ACP server supports multiple
// concurrent sessions on ONE stdio connection:
//   - two session/new sessions, prompted concurrently via bare sessionId
//   - session/update notifications interleave, demultiplexed by sessionId
//   - probe session/load (README says rejected) and session/resume
//
// Opt-in only — consumes a small amount of model tokens. Not part of npm test.
// Usage: node scripts/acp-multi-session.smoke.mjs [path-to-dsh-acp]

import { spawn } from 'node:child_process'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

const agent = process.argv[2]
  ?? path.join(process.env.HOME, '.dsh/profiles/martty/node_modules/.bin/dsh-acp')
const cwd = mkdtempSync(path.join(tmpdir(), 'martty-acp-multi-'))

const child = spawn(agent, [], { stdio: ['pipe', 'pipe', 'inherit'] })
let buf = ''
let nextId = 1
const pending = new Map()
const updates = new Map() // sessionId -> { count, kinds: Map }
const notifications = []

function send(method, params) {
  const id = nextId++
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n')
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }))
}

function recordUpdate(params) {
  const sid = params?.sessionId ?? '<none>'
  const kind = params?.update?.sessionUpdate ?? '?'
  if (!updates.has(sid)) updates.set(sid, { count: 0, kinds: new Map() })
  const rec = updates.get(sid)
  rec.count++
  rec.kinds.set(kind, (rec.kinds.get(kind) ?? 0) + 1)
}

child.stdout.on('data', (chunk) => {
  buf += chunk.toString('utf8')
  for (;;) {
    const nl = buf.indexOf('\n')
    if (nl < 0) break
    const line = buf.slice(0, nl).trim()
    buf = buf.slice(nl + 1)
    if (!line) continue
    let msg
    try { msg = JSON.parse(line) } catch { continue }
    if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined)) {
      const p = pending.get(msg.id)
      if (p) {
        pending.delete(msg.id)
        if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.code} ${msg.error.message}`))
        else p.resolve(msg.result)
      }
    } else if (msg.id !== undefined && msg.method) {
      // Agent -> client request. Auto-approve permissions with the first
      // allow option; answer anything else with a generic empty result.
      if (msg.method === 'session/request_permission') {
        const options = msg.params?.options ?? []
        const allow = options.find((o) => String(o.kind ?? '').includes('allow')) ?? options[0]
        child.stdin.write(JSON.stringify({
          jsonrpc: '2.0', id: msg.id,
          result: { outcome: { outcome: 'selected', optionId: allow?.optionId } },
        }) + '\n')
      } else {
        child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: {} }) + '\n')
      }
    } else if (msg.method === 'session/update') {
      recordUpdate(msg.params)
      notifications.push(msg.params)
    }
  }
})

function withTimeout(promise, ms, label) {
  return Promise.race([
    promise,
    new Promise((_, reject) => setTimeout(() => reject(new Error(`${label}: timeout after ${ms}ms`)), ms)),
  ])
}

const failures = []
function check(name, cond, detail = '') {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`)
  if (!cond) failures.push(name)
}

try {
  const init = await withTimeout(send('initialize', {
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
  }), 30000, 'initialize')
  console.log('initialize ok. agentCapabilities:', JSON.stringify(init.agentCapabilities ?? {}))
  console.log('authMethods:', JSON.stringify(init.authMethods ?? []))

  const s1 = await withTimeout(send('session/new', { cwd, mcpServers: [] }), 60000, 'session/new #1')
  const s2 = await withTimeout(send('session/new', { cwd, mcpServers: [] }), 60000, 'session/new #2')
  const sid1 = s1.sessionId
  const sid2 = s2.sessionId
  console.log(`sessions: ${sid1} / ${sid2}`)
  check('two distinct sessions', sid1 && sid2 && sid1 !== sid2)

  const marker1 = 'ALPHA_ONE'
  const marker2 = 'BRAVO_TWO'
  const prompt = (sessionId, marker) => send('session/prompt', {
    sessionId,
    prompt: [{ type: 'text', text: `Reply with exactly the token ${marker} and nothing else.` }],
  })
  const [r1, r2] = await withTimeout(
    Promise.all([prompt(sid1, marker1), prompt(sid2, marker2)]),
    180000, 'concurrent prompts',
  )
  console.log(`prompt responses: ${JSON.stringify(r1)} / ${JSON.stringify(r2)}`)

  const u1 = updates.get(sid1)
  const u2 = updates.get(sid2)
  check('session #1 streamed updates on this connection', !!u1 && u1.count > 0,
    u1 ? `${u1.count} updates: ${[...u1.kinds.entries()].map(([k, n]) => `${k}×${n}`).join(', ')}` : 'none')
  check('session #2 streamed updates on this connection', !!u2 && u2.count > 0,
    u2 ? `${u2.count} updates: ${[...u2.kinds.entries()].map(([k, n]) => `${k}×${n}`).join(', ')}` : 'none')
  const foreign = [...updates.keys()].filter((k) => k !== sid1 && k !== sid2)
  check('no updates tagged with unknown session ids', foreign.length === 0, foreign.join(','))

  const text1 = notifications.filter((n) => n.sessionId === sid1)
    .map((n) => n.update?.content?.text ?? '').join('')
  const text2 = notifications.filter((n) => n.sessionId === sid2)
    .map((n) => n.update?.content?.text ?? '').join('')
  check('session #1 answered its own marker', text1.includes(marker1), JSON.stringify(text1.slice(0, 80)))
  check('session #2 answered its own marker', text2.includes(marker2), JSON.stringify(text2.slice(0, 80)))

  const list = await withTimeout(send('session/list', {}), 30000, 'session/list')
  const listed = (list.sessions ?? []).map((s) => s.sessionId)
  check('session/list contains both sessions', listed.includes(sid1) && listed.includes(sid2),
    `${listed.length} listed`)

  // Probes: record behavior, do not fail the smoke on the outcome.
  try {
    const r = await withTimeout(send('session/load', { sessionId: sid1, cwd, mcpServers: [] }), 30000, 'session/load')
    console.log('probe session/load: SUCCEEDED', JSON.stringify(r))
  } catch (err) {
    console.log(`probe session/load: rejected — ${err.message}`)
  }
  try {
    await withTimeout(send('session/close', { sessionId: sid2 }), 30000, 'session/close')
    const r = await withTimeout(send('session/resume', { sessionId: sid2, cwd, mcpServers: [] }), 60000, 'session/resume')
    console.log('probe session/resume after close: SUCCEEDED', JSON.stringify(r))
  } catch (err) {
    console.log(`probe session/resume: ${err.message}`)
  }
} catch (err) {
  console.error(`smoke aborted: ${err.message}`)
  failures.push('aborted')
} finally {
  child.kill('SIGTERM')
}

console.log(failures.length === 0 ? '\nSMOKE OK' : `\nSMOKE FAILED: ${failures.join(', ')}`)
process.exit(failures.length === 0 ? 0 : 1)

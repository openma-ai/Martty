import assert from 'node:assert/strict'
import test from 'node:test'
import { createHostNoticeServer } from '../npm/lib/host-notice-agent.js'
import { hostNoticeMessage } from '../npm/lib/acp-host.js'
import { apply as applyLaunchNotice } from '../npm/lib/launch-notice.js'
import { isUntested, requirementOf, satisfiesRange, DSH_FLOOR } from '../npm/lib/dsh-compat.js'

test('Martty declares a dsh floor, not an exact list, and evaluates it like dsh', () => {
  assert.match(DSH_FLOOR, /^>=\d/)
  assert.equal(satisfiesRange('0.2.1-rc.2', DSH_FLOOR), true)
  assert.equal(satisfiesRange('9.0.0', DSH_FLOOR), true)
  assert.equal(satisfiesRange('0.1.0', DSH_FLOOR), false)
  assert.equal(isUntested('9.0.0'), true)
  assert.equal(isUntested('0.2.0-rc.2'), false)
  assert.equal(requirementOf({ engines: { dsh: '>=1' }, peerDependencies: { '@deepseek-ai/dsh': '1.0.0' } }), '>=1')
  assert.equal(requirementOf({ peerDependencies: { '@deepseek-ai/dsh': '1.0.0' } }), '1.0.0')
})

test('the host notice message names the dsh, the unmet floors, and the ways out', () => {
  const text = hostNoticeMessage({
    dsh: '0.1.0', dshPath: '/x/package.json', reason: 'Because.',
    unmet: [{ name: 'martty', version: '1.0.0', range: '>=0.1.7-rc.2' }],
  })
  assert.match(text, /Installed dsh: `0\.1\.0`/)
  assert.match(text, /`martty` 1\.0\.0 supports dsh `>=0\.1\.7-rc\.2`/)
  assert.match(text, /npx martty/)
  assert.match(text, /npm install -g @deepseek-ai\/dsh@latest/)
  assert.match(text, /Because\./)
})

test('the host notice ACP agent answers initialize, session/new, and prompts with the message', async () => {
  const toAgent = new TransformStream()
  const fromAgent = new TransformStream()
  const server = createHostNoticeServer('why', { version: '1.2.3' })
  const connection = server.connect({ readable: toAgent.readable, writable: fromAgent.writable })
  const writer = toAgent.writable.getWriter()
  const reader = fromAgent.readable.getReader()
  const next = async () => (await reader.read()).value

  await writer.write({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: 1 } })
  const init = await next()
  assert.equal(init.result.protocolVersion, 1)
  assert.equal(init.result.agentInfo.name, 'martty-host-notice')

  await writer.write({ jsonrpc: '2.0', id: 2, method: 'session/new', params: { cwd: '/', mcpServers: [] } })
  const created = await next()
  assert.equal(typeof created.result.sessionId, 'string')
  const shown = await next()
  assert.equal(shown.method, 'session/update')
  assert.equal(shown.params.update.content.text, 'why')

  await writer.write({ jsonrpc: '2.0', id: 3, method: 'session/prompt', params: { sessionId: created.result.sessionId, prompt: [] } })
  assert.equal((await next()).params.update.content.text, 'why')
  assert.deepEqual((await next()).result, { stopReason: 'end_turn' })

  await writer.write({ jsonrpc: '2.0', id: 4, method: 'session/load', params: {} })
  assert.equal((await next()).error.code, -32601)
  await connection.dispose()
})

test('the launch notice docks once and /dismiss removes it', async () => {
  const registered = []
  let disposed = 0
  let handler
  let stopped = 0
  const ctx = {
    tuiSlots: {
      register(options, nodes) {
        registered.push({ options, nodes })
        return { update() {}, dispose() { disposed += 1 } }
      },
    },
    tuiCommands: {
      register(options, fn) {
        assert.equal(options.name, 'dismiss')
        handler = fn
        return () => { stopped += 1 }
      },
    },
  }
  assert.equal(applyLaunchNotice(ctx, {}), undefined)
  const dispose = applyLaunchNotice(ctx, { text: 'fell back', level: 'warn', dismissAfterMs: 0 })
  assert.equal(registered[0].options.name, 'conversation.input.dock')
  assert.equal(registered[0].nodes[0].kind, 'notice')
  assert.match(registered[0].nodes[0].text, /fell back/)
  await handler()
  dispose()
  assert.equal(disposed, 1)
  assert.equal(stopped, 1)
})

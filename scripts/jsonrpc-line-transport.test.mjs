import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import test from 'node:test'
import { JsonRpcLineTransport } from '../npm/lib/jsonrpc-line-transport.js'

test('answers a JSON-RPC request and emits notifications as NDJSON', async () => {
  const input = new PassThrough()
  const output = new PassThrough()
  const chunks = []
  output.on('data', (c) => { chunks.push(String(c)) })

  const transport = new JsonRpcLineTransport(input, output)
  transport.onRequest(async (method, params) => {
    assert.equal(method, 'initialize')
    assert.equal(params.cwd, '/tmp')
    return { serverInfo: { name: 'test' } }
  })
  transport.start()
  input.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { cwd: '/tmp' } }) + '\n')
  await new Promise((resolve) => setImmediate(resolve))

  const reply = JSON.parse(chunks.join('').trim())
  assert.equal(reply.id, 1)
  assert.deepEqual(reply.result, { serverInfo: { name: 'test' } })

  chunks.length = 0
  transport.notify('session.status', { sessionId: 's1', status: 'idle' })
  const note = JSON.parse(chunks.join('').trim())
  assert.equal(note.method, 'session.status')
  assert.equal(note.params.sessionId, 's1')

  transport.close()
})

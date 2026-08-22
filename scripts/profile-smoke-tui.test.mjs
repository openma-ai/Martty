import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createServer } from 'node:net'
import path from 'node:path'
import test from 'node:test'

const fixture = path.join(import.meta.dirname, 'profile-smoke-tui.mjs')

test('profile smoke painter completes the authenticated Windows TCP flow', async () => {
  const token = 'profile-smoke-test-token'
  let resolveCompleted
  let rejectCompleted
  const completed = new Promise((resolve, reject) => {
    resolveCompleted = resolve
    rejectCompleted = reject
  })
  const server = createServer((socket) => {
    let buffer = ''
    let authenticated = false
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8')
      for (;;) {
        const newline = buffer.indexOf('\n')
        if (newline < 0) break
        const line = buffer.slice(0, newline).trim()
        buffer = buffer.slice(newline + 1)
        if (!authenticated) {
          try {
            assert.equal(line, token)
            authenticated = true
          } catch (error) {
            rejectCompleted(error)
          }
          continue
        }
        const message = JSON.parse(line)
        if (message.id === 1) {
          socket.write(`${JSON.stringify({
            jsonrpc: '2.0',
            id: 1,
            result: { protocolVersion: 1, agentInfo: { name: 'smoke-agent' } },
          })}\n`)
        } else if (message.id === 2) {
          socket.write(`${JSON.stringify({
            jsonrpc: '2.0',
            id: 2,
            result: {
              sessionId: 'smoke-session',
              configOptions: [{ id: 'agent', currentValue: 'standard' }],
            },
          })}\n`)
          resolveCompleted()
        }
      }
    })
    socket.on('error', rejectCompleted)
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  assert.notEqual(address, null)
  assert.equal(typeof address, 'object')

  const child = spawn(process.execPath, [
    fixture,
    '--attach-tcp',
    `127.0.0.1:${address.port}`,
  ], {
    env: { ...process.env, DSH_TUI_ATTACH_TOKEN: token },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let stdout = ''
  let stderr = ''
  const childExit = new Promise((resolve) => child.once('exit', (...args) => resolve(args)))
  child.stdout.on('data', (chunk) => { stdout += chunk.toString('utf8') })
  child.stderr.on('data', (chunk) => { stderr += chunk.toString('utf8') })

  try {
    let timeout
    const timedOut = new Promise((_, reject) => {
      timeout = setTimeout(() => reject(new Error('TCP profile smoke timed out')), 5_000)
      timeout.unref?.()
    })
    await Promise.race([completed, timedOut])
    clearTimeout(timeout)
    const [code] = await childExit
    assert.equal(code, 0, stderr)
    assert.match(stdout, /"marker":"PROFILE_SMOKE_OK"/)
  } finally {
    server.close()
    if (child.exitCode === null) child.kill('SIGTERM')
  }
})

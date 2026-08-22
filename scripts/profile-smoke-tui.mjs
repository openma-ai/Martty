#!/usr/bin/env node

/**
 * Minimal native-painter stand-in for profile installation smoke tests.
 *
 * The profile runner gives the real Rust painter fd 3 (Node -> TUI) and fd 4
 * (TUI -> Node).  This fixture speaks the same NDJSON boundary, completes a
 * real ACP initialize + session/new round trip, then exits so the profile
 * process has an observable terminal condition.
 */

import { createReadStream, createWriteStream } from 'node:fs'
import { createConnection } from 'node:net'

const tcpIndex = process.argv.indexOf('--attach-tcp')
let incoming
let outgoing
if (tcpIndex >= 0) {
  const address = process.argv[tcpIndex + 1]
  const separator = address?.lastIndexOf(':') ?? -1
  const token = process.env.DSH_TUI_ATTACH_TOKEN
  if (separator <= 0 || token === undefined) {
    throw new Error('profile-smoke-tui requires a TCP address and attach token')
  }
  const socket = createConnection({
    host: address.slice(0, separator),
    port: Number(address.slice(separator + 1)),
  })
  await new Promise((resolve, reject) => {
    socket.once('connect', resolve)
    socket.once('error', reject)
  })
  socket.write(`${token}\n`)
  incoming = socket
  outgoing = socket
} else if (process.argv.includes('--attach-fds')) {
  incoming = createReadStream(null, { fd: 3, autoClose: false })
  outgoing = createWriteStream(null, { fd: 4, autoClose: false })
} else {
  throw new Error('profile-smoke-tui requires --attach-fds or --attach-tcp')
}
let buffer = ''
let initialized

const timeout = setTimeout(() => {
  console.error('PROFILE_SMOKE_TIMEOUT')
  process.exit(1)
}, 60_000)

function send(id, method, params) {
  outgoing.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`)
}

function fail(message) {
  clearTimeout(timeout)
  console.error(`PROFILE_SMOKE_FAILED ${message}`)
  process.exit(1)
}

function accept(message) {
  if (message?.id === 1) {
    if (message.error !== undefined) {
      fail(`initialize: ${message.error?.message ?? JSON.stringify(message.error)}`)
      return
    }
    initialized = message.result
    if (initialized?.protocolVersion !== 1) {
      fail(`initialize returned protocol ${String(initialized?.protocolVersion)}`)
      return
    }
    send(2, 'session/new', { cwd: process.cwd(), mcpServers: [] })
    return
  }
  if (message?.id !== 2) return
  if (message.error !== undefined) {
    fail(`session/new: ${message.error?.message ?? JSON.stringify(message.error)}`)
    return
  }
  const session = message.result
  if (typeof session?.sessionId !== 'string' || session.sessionId.length === 0) {
    fail('session/new returned no sessionId')
    return
  }
  const agentOption = session.configOptions?.find?.((option) => option?.id === 'agent')
  if (agentOption === undefined) {
    fail('session/new exposed no Agent preset config option')
    return
  }
  clearTimeout(timeout)
  console.log(JSON.stringify({
    marker: 'PROFILE_SMOKE_OK',
    agent: initialized?.agentInfo?.name,
    preset: agentOption.currentValue,
  }))
  process.exit(0)
}

incoming.on('data', (chunk) => {
  buffer += chunk.toString('utf8')
  for (;;) {
    const newline = buffer.indexOf('\n')
    if (newline < 0) break
    const line = buffer.slice(0, newline).trim()
    buffer = buffer.slice(newline + 1)
    if (line.length === 0) continue
    try {
      accept(JSON.parse(line))
    } catch (error) {
      fail(error instanceof Error ? error.message : String(error))
    }
  }
})

incoming.on('error', (error) => fail(error.message))
outgoing.on('error', (error) => fail(error.message))

send(1, 'initialize', {
  protocolVersion: 1,
  clientCapabilities: {
    fs: { readTextFile: false, writeTextFile: false },
    _meta: { dsh: { cordis: { protocol: 0 } } },
  },
  clientInfo: { name: 'dsh-profile-smoke', version: '0' },
})

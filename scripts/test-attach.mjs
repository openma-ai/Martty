#!/usr/bin/env node
/**
 * Attach-mode wire test: emulates the dsh plugin host role implemented by
 * npm/lib/index.js and drives the native `dsh-tui --attach-fds` binary end to
 * end over fds 3/4.
 *
 * Needs a TTY on stdio (run under `script -q /dev/null …` in CI-ish shells).
 * DSH_TUI_AUTOPROMPT makes the TUI submit one prompt on startup, so the full
 * initialize → session/prompt → notifications round-trip happens untouched.
 */

import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const bin = process.env.DSH_TUI_BIN ?? path.join(root, 'target', 'debug', 'dsh-tui')

const child = spawn(bin, ['--attach-fds'], {
  stdio: ['inherit', 'inherit', 'inherit', 'pipe', 'pipe'],
  env: { ...process.env, DSH_TUI_AUTOPROMPT: 'ping from the attach test' },
})

const toTui = child.stdio[3]
const fromTui = child.stdio[4]
const seen = []
const notify = (method, params) =>
  toTui.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n')
const reply = (id, result) =>
  toTui.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n')

const S = 'attach-test-session'
const rl = createInterface({ input: fromTui })
rl.on('line', (line) => {
  if (!line.trim()) return
  let msg
  try {
    msg = JSON.parse(line)
  } catch {
    return
  }
  if (msg.method === undefined) return
  seen.push(msg.method)
  if (msg.method === 'initialize') {
    toTui.write(
      JSON.stringify({
        jsonrpc: '2.0',
        id: msg.id,
        result: { serverInfo: { name: 'attach-test-host', version: '0.0.0' } },
      }) + '\n',
    )
  } else if (msg.method === 'tui/catalog') {
    reply(msg.id, {
      providers: [{ id: 'deepseek-official', name: 'DeepSeek' }],
      models: [
        { provider: 'deepseek-official', id: 'attach-test', name: 'Attach Test', vision: false },
        { provider: 'deepseek-official', id: 'attach-test-vision', name: 'Attach Test Vision', vision: true },
      ],
      presets: [{ id: 'default', name: 'Default' }],
      current: { provider: 'deepseek-official', model: 'attach-test' },
    })
  } else if (msg.method === 'tui/model-info') {
    reply(msg.id, {
      reasoning: {
        efforts: [
          { id: 'off', name: 'Off' },
          { id: 'high', name: 'High' },
          { id: 'max', name: 'Max' },
        ],
        defaultEffort: 'high',
      },
      context: { contextWindow: 128000 },
      defaultMaxTokens: 32000,
    })
  } else if (msg.method === 'tui/select-model') {
    reply(msg.id, {
      ok: true,
      current: {
        provider: msg.params?.provider ?? 'deepseek-official',
        model: msg.params?.model ?? 'attach-test',
        ...(msg.params?.reasoningEffort == null ? {} : { reasoningEffort: msg.params.reasoningEffort }),
      },
    })
  } else if (msg.method === 'tui/permission') {
    reply(msg.id, { ok: true })
  } else if (msg.method === 'tui/preset') {
    reply(msg.id, { ok: true, applied: 'on-first-prompt' })
  } else if (msg.method === 'session/prompt') {
    const sid = msg.params.sessionId
    toTui.write(
      JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { messageId: 'm-attach-1' } }) + '\n',
    )
    notify('session.status', { sessionId: sid, status: 'running' })
    notify('session.event', { sessionId: sid, event: { type: 'turn/start', data: { turn: 1 } } })
    // Durable permission facts, as the permission-presets service records them.
    notify('session.event', {
      sessionId: sid,
      event: { type: 'permission/preset', data: { preset: 'workspace-write' } },
    })
    notify('session.event', {
      sessionId: sid,
      event: { type: 'sandbox/mode', data: { mode: 'workspace-write' } },
    })
    notify('session.event', {
      sessionId: sid,
      event: { type: 'approval/policy', data: { policy: 'ask' } },
    })
    for (const piece of ['pong ', 'over ', 'fds 3/4 ✓']) {
      notify('session.event', {
        sessionId: sid,
        event: {
          type: 'assistant/chunk',
          data: { turn: 1, step: 0, chunk: { type: 'text-delta', index: 0, text: piece } },
        },
      })
    }
    notify('session.event', {
      sessionId: sid,
      event: {
        type: 'assistant/message',
        data: {
          turn: 1,
          step: 0,
          message: {
            id: 'm-a',
            role: 'assistant',
            content: [{ type: 'text', text: 'pong over fds 3/4 ✓' }],
            source: { kind: 'model', provider: 'deepseek-official', model: 'attach-test' },
          },
        },
      },
    })
    notify('session.event', {
      sessionId: sid,
      event: { type: 'turn/end', data: { turn: 1, reason: { kind: 'completed' } } },
    })
    notify('session.status', { sessionId: sid, status: 'idle' })
    // Give the TUI a beat to render, then end it like a user quitting.
    setTimeout(() => child.kill('SIGTERM'), 700)
  }
})

void S

const deadline = setTimeout(() => {
  console.error('attach test: timeout — saw only:', seen.join(', ') || '(nothing)')
  child.kill('SIGKILL')
  process.exit(1)
}, 15000)

child.on('exit', () => {
  clearTimeout(deadline)
  const ok = seen.includes('initialize') && seen.includes('session/prompt')
  if (ok) {
    console.log(`ATTACH-TEST-OK methods=[${seen.join(', ')}]`)
    process.exit(0)
  }
  console.error(`ATTACH-TEST-FAIL methods=[${seen.join(', ')}]`)
  process.exit(1)
})

import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { apply, resolveAgent } from '../npm/lib/acp-client.js'
import { resolveStackedAgent } from '../npm/lib/agent.js'
import { parseClientArgv, painterArgs } from '../npm/lib/boot.js'

test('resolveAgent defaults to dsh-acp and honors command/args', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  try {
    assert.deepEqual(resolveAgent(), { command: 'dsh-acp', args: [] })
    assert.deepEqual(resolveAgent({ agent: { command: 'dsh', args: ['--profile', 'acp'] } }), {
      command: 'dsh',
      args: ['--profile', 'acp'],
    })
    process.env.DSH_TUI_AGENT = 'codex --acp'
    assert.deepEqual(resolveAgent(), { command: 'codex', args: ['--acp'] })
  } finally {
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('standalone resolution uses the active harness from Martty settings', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-active-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'local',
        label: 'Local ACP',
        command: '/opt/local/bin/local-acp',
        args: ['--stdio'],
      }],
      activeHarness: 'local',
    }))
    assert.deepEqual(resolveStackedAgent(import.meta.url, { settingsPath }), {
      command: '/opt/local/bin/local-acp',
      args: ['--stdio'],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('a product default Harness wins the last active Harness at startup', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-default-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  const defaultHarness = {
    id: 'product-default',
    label: 'Product Default',
    command: 'product-acp',
    args: ['--stdio'],
  }
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'last-used',
        label: 'Last Used',
        command: 'last-used-acp',
        args: [],
      }],
      activeHarness: 'last-used',
    }))

    assert.deepEqual(
      parseClientArgv([], { settingsPath, defaultHarness }).agent,
      { command: 'product-acp', args: ['--stdio'] },
    )
    assert.equal(
      JSON.parse(readFileSync(settingsPath, 'utf8')).activeHarness,
      'last-used',
      'startup resolution must not overwrite the last user selection',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('parseClientArgv strips agent flags for the painter', () => {
  const parsed = parseClientArgv([
    '--theme',
    'dark',
    '--agent',
    'dsh',
    '--agent-arg',
    '--profile',
    '--agent-arg',
    'acp',
    '-w',
    '/tmp/ws',
  ])
  assert.deepEqual(parsed.agent, { command: 'dsh', args: ['--profile', 'acp'] })
  assert.deepEqual(parsed.rustArgs, ['--theme', 'dark', '-w', '/tmp/ws'])
  assert.deepEqual(painterArgs(parsed), [
    '--theme',
    'dark',
    '-w',
    '/tmp/ws',
    '--agent',
    'dsh',
    '--agent-arg',
    '--profile',
    '--agent-arg',
    'acp',
  ])
})

test('parseClientArgv uses the selected harness when no CLI override is present', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-argv-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'other',
        label: 'Other ACP',
        command: 'other-acp',
        args: ['serve'],
      }],
      activeHarness: 'other',
    }))
    assert.deepEqual(parseClientArgv(['--theme', 'dark'], { settingsPath }), {
      agent: { command: 'other-acp', args: ['serve'] },
      rustArgs: ['--theme', 'dark'],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('standalone argv resolution reads the active harness from MARTTY_HOME by default', () => {
  const previousAgent = process.env.DSH_TUI_AGENT
  const previousHome = process.env.MARTTY_HOME
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-default-harness-home-'))
  process.env.MARTTY_HOME = root
  try {
    writeFileSync(path.join(root, 'settings.json'), JSON.stringify({
      harnesses: [{
        id: 'home',
        label: 'Home ACP',
        command: 'home-acp',
        args: [],
      }],
      activeHarness: 'home',
    }))
    assert.deepEqual(parseClientArgv([]).agent, { command: 'home-acp', args: [] })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previousAgent === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previousAgent
    if (previousHome === undefined) delete process.env.MARTTY_HOME
    else process.env.MARTTY_HOME = previousHome
  }
})

test('apply with a nonexistent agent command does not crash with an uncaught exception', async () => {
  const ctx = {}
  apply(ctx, { agent: { command: 'martty-definitely-missing-agent' } })
  assert.equal(ctx.acpClient.kind, 'spawn')
  // The child emits 'error' (ENOENT) asynchronously; without a listener Node
  // would kill the whole process before this assertion runs.
  await new Promise((resolve) => setTimeout(resolve, 200))
  assert.ok(ctx.acpClient.stdin.destroyed, 'stdin destroyed after failed spawn')
  assert.ok(ctx.acpClient.stdout.destroyed, 'stdout destroyed after failed spawn')
})

test('standalone ACP client replaces its child without replacing the transport', async () => {
  const ctx = {}
  apply(ctx, {
    agent: {
      command: process.execPath,
      args: ['-e', 'setInterval(() => {}, 10000)', 'first'],
    },
  })
  const service = ctx.acpClient
  const stableInput = service.stdin
  const stableOutput = service.stdout
  const first = service.child
  const switches = []
  try {
    service.onSwitch((next, previous) => switches.push({ next, previous }))
    await service.switchAgent({
      command: process.execPath,
      args: ['-e', 'setInterval(() => {}, 10000)', 'second'],
    })

    assert.equal(service.stdin, stableInput)
    assert.equal(service.stdout, stableOutput)
    assert.notEqual(service.child, first)
    assert.equal(switches.length, 1)
    assert.equal(switches[0].previous.child, first)
    assert.equal(switches[0].next.child, service.child)
    if (first.exitCode === null) await once(first, 'exit')
    assert.notEqual(first.signalCode, null)
  } finally {
    service.close?.()
    if (service.child?.exitCode === null) service.child.kill('SIGTERM')
    if (first.exitCode === null) first.kill('SIGTERM')
  }
})

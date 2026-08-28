import assert from 'node:assert/strict'
import test from 'node:test'
import { apply, resolveAgent } from '../npm/lib/acp-client.js'
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

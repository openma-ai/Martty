import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const repoRoot = path.resolve(import.meta.dirname, '..')
const npmRequire = createRequire(path.join(repoRoot, 'npm/package.json'))
const cordisUrl = pathToFileURL(npmRequire.resolve('@deepseek-ai/cordis')).href
const compatUrl = pathToFileURL(path.join(repoRoot, 'npm/lib/dsh-compat.js')).href

const compatPlugin = await import(compatUrl)

test('dsh-compat ships in the bundle patch and package exports', () => {
  const patch = readFileSync(path.join(repoRoot, 'npm/cordis.patch.yml'), 'utf8')
  assert.match(patch, /tui-permission-compat/)
  assert.match(patch, /@openma\/deepseek-harness-tui\/dsh-compat/)
  const pkg = JSON.parse(readFileSync(path.join(repoRoot, 'npm/package.json'), 'utf8'))
  assert.equal(pkg.exports['./dsh-compat'], './lib/dsh-compat.js')
})

test('dsh-compat defaults an omitted permission/preset origin to "selection"', async () => {
  const { Context } = await import(cordisUrl)
  const ctx = new Context()
  const calls = []
  const service = {
    apply(session, preset, setApproval, origin) {
      calls.push({ session, preset, setApproval, origin })
    },
  }
  ctx.provide('permissionPresets', service)
  ctx.provide('commands', { execute() {} })
  await ctx.plugin(compatPlugin)

  const session = { id: 's1' }
  const setApproval = () => {}
  // The bundled ACP adapter calls apply without an origin — the compat layer
  // must supply one so the appended event data is losslessly serializable.
  ctx.permissionPresets.apply(session, 'danger-full-access', setApproval)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].session, session)
  assert.equal(calls[0].preset, 'danger-full-access')
  assert.equal(calls[0].setApproval, setApproval)
  assert.equal(calls[0].origin, 'selection')

  // Explicit origins pass through untouched.
  ctx.permissionPresets.apply(session, 'workspace-write', setApproval, 'default')
  assert.equal(calls[1].origin, 'default')
})

test('dsh-compat inserts the images batch for legacy three-argument execute calls', async () => {
  const { Context } = await import(cordisUrl)
  const ctx = new Context()
  const calls = []
  const signal = { aborted: false }
  // rc.8+ signature: execute(agent, line, images, signal)
  const commands = {
    execute(agent, line, images, signalArg) {
      calls.push({ agent, line, images, signalArg })
    },
  }
  ctx.provide('permissionPresets', { apply() {} })
  ctx.provide('commands', commands)
  await ctx.plugin(compatPlugin)

  // The adapter's legacy three-argument call shape: signal in the images slot.
  ctx.commands.execute('agent-1', '/plan', signal)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].agent, 'agent-1')
  assert.equal(calls[0].line, '/plan')
  assert.deepEqual(calls[0].images, [])
  assert.equal(calls[0].signalArg, signal)

  // Four-argument calls pass through with the caller's images.
  ctx.commands.execute('agent-1', '/plan', ['data:image/png;base64,x'], { aborted: false })
  assert.equal(calls[1].images.length, 1)
  assert.equal(calls[1].signalArg.aborted, false)

  // An omitted images slot defaults to the empty batch.
  ctx.commands.execute('agent-1', '/compact', undefined, { aborted: false })
  assert.deepEqual(calls[2].images, [])
})

test('dsh-compat passes through the legacy three-parameter execute signature', async () => {
  const { Context } = await import(cordisUrl)
  const ctx = new Context()
  const calls = []
  // rc.7 signature: execute(agent, line, signal)
  const commands = {
    execute(agent, line, signal) {
      calls.push({ agent, line, signal })
    },
  }
  ctx.provide('permissionPresets', { apply() {} })
  ctx.provide('commands', commands)
  await ctx.plugin(compatPlugin)

  const signal = { aborted: false }
  ctx.commands.execute('agent-1', '/compact', signal)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].agent, 'agent-1')
  assert.equal(calls[0].line, '/compact')
  assert.equal(calls[0].signal, signal)
})

test('dsh-compat tolerates apply-less and execute-less services', async () => {
  const { Context } = await import(cordisUrl)
  const ctx = new Context()
  ctx.provide('permissionPresets', {})
  ctx.provide('commands', {})
  await ctx.plugin(compatPlugin)
  assert.equal(ctx.permissionPresets.apply, undefined)
  assert.equal(ctx.commands.execute, undefined)
})

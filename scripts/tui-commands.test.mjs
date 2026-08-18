import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const commandsUrl = pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-commands.js'),
).href
const commandsPlugin = await import(commandsUrl).catch(() => ({}))

function makeCtx() {
  return {
    effect(fn) {
      return fn()
    },
    get() {},
    on() {
      return () => {}
    },
  }
}

test('commands publish a complete catalog, invoke locally, and retract with lifecycle', async () => {
  assert.equal(typeof commandsPlugin.installTuiCommands, 'function')
  if (typeof commandsPlugin.installTuiCommands !== 'function') return

  const sent = []
  const invoked = []
  const commands = commandsPlugin.installTuiCommands(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  const dispose = commands.register({
    name: 'liang-effort',
    description: 'slide the reasoning effort',
  }, async (args) => {
    invoked.push(args)
  })

  assert.deepEqual(sent.at(-1), {
    method: '_dsh/cordis/tui/commands/update',
    params: {
      protocol: 0,
      commands: [{
        name: 'liang-effort',
        description: 'slide the reasoning effort',
      }],
    },
  })

  await commands.dispatch({
    protocol: 0,
    name: 'liang-effort',
    args: '--preview',
  })
  assert.deepEqual(invoked, ['--preview'])

  dispose()
  assert.deepEqual(sent.at(-1).params.commands, [])
  await assert.rejects(
    commands.dispatch({ protocol: 0, name: 'liang-effort', args: '' }),
    /not registered/i,
  )
})

test('command options stay orthogonal and command names are generic slash identifiers', () => {
  assert.equal(typeof commandsPlugin.installTuiCommands, 'function')
  if (typeof commandsPlugin.installTuiCommands !== 'function') return

  const sent = []
  const commands = commandsPlugin.installTuiCommands(makeCtx(), {
    notify(method, params) {
      sent.push({ method, params })
    },
  })
  commands.register({ name: 'threshold', description: 'adjust threshold' }, () => {})
  assert.deepEqual(sent.at(-1).params.commands, [{
    name: 'threshold',
    description: 'adjust threshold',
  }])
  assert.throws(
    () => commands.register({ name: '/bad name', description: 'bad' }, () => {}),
    /name/i,
  )
  assert.throws(
    () => commands.register({
      name: 'themed',
      description: 'must be composed by the plugin',
      when: { theme: 'charcoal' },
    }, () => {}),
    /unknown.*when/i,
  )
})

test('late transport binding publishes one authoritative current catalog', () => {
  assert.equal(typeof commandsPlugin.installTuiCommands, 'function')
  if (typeof commandsPlugin.installTuiCommands !== 'function') return

  const commands = commandsPlugin.installTuiCommands(makeCtx())
  commands.register({ name: 'plan-view', description: 'Open the current ACP plan' }, () => {})
  const disposeTemporary = commands.register({ name: 'temporary', description: 'Temporary' }, () => {})
  disposeTemporary()

  const sent = []
  commands.bindNotify((method, params) => sent.push({ method, params }))

  assert.deepEqual(sent, [{
    method: '_dsh/cordis/tui/commands/update',
    params: {
      protocol: 0,
      commands: [{ name: 'plan-view', description: 'Open the current ACP plan' }],
    },
  }])
})

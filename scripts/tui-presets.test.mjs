import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'
import { installTuiOverlay } from '../npm/lib/tui-overlay.js'

const presetsPlugin = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-presets.js'),
).href).catch(() => ({}))

function commandService() {
  let command
  return {
    register(options, handler) {
      command = { options, handler }
      const dispose = () => { command = undefined }
      dispose.update = (patch) => {
        command.options = { ...command.options, ...patch }
      }
      return dispose
    },
    async run(args) {
      return command.handler(args)
    },
    current() {
      return command
    },
  }
}

test('UI presets compose plugin mounts, switch atomically, and persist selection', async () => {
  assert.equal(typeof presetsPlugin.installTuiPresets, 'function')
  if (typeof presetsPlugin.installTuiPresets !== 'function') return

  const root = mkdtempSync(path.join(tmpdir(), 'martty-ui-presets-'))
  const settingsPath = path.join(root, 'dsh-tui-settings.json')
  writeFileSync(settingsPath, JSON.stringify({ language: 'zh', uiPreset: 'default' }))
  const commands = commandService()
  const service = presetsPlugin.installTuiPresets({ tuiCommands: commands }, { settingsPath })
  const mounted = []
  const register = (id) => service.register(
    { id, label: id === 'default' ? 'Martty' : 'DeepSeek' },
    () => {
      mounted.push(id)
      return () => mounted.splice(mounted.lastIndexOf(id), 1)
    },
  )

  const stopDefault = register('default')
  const stopDeepseek = register('deepseek')
  assert.deepEqual(service.list(), [
    { id: 'default', label: 'Martty' },
    { id: 'deepseek', label: 'DeepSeek' },
  ])
  assert.equal(service.active(), 'default')
  assert.deepEqual(mounted, ['default'])
  assert.deepEqual(commands.current().options, {
    name: 'ui',
    description: 'Switch UI Plugin',
    input: {
      hint: 'plugin',
      options: [
        { value: 'default', label: 'Martty', description: 'static · active' },
        { value: 'deepseek', label: 'DeepSeek', description: 'static · ready' },
      ],
    },
  })

  await commands.run('deepseek')
  assert.equal(service.active(), 'deepseek')
  assert.deepEqual(mounted, ['deepseek'])
  assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')), {
    language: 'zh',
    uiPreset: 'deepseek',
  })

  stopDeepseek()
  assert.equal(service.active(), 'default', 'unloading the active preset falls back safely')
  assert.deepEqual(mounted, ['default'])
  stopDefault()
  service.dispose()

  const restoredCommands = commandService()
  const restored = presetsPlugin.installTuiPresets(
    { tuiCommands: restoredCommands },
    { settingsPath },
  )
  restored.register({ id: 'default', label: 'Martty' }, () => () => {})
  restored.register({ id: 'deepseek', label: 'DeepSeek' }, () => () => {})
  assert.equal(restored.active(), 'deepseek', 'registered preferred preset restores on startup')
  restored.dispose()
  rmSync(root, { recursive: true, force: true })
})

test('/ui without arguments opens a native preset selector', async () => {
  assert.equal(typeof presetsPlugin.installTuiPresets, 'function')
  if (typeof presetsPlugin.installTuiPresets !== 'function') return

  const notifications = []
  const commands = commandService()
  const overlay = installTuiOverlay({}, {
    notify(method, params) {
      notifications.push({ method, params })
    },
  })
  const service = presetsPlugin.installTuiPresets({
    tuiCommands: commands,
    tuiOverlay: overlay,
  })
  service.register({ id: 'default', label: 'Martty' }, () => () => {})
  service.register({ id: 'deepseek', label: 'DeepSeek' }, () => () => {})

  await commands.run('')

  assert.deepEqual(overlay.active(), {
    kind: 'select',
    id: 'ui-preset',
    title: 'UI Plugin',
    value: 'default',
    options: [
      { value: 'default', label: 'Martty', description: 'static · active' },
      { value: 'deepseek', label: 'DeepSeek', description: 'static · ready' },
    ],
  })

  await overlay.dispatch({ protocol: 0, id: 'ui-preset', event: 'submit', value: 'deepseek' })
  assert.equal(service.active(), 'deepseek')
  assert.equal(notifications.at(-1).params.overlay, null)
  service.dispose()
})

test('UI Plugin catalog retains stopped dynamic owners and routes restore through its lifecycle seam', async () => {
  const commands = commandService()
  const service = presetsPlugin.installTuiPresets({ tuiCommands: commands })
  service.register({ id: 'default', label: 'Martty' }, () => () => {})
  const stopDynamic = service.registerOwned(
    'panel-1',
    { id: 'focused', label: 'Focused' },
    () => () => {},
    { source: 'dynamic' },
  )
  stopDynamic()

  assert.deepEqual(service.catalog(), [
    { id: 'default', label: 'Martty', source: 'static', status: 'active' },
    {
      id: 'focused',
      label: 'Focused',
      source: 'dynamic',
      status: 'stopped',
      owner: { pluginId: 'panel-1' },
    },
  ])
  assert.equal(
    commands.current().options.input.options[1].description,
    'dynamic · stopped',
  )

  const restored = []
  service.bindLifecycle(async (id, owner) => restored.push({ id, owner }))
  await commands.run('focused')
  assert.deepEqual(restored, [{ id: 'focused', owner: 'panel-1' }])
  service.dispose()
})

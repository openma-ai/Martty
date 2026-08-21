import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const presetsPlugin = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/tui-presets.js'),
).href).catch(() => ({}))

function commandService() {
  let command
  return {
    register(options, handler) {
      command = { options, handler }
      return () => { command = undefined }
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
    description: 'Switch UI preset',
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

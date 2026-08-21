import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const deepseek = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/deepseek-logo.js'),
).href).catch(() => ({}))
const martty = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/martty-preset.js'),
).href).catch(() => ({}))

function capturePreset(plugin) {
  let registered
  const slots = []
  const stop = plugin.apply({
    tuiPresets: {
      register(options, mount) {
        registered = { options, mount }
        return () => { registered = undefined }
      },
    },
    tuiSlots: {
      register(options, nodes) {
        const contribution = { options, nodes }
        slots.push(contribution)
        return { dispose() { slots.splice(slots.indexOf(contribution), 1) } }
      },
    },
  })
  return { registered, slots, stop }
}

test('Martty and DeepSeek UI presets fill the Hero and information regions', () => {
  assert.deepEqual(martty.inject, ['tuiPresets', 'tuiSlots'])
  assert.deepEqual(deepseek.inject, ['tuiPresets', 'tuiSlots'])

  const builtin = capturePreset(martty)
  assert.deepEqual(builtin.registered.options, { id: 'default', label: 'Martty' })
  const unmountMartty = builtin.registered.mount()
  assert.deepEqual(builtin.slots, [
    {
      options: { name: 'welcome.hero', id: 'martty-hero' },
      nodes: [
        { id: 'logo', kind: 'logo', name: 'martty' },
        { id: 'hint', kind: 'text', text: 'https://martty.sh', tone: 'fg_tertiary' },
      ],
    },
    {
      options: { name: 'welcome.info', id: 'martty-info' },
      nodes: [{ id: 'info', kind: 'welcomeinfo' }],
    },
  ])
  unmountMartty()
  assert.deepEqual(builtin.slots, [])

  const classic = capturePreset(deepseek)
  assert.deepEqual(classic.registered.options, { id: 'deepseek', label: 'DeepSeek' })
  const unmountDeepseek = classic.registered.mount()
  assert.deepEqual(classic.slots, [
    {
      options: { name: 'welcome.hero', id: 'deepseek-hero' },
      nodes: [
        { id: 'logo', kind: 'logo', name: 'deepseek' },
        { id: 'hint', kind: 'text', text: 'Into the Unknown', tone: 'fg_tertiary' },
      ],
    },
    {
      options: { name: 'welcome.info', id: 'deepseek-info' },
      nodes: [{ id: 'info', kind: 'welcomeinfo' }],
    },
  ])
  unmountDeepseek()
  classic.stop()
  builtin.stop()
})

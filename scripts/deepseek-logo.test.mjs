import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const deepseekLogo = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/deepseek-logo.js'),
).href).catch(() => ({}))

test('the DeepSeek logo Client Plugin replaces the welcome hero in place', () => {
  assert.deepEqual(deepseekLogo.inject, ['tuiCommands', 'tuiSlots'])
  assert.equal(typeof deepseekLogo.apply, 'function')
  if (typeof deepseekLogo.apply !== 'function') return

  let command
  let hero
  const stop = deepseekLogo.apply({
    tuiCommands: {
      register(options, handler) {
        command = { options, handler }
        return () => { command = undefined }
      },
    },
    tuiSlots: {
      register(options, nodes) {
        hero = { options, nodes }
        return {
          dispose() {
            hero = undefined
          },
        }
      },
    },
  })

  assert.deepEqual(command.options, {
    name: 'deepseeklogo',
    description: 'Replace the welcome hero with classic DeepSeek Harness',
  })
  command.handler()

  assert.deepEqual(hero.options, { name: 'welcome.hero', id: 'deepseek-logo' })
  assert.equal(hero.nodes.length, 2)
  assert.deepEqual(hero.nodes.map((node) => node.kind), ['ascii', 'ascii'])
  assert.equal(hero.nodes[0].tone, 'brand')
  assert.ok(hero.nodes[0].lines.some((line) => line.includes('▄█████████████████▄▄')))
  assert.ok(hero.nodes[1].lines.some((line) => line.includes('___  ___ ___ ___')))
  assert.ok(hero.nodes[1].lines.includes('H A R N E S S'))
  assert.ok(hero.nodes[1].lines.includes('Into the Unknown'))

  stop()
  assert.equal(command, undefined)
  assert.equal(hero, undefined)
})

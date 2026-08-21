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
  command.handler('martty')
  assert.equal(hero, undefined, 'martty can clear a selection restored by the painter')
  command.handler()

  assert.deepEqual(hero.options, { name: 'welcome.hero', id: 'deepseek-logo' })
  assert.deepEqual(hero.nodes, [
    { id: 'logo', kind: 'logo', name: 'deepseek' },
    {
      id: 'hint',
      kind: 'text',
      text: 'Into the Unknown',
      tone: 'fg_tertiary',
    },
  ])

  command.handler('martty')
  assert.equal(hero, undefined, 'martty restores the builtin UI preset')

  command.handler('deepseek')
  assert.equal(hero.options.id, 'deepseek-logo', 'deepseek can be selected again')

  stop()
  assert.equal(command, undefined)
  assert.equal(hero, undefined)
})

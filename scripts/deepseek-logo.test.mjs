import assert from 'node:assert/strict'
import path from 'node:path'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const deepseekLogo = await import(pathToFileURL(
  path.join(import.meta.dirname, '../npm/lib/deepseek-logo.js'),
).href).catch(() => ({}))

test('the DeepSeek logo Client Plugin registers /deepseeklogo and opens the classic whale', () => {
  assert.deepEqual(deepseekLogo.inject, ['tuiCommands', 'tuiOverlay'])
  assert.equal(typeof deepseekLogo.apply, 'function')
  if (typeof deepseekLogo.apply !== 'function') return

  let command
  let overlay
  const stop = deepseekLogo.apply({
    tuiCommands: {
      register(options, handler) {
        command = { options, handler }
        return () => { command = undefined }
      },
    },
    tuiOverlay: {
      openView(options) {
        overlay = options
        return { close() {} }
      },
    },
  })

  assert.deepEqual(command.options, {
    name: 'deepseeklogo',
    description: 'Open the classic DeepSeek Harness whale',
  })
  command.handler()

  assert.equal(overlay.id, 'deepseek-logo')
  assert.equal(overlay.title, 'DeepSeek Harness')
  assert.equal(overlay.nodes.length, 1)
  assert.equal(overlay.nodes[0].kind, 'markdown')
  assert.match(overlay.nodes[0].text, /DeepSeek Harness/)
  assert.match(overlay.nodes[0].text, /▄█████████████████▄▄/)
  assert.match(overlay.nodes[0].text, /___  ___ ___ ___  ___ ___ ___ _  __/)
  assert.match(overlay.nodes[0].text, /H A R N E S S/)

  stop()
  assert.equal(command, undefined)
})

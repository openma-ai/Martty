/**
 * `--demo-skin` entry: mount gallery pack `ember` through `tuiTheme.register`,
 * spawn the native TUI in `--demo` attach mode, and flush the Cordis TUI
 * theme update with
 * `activate: true`. Runnable as `node npm/lib/demo-skin.js`.
 */

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import * as emberPlugin from './ember.js'
import { JsonRpcLineTransport } from './jsonrpc-line-transport.js'
import { nativeBinary, spawnPluginTui } from './spawn-tui.js'
import * as themePlugin from './tui-theme.js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const { version } = JSON.parse(readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'))

function extraBinaryArgs(argv) {
  const extra = ['--demo']
  for (const arg of argv) {
    if (arg === '--demo-skin' || arg === '--demo') continue
    extra.push(arg)
  }
  return extra
}

/**
 * Register ember, activate it, spawn the TUI, and serve the attach handshake.
 * @param {string[]} [argv]
 */
export async function runDemoSkin(argv = process.argv.slice(2)) {
  const { Context } = await import('@deepseek-ai/cordis')
  const ctx = new Context()
  await ctx.plugin(themePlugin)
  await ctx.plugin(emberPlugin)
  const theme = ctx.tuiTheme
  ctx.tuiTheme.activate('ember')

  const bin = process.env.DSH_TUI_BIN || nativeBinary()
  const connection = await spawnPluginTui(bin, extraBinaryArgs(argv))
  const { child } = connection
  const transport = new JsonRpcLineTransport(connection.input, connection.output)

  function notify(method, params) {
    transport.notify(method, params)
  }
  theme.bindNotify(notify)

  transport.onRequest(async (method) => {
    switch (method) {
      case 'initialize':
        return { serverInfo: { name: 'dsh-tui-demo-skin', version } }
      case 'session/prompt':
        return { messageId: 'demo-skin' }
      case 'shutdown':
        return {}
      default:
        throw new Error(`unknown method: ${method}`)
    }
  })

  transport.start()
  connection.resume?.()

  child.on('exit', (code) => {
    process.exit(code === null ? 0 : code)
  })
  child.on('error', (err) => {
    console.error(`dsh-tui demo-skin failed: ${err.message}`)
    process.exit(1)
  })
}

const isMain = process.argv[1] !== undefined
  && path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
if (isMain) {
  await runDemoSkin(process.argv.slice(2))
}

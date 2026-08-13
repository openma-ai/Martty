'use strict'
/**
 * dsh plugin runner for the native dsh-tui terminal UI.
 *
 * Install into a profile and launch:
 *
 *     dsh plugin --profile tui add @deepseek-ai-harness/tui
 *     dsh --profile tui
 *
 * The runner spawns the platform-native `dsh-tui` binary with the host TTY
 * (fds 0/1/2 inherited) and mounts the official
 * `@deepseek-ai/dsh-sdk-jsonrpc-server` plugin with its transport redirected
 * onto two extra pipes (child fd 3: server→tui frames, child fd 4:
 * tui→server frames). The TUI therefore speaks the exact SDK JSON-RPC
 * protocol it uses standalone, while the agent, tools, persistence, and
 * providers come from the surrounding dsh profile.
 *
 * Note: stdout of the host process is the TUI's screen — keep stdout loggers
 * out of the profile (the sdk server has the same requirement).
 */

const { spawn } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')

const name = 'dsh-tui-runner'

function nativeBinary() {
  const key = `${process.platform}-${process.arch}`
  const exe = process.platform === 'win32' ? 'dsh-tui.exe' : 'dsh-tui'
  const bin = path.join(__dirname, '..', 'vendor', key, exe)
  if (!fs.existsSync(bin)) {
    let have = []
    try {
      have = fs.readdirSync(path.join(__dirname, '..', 'vendor'))
    } catch {}
    throw new Error(
      `@deepseek-ai-harness/tui: no native binary for ${key}` +
        ` (packaged: ${have.join(', ') || 'none'});` +
        ' rebuild the package on this machine with scripts/build-npm.sh',
    )
  }
  return bin
}

function apply(ctx) {
  const sdkServer = require('@deepseek-ai/dsh-sdk-jsonrpc-server')
  const bin = nativeBinary()

  const child = spawn(bin, ['--attach-fds'], {
    stdio: ['inherit', 'inherit', 'inherit', 'pipe', 'pipe'],
  })

  // child fd 3 reads what we write; child fd 4 writes what we read.
  const output = child.stdio[3]
  const input = child.stdio[4]
  ctx.plugin(sdkServer, { input, output })

  let closing = false
  child.on('exit', (code) => {
    if (closing) return
    closing = true
    // The TUI owns the product lifetime in this profile: tear the harness
    // down like the sdk server's own shutdown path, then exit.
    Promise.resolve()
      .then(() => ctx.root.fiber.dispose())
      .catch(() => {})
      .finally(() => process.exit(code === null ? 0 : code))
  })
  child.on('error', (err) => {
    if (closing) return
    closing = true
    console.error(`dsh-tui runner failed: ${err.message}`)
    process.exit(1)
  })

  ctx.effect(() => () => {
    closing = true
    try {
      child.kill('SIGTERM')
    } catch {}
  }, 'dsh-tui.native-child')
}

module.exports = { name, apply }

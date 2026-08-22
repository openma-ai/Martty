/**
 * Spawn the platform-native `martty` binary for plugin / demo-skin attach.
 *
 * Unix extra fds: 3 = Node→TUI (Rust reads), 4 = TUI→Node (Rust writes).
 * Windows (and DSH_TUI_FORCE_TCP=1): authenticated loopback TCP.
 */

import { spawn } from 'node:child_process'
import { randomBytes, timingSafeEqual } from 'node:crypto'
import fs from 'node:fs'
import { createServer } from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

/** Select the first existing native painter in development-to-release order. */
export function selectNativeBinary({ envBin, devBin, packagedBin }) {
  for (const candidate of [envBin, devBin, packagedBin]) {
    if (typeof candidate === 'string' && candidate.length > 0 && fs.existsSync(candidate)) {
      return candidate
    }
  }
  return undefined
}

/**
 * Packaged native binary for this platform, or throw if it was not staged.
 * Honors `MARTTY_BIN`, with `DSH_TUI_BIN` as a compatibility fallback.
 * @returns {string}
 */
export function nativeBinary() {
  const envBin = process.env.MARTTY_BIN || process.env.DSH_TUI_BIN
  const key = `${process.platform}-${process.arch}`
  const exe = process.platform === 'win32' ? 'martty.exe' : 'martty'
  const bin = path.join(__dirname, '..', 'vendor', key, exe)
  const devBin = path.join(__dirname, '..', '..', 'target', 'debug', exe)
  const selected = selectNativeBinary({ envBin, devBin, packagedBin: bin })
  if (selected === undefined) {
    let have = []
    try {
      have = fs.readdirSync(path.join(__dirname, '..', 'vendor'))
    } catch {
      // vendor dir missing entirely
    }
    throw new Error(
      `@openma/deepseek-harness-tui: no native binary for ${key}` +
        ` (packaged: ${have.join(', ') || 'none'});` +
      ' rebuild the package on this machine with scripts/build-npm.sh',
    )
  }
  return selected
}

/**
 * Spawn the TUI in attach mode. Extra args are appended after `--attach-fds`
 * or `--attach-tcp <addr>`. `--demo-skin` is dropped so Node cannot re-exec
 * itself in a loop.
 * @param {string} bin
 * @param {string[]} [extraArgs]
 * @param {{ stdin?: number | 'inherit', stdout?: number | 'inherit' }} [tty]
 * @returns {Promise<{ child: import('node:child_process').ChildProcess, input: import('node:stream').Writable, output: import('node:stream').Readable, resume?: () => void }>}
 */
export async function spawnPluginTui(bin, extraArgs = [], tty = {}) {
  const extra = extraArgs.filter((arg) => arg !== '--demo-skin')
  const ttyIn = tty.stdin ?? 'inherit'
  const ttyOut = tty.stdout ?? 'inherit'
  const useTcp = process.platform === 'win32' || process.env.DSH_TUI_FORCE_TCP === '1'
  if (!useTcp) {
    const child = spawn(bin, ['--attach-fds', ...extra], {
      stdio: [ttyIn, ttyOut, 'inherit', 'pipe', 'pipe'],
    })

    // Extra-fd writes race the TUI exiting (window closed, or the binary died
    // before a JSON-RPC response flushed). Node treats EPIPE on a Socket with
    // no 'error' listener as an unhandled exception; the child's 'exit' handler
    // is the lifetime signal, so the stream error only needs to be non-fatal.
    // Same posture as @deepseek-ai/dsh-sdk-client toward the runtime stdin.
    child.stdio[3].on('error', () => {})
    child.stdio[4].on('error', () => {})
    return { child, input: child.stdio[4], output: child.stdio[3] }
  }

  const token = randomBytes(32).toString('hex')
  const server = createServer()
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  if (address === null || typeof address === 'string') {
    server.close()
    throw new Error('martty runner failed to allocate a loopback TCP port')
  }

  const authenticated = waitForAuthenticatedSocket(server, token)
  const child = spawn(bin, ['--attach-tcp', `127.0.0.1:${address.port}`, ...extra], {
    stdio: [ttyIn, ttyOut, 'inherit'],
    env: { ...process.env, DSH_TUI_ATTACH_TOKEN: token },
  })
  let onChildError
  let onChildExit
  const childFailed = new Promise((_, reject) => {
    onChildError = reject
    onChildExit = (code, signal) => {
      reject(new Error(`martty exited before connecting (code=${code}, signal=${signal})`))
    }
    child.once('error', onChildError)
    child.once('exit', onChildExit)
  })
  try {
    const socket = await Promise.race([authenticated, childFailed])
    child.off('error', onChildError)
    child.off('exit', onChildExit)
    return { child, input: socket, output: socket, resume: () => socket.resume() }
  } catch (error) {
    server.close()
    try {
      child.kill('SIGTERM')
    } catch {
      // child may already be gone
    }
    throw error
  }
}

function waitForAuthenticatedSocket(server, token) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cleanup()
      server.close()
      reject(new Error('martty timed out connecting to the loopback transport'))
    }, 10_000)
    timeout.unref?.()

    const cleanup = () => {
      clearTimeout(timeout)
      server.off('error', fail)
      server.off('connection', authenticate)
    }
    const fail = (error) => {
      cleanup()
      reject(error)
    }
    const authenticate = (socket) => {
      let buffered = Buffer.alloc(0)
      const onData = (chunk) => {
        buffered = Buffer.concat([buffered, chunk])
        if (buffered.length > 1024) {
          socket.destroy()
          return
        }
        const newline = buffered.indexOf(0x0a)
        if (newline < 0) return
        socket.off('data', onData)
        const supplied = buffered.subarray(0, newline).toString('utf8').trim()
        const expectedBytes = Buffer.from(token)
        const suppliedBytes = Buffer.from(supplied)
        const valid = suppliedBytes.length === expectedBytes.length
          && timingSafeEqual(suppliedBytes, expectedBytes)
        if (!valid) {
          socket.destroy()
          return
        }
        socket.pause()
        const remainder = buffered.subarray(newline + 1)
        if (remainder.length > 0) socket.unshift(remainder)
        cleanup()
        server.close()
        resolve(socket)
      }
      socket.on('data', onData)
      socket.on('error', () => {})
    }

    server.on('error', fail)
    server.on('connection', authenticate)
  })
}

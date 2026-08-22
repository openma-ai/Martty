/**
 * Host-side TUI launcher.
 *
 * The dsh profile keeps the Base and ACP server on its Host Cordis tree. This
 * plugin starts a separate Node process for the TUI Client Cordis tree and
 * connects the two processes through the Client process's standard ACP
 * stdin/stdout. Separate inherited descriptors carry the user's TTY.
 */

import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const name = 'dsh-tui-runner'
export const inject = ['loader', 'acpServer', 'cmdlineArgs', 'appExit', 'tuiClientPlugins']

const clientEntry = fileURLToPath(new URL('./client-process.js', import.meta.url))
const requireFromTui = createRequire(import.meta.url)

function ownAcpBridge() {
  const specifier = '@openma/deepseek-harness-acp/bridge'
  try {
    return pathToFileURL(requireFromTui.resolve(specifier)).href
  } catch {
    // A source-linked package may rely on the active profile's installation.
    return specifier
  }
}

function appExitOf(ctx) {
  return ctx.root?.get?.('appExit') ?? ctx.get?.('appExit') ?? ctx.appExit
}

/** @param {object} ctx */
export async function apply(ctx) {
  const bridge = await ctx.loader.import(ownAcpBridge())
  if (typeof bridge.nodeAcpStream !== 'function') {
    throw new Error('dsh-tui: ACP bridge does not export nodeAcpStream')
  }
  const child = spawn(
    process.execPath,
    [clientEntry, ...ctx.cmdlineArgs.get()],
    {
      // ACP owns the Client process's stdin/stdout. Its fd 3/4 retain the
      // user's terminal for the Rust painter spawned inside that process.
      stdio: ['pipe', 'pipe', 'inherit', process.stdin, process.stdout],
      env: {
        ...process.env,
        DSH_TUI_CLIENT_PLUGINS_V0: JSON.stringify(ctx.tuiClientPlugins.list()),
      },
    },
  )
  const agentToClient = child.stdin
  const clientToAgent = child.stdout
  agentToClient?.on?.('error', () => {})
  clientToAgent?.on?.('error', () => {})

  let connection
  let released = false
  let exitRequested = false
  const requestExit = (code) => {
    if (exitRequested) return
    exitRequested = true
    appExitOf(ctx)?.(code)
  }
  const release = async () => {
    if (released) return
    released = true
    process.off('exit', onProcessExit)
    if (child.exitCode === null) child.kill('SIGTERM')
    await connection?.dispose?.()
  }
  const onProcessExit = () => {
    if (child.exitCode === null) child.kill('SIGTERM')
  }
  child.once('error', (error) => {
    console.error(`dsh-tui: failed to start Client process: ${error.message}`)
    requestExit(1)
    void release()
  })
  child.once('exit', (code, signal) => {
    requestExit(code ?? (signal === 'SIGINT' ? 130 : 1))
    void release()
  })
  process.once('exit', onProcessExit)

  try {
    connection = ctx.acpServer.connect(
      bridge.nodeAcpStream(clientToAgent, agentToClient),
    )
    await connection
  } catch (error) {
    await release()
    throw error
  }
  ctx.effect(() => release)
}

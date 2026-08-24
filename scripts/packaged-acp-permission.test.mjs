import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const npmRoot = path.join(repoRoot, 'npm')
const requireFromPackage = createRequire(path.join(npmRoot, 'package.json'))
const acpManifestPath = requireFromPackage.resolve('@openma/deepseek-harness-acp/package.json')
const acpManifest = requireFromPackage(acpManifestPath)
const acpBin = path.join(path.dirname(acpManifestPath), acpManifest.bin['dsh-acp'])

function request(child, pending, id, method, params) {
  const result = new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id)
      reject(new Error(`${method} timed out`))
    }, 30_000)
    timer.unref?.()
    pending.set(id, {
      resolve(value) {
        clearTimeout(timer)
        resolve(value)
      },
      reject(error) {
        clearTimeout(timer)
        reject(error)
      },
    })
  })
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`)
  return result
}

test('the packaged ACP ignores an old installed profile and restores the Host permission default', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'martty-packaged-acp-'))
  const home = path.join(root, 'home')
  const sessions = path.join(root, 'sessions')
  const workspace = path.join(root, 'workspace')
  const staleProfile = path.join(home, 'profiles', 'acp')
  await mkdir(staleProfile, { recursive: true })
  await mkdir(workspace, { recursive: true })
  await writeFile(path.join(home, 'settings.yaml'), 'permission:\n  defaultPreset: danger-full-access\n')
  await writeFile(path.join(staleProfile, 'cordis.yml'), '[]\n')
  await writeFile(path.join(staleProfile, 'cordis.patch.yml'), '[]\n')
  await writeFile(path.join(staleProfile, 'package.json'), JSON.stringify({
    name: 'stale-acp-profile',
    private: true,
    dependencies: { '@openma/deepseek-harness-acp': '0.0.1' },
    dsh: { profile: { bundles: ['definitely-missing-stale-acp-bundle'] } },
  }))

  const env = {
    ...process.env,
    DSH_HOME: home,
    DSH_SESSION_ROOT: sessions,
    DEEPSEEK_API_KEY: 'sk-test-not-real',
    DEEPSEEK_BASE_URL: 'http://127.0.0.1:1',
  }
  delete env.DSH_PERMISSION_MODE
  const child = spawn(process.execPath, [acpBin], {
    cwd: workspace,
    env,
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const pending = new Map()
  let stdout = ''
  let stderr = ''
  child.stdout.setEncoding('utf8')
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => { stderr += chunk })
  child.stdout.on('data', (chunk) => {
    stdout += chunk
    for (;;) {
      const newline = stdout.indexOf('\n')
      if (newline < 0) break
      const line = stdout.slice(0, newline).trim()
      stdout = stdout.slice(newline + 1)
      if (line.length === 0) continue
      const message = JSON.parse(line)
      if (typeof message.id !== 'number') continue
      const waiter = pending.get(message.id)
      if (waiter === undefined) continue
      pending.delete(message.id)
      if (message.error !== undefined) waiter.reject(new Error(message.error.message))
      else waiter.resolve(message.result)
    }
  })
  child.once('exit', (code, signal) => {
    const error = new Error(`packaged ACP exited (${code ?? signal})\n${stderr}`)
    for (const waiter of pending.values()) waiter.reject(error)
    pending.clear()
  })

  try {
    await request(child, pending, 1, 'initialize', { protocolVersion: 1 })
    const created = await request(child, pending, 2, 'session/new', {
      cwd: workspace,
      mcpServers: [],
    })
    assert.equal(created.modes.currentModeId, 'danger-full-access')
  } finally {
    child.stdin.end()
    if (child.exitCode === null) child.kill('SIGTERM')
    await rm(root, { recursive: true, force: true })
  }
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { discoverHarnessCandidates } from '../npm/lib/harnesses.js'

const discovery = await import('../npm/lib/harness-discovery.js').catch(() => ({}))

test('Background discovery preserves synchronous discovery results without blocking the caller', async (t) => {
  assert.equal(typeof discovery.scanHarnessCandidates, 'function')
  const root = mkdtempSync(path.join(tmpdir(), 'martty-scan-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  const options = { pathValue: '', defaults: [], registry: [{
    id: 'missing', label: 'Missing', commands: [{ command: 'missing-acp' }],
  }], fetchRegistry() { throw new Error('discovery must not fetch') } }
  let ticked = false
  const pending = discovery.scanHarnessCandidates(settingsPath, options, 'missing')
  setImmediate(() => { ticked = true })
  const entries = await pending
  assert.equal(ticked, true, 'the event loop runs while the worker probes the filesystem')
  assert.deepEqual(entries, discoverHarnessCandidates(settingsPath, options, 'missing'))
})

test('Background discovery aborts before or during a scan', async () => {
  assert.equal(typeof discovery.scanHarnessCandidates, 'function')
  const options = { pathValue: '', defaults: [], registry: [] }
  const controller = new AbortController()
  const pending = discovery.scanHarnessCandidates(undefined, { ...options, signal: controller.signal })
  controller.abort(new Error('left the picker'))
  await assert.rejects(pending, /left the picker/)
  await assert.rejects(discovery.scanHarnessCandidates(undefined, { ...options, signal: controller.signal }), /left the picker/)
})

import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { upsertHarness, setDefaultHarness } from '../npm/lib/harnesses.js'
import { planHarnessRemoval, removeHarness } from '../npm/lib/harness-removal.js'

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-removal-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settings = path.join(root, '.martty', 'settings.json')
  const resource = path.join(root, '.martty', 'bin', 'agent', '1.0', 'darwin-arm64')
  mkdirSync(resource, { recursive: true })
  const command = path.join(resource, 'agent')
  writeFileSync(command, 'fixture')
  upsertHarness(settings, { id: 'agent', command })
  setDefaultHarness(settings, 'agent')
  return { root, settings, resource, command }
}

test('Configuration-only removal clears the default but keeps resources and unrelated settings', t => {
  const { settings, resource } = fixture(t)
  const config = JSON.parse(readFileSync(settings)); config.theme = 'ember'
  writeFileSync(settings, JSON.stringify(config))
  removeHarness(settings, planHarnessRemoval(settings, 'agent'), { cleanup: false })
  assert.ok(existsSync(resource))
  assert.deepEqual(JSON.parse(readFileSync(settings)), { harnesses: [], theme: 'ember' })
})

test('Resolved executable paths still identify the private installation through OS directory aliases', t => {
  const { settings, command, resource } = fixture(t)
  upsertHarness(settings, { id: 'agent', command: realpathSync(command) })
  assert.deepEqual(planHarnessRemoval(settings, 'agent').resources, [resource])
})

test('Cleanup deletes only the exact private installation, preserving siblings and credentials', t => {
  const { settings, resource, root } = fixture(t)
  const sibling = path.join(path.dirname(resource), 'other-platform')
  mkdirSync(sibling); writeFileSync(path.join(root, '.martty', 'auth.json'), 'secret fixture')
  const plan = planHarnessRemoval(settings, 'agent')
  assert.deepEqual(plan.resources, [resource])
  removeHarness(settings, plan, { cleanup: true })
  assert.ok(!existsSync(resource)); assert.ok(existsSync(sibling))
  assert.ok(existsSync(path.join(root, '.martty', 'auth.json')))
})

test('Shared resources, external commands and symlinks cannot be cleaned', t => {
  const { settings, resource, root, command } = fixture(t)
  upsertHarness(settings, { id: 'other', command: 'node', args: [command] })
  assert.match(planHarnessRemoval(settings, 'agent').cleanupReason, /shared/i)
  assert.throws(() => removeHarness(settings, planHarnessRemoval(settings, 'agent'), { cleanup: true }), /shared/i)
  upsertHarness(settings, { id: 'agent', command: '/usr/local/bin/agent' })
  assert.equal(planHarnessRemoval(settings, 'agent').resources.length, 0)
  upsertHarness(settings, { id: 'agent', command })
  upsertHarness(settings, { id: 'other', command: 'npx', args: ['package'] })
  symlinkSync(root, path.join(resource, 'outside'), 'dir')
  assert.match(planHarnessRemoval(settings, 'agent').cleanupReason, /symbolic link/i)
  assert.ok(existsSync(command))
})

test('Running and product-forced Harnesses cannot be removed; stale confirmations cannot delete replacements', t => {
  const { settings, command } = fixture(t)
  assert.throws(() => planHarnessRemoval(settings, 'agent', { isCurrent: () => true }), /still running/i)
  assert.throws(() => planHarnessRemoval(settings, 'agent', { forcedHarness: { id: 'agent', command } }), /forced/i)
  const plan = planHarnessRemoval(settings, 'agent')
  upsertHarness(settings, { id: 'agent', command: 'replacement' })
  assert.throws(() => removeHarness(settings, plan, { cleanup: false }), /changed/i)
  assert.ok(existsSync(command))
})

test('Symlinked installation parent is not followed and new shared references invalidate cleanup', t => {
  const { root, settings, resource, command } = fixture(t)
  const plan = planHarnessRemoval(settings, 'agent')
  upsertHarness(settings, { id: 'new-reference', command })
  assert.throws(() => removeHarness(settings, plan, { cleanup: true }), /shared/i)
  const elsewhere = path.join(root, 'elsewhere'); mkdirSync(elsewhere)
  symlinkSync(elsewhere, path.join(path.dirname(resource), 'linked'), 'dir')
  upsertHarness(settings, { id: 'linked', command: path.join(path.dirname(resource), 'linked', 'agent') })
  assert.match(planHarnessRemoval(settings, 'linked').cleanupReason, /symbolic link/i)
})

test('Shared command arguments using equals syntax prevent cleanup', t => {
  const { settings, command } = fixture(t)
  upsertHarness(settings, { id: 'other', command: 'runner', args: [`--agent=${command}`] })
  assert.match(planHarnessRemoval(settings, 'agent').cleanupReason, /shared/i)
})

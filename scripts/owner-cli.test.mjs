import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import test from 'node:test'

import { runsNativeOwner } from '../npm/lib/native-cli.js'

const repoRoot = path.resolve(import.meta.dirname, '..')

test('Martty owner bypasses the UI compositor and runs the native ACP owner', () => {
  assert.equal(runsNativeOwner(['owner', '--startup-command', '/telegram-connect']), true)
  assert.equal(runsNativeOwner(['--workspace', '/tmp']), false)
  assert.equal(runsNativeOwner(['--workspace', 'owner']), false)
})

test('npm exposes the Martty name without removing compatibility bins', () => {
  const pkg = JSON.parse(readFileSync(path.join(repoRoot, 'npm', 'package.json'), 'utf8'))
  assert.equal(pkg.bin.martty, 'bin/dsh-tui.js')
  assert.equal(pkg.bin['dsh-tui'], 'bin/dsh-tui.js')
  assert.equal(pkg.bin.dsb, 'bin/dsh-tui.js')
})

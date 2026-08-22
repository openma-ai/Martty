import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const script = path.join(repoRoot, 'scripts', 'package-native.mjs')

function run(args) {
  return spawnSync(process.execPath, [script, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
  })
}

test('stages native binaries under npm platform keys', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-package-'))
  const source = path.join(root, 'compiled-binary')
  const vendorRoot = path.join(root, 'vendor')
  writeFileSync(source, 'native-binary-fixture')

  const cases = [
    ['darwin', 'arm64', 'darwin-arm64/martty'],
    ['darwin', 'x64', 'darwin-x64/martty'],
    ['linux', 'x64', 'linux-x64/martty'],
    ['linux', 'arm64', 'linux-arm64/martty'],
    ['win32', 'x64', 'win32-x64/martty.exe'],
  ]

  for (const [platform, arch, expectedPath] of cases) {
    const result = run([
      'stage',
      '--source', source,
      '--platform', platform,
      '--arch', arch,
      '--vendor-root', vendorRoot,
    ])
    assert.equal(result.status, 0, result.stderr)
    assert.equal(
      readFileSync(path.join(vendorRoot, expectedPath), 'utf8'),
      'native-binary-fixture',
    )
  }
})

test('verifies that a release contains every supported platform', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-verify-'))
  const source = path.join(root, 'compiled-binary')
  const vendorRoot = path.join(root, 'vendor')
  writeFileSync(source, 'native-binary-fixture')

  for (const [platform, arch] of [
    ['darwin', 'arm64'],
    ['darwin', 'x64'],
    ['linux', 'x64'],
    ['linux', 'arm64'],
    ['win32', 'x64'],
  ]) {
    const staged = run([
      'stage',
      '--source', source,
      '--platform', platform,
      '--arch', arch,
      '--vendor-root', vendorRoot,
    ])
    assert.equal(staged.status, 0, staged.stderr)
  }

  const verified = run(['verify', '--vendor-root', vendorRoot])
  assert.equal(verified.status, 0, verified.stderr)
})

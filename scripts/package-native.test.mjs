import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
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

test('restaging replaces the executable inode instead of overwriting it in place', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-restage-'))
  const source = path.join(root, 'compiled-binary')
  const vendorRoot = path.join(root, 'vendor')
  const destination = path.join(vendorRoot, 'darwin-arm64', 'martty')
  const args = [
    'stage',
    '--source', source,
    '--platform', 'darwin',
    '--arch', 'arm64',
    '--vendor-root', vendorRoot,
  ]

  writeFileSync(source, 'first-native-binary')
  const first = run(args)
  assert.equal(first.status, 0, first.stderr)
  const firstInode = statSync(destination).ino

  writeFileSync(source, 'second-native-binary')
  const second = run(args)
  assert.equal(second.status, 0, second.stderr)
  assert.notEqual(
    statSync(destination).ino,
    firstInode,
    'macOS caches Mach-O signatures by vnode, so an executed binary must be atomically replaced',
  )
  assert.equal(readFileSync(destination, 'utf8'), 'second-native-binary')
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

test('packages each staged binary for only its matching npm platform', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-platform-packages-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'compiled-binary')
  const vendorRoot = path.join(root, 'vendor')
  const packageRoot = path.join(root, 'packages')
  for (const [platform, arch] of [
    ['darwin', 'arm64'], ['darwin', 'x64'], ['linux', 'arm64'],
    ['linux', 'x64'], ['win32', 'x64'],
  ]) {
    writeFileSync(source, `${platform}-${arch}`)
    assert.equal(run(['stage', '--source', source, '--platform', platform,
      '--arch', arch, '--vendor-root', vendorRoot]).status, 0)
  }

  const packaged = run(['package', '--vendor-root', vendorRoot,
    '--package-root', packageRoot, '--version', '1.2.3'])
  assert.equal(packaged.status, 0, packaged.stderr)
  assert.deepEqual(readdirSync(packageRoot).sort(), [
    'darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64',
  ])
  for (const [platform, arch] of [
    ['darwin', 'arm64'], ['darwin', 'x64'], ['linux', 'arm64'],
    ['linux', 'x64'], ['win32', 'x64'],
  ]) {
    const key = `${platform}-${arch}`
    const directory = path.join(packageRoot, key)
    const binaryName = platform === 'win32' ? 'martty.exe' : 'martty'
    const manifest = JSON.parse(readFileSync(path.join(directory, 'package.json'), 'utf8'))
    assert.equal(manifest.name, `@openma/martty-${key}`)
    assert.equal(manifest.version, '1.2.3')
    assert.deepEqual(manifest.os, [platform])
    assert.deepEqual(manifest.cpu, [arch])
    assert.deepEqual(readdirSync(path.join(directory, 'bin')), [binaryName])
    assert.equal(readFileSync(path.join(directory, 'bin', binaryName), 'utf8'), key)
  }
})

test('does not create partial platform packages when a binary is missing', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-platform-missing-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const packageRoot = path.join(root, 'packages')
  const result = run(['package', '--vendor-root', path.join(root, 'vendor'),
    '--package-root', packageRoot, '--version', '1.2.3'])
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /missing native binaries/)
  assert.equal(existsSync(packageRoot), false)
})

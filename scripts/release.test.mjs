import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const script = path.join(repoRoot, 'scripts', 'release.mjs')

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-release-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  mkdirSync(path.join(root, 'npm'))
  writeFileSync(path.join(root, 'npm', 'package.json'), JSON.stringify({ name: 'fixture', version: '0.1.0' }, null, 2) + '\n')
  writeFileSync(path.join(root, 'npm', 'package-lock.json'), JSON.stringify({
    name: 'fixture',
    version: '0.1.0',
    lockfileVersion: 3,
    requires: true,
    packages: { '': { name: 'fixture', version: '0.1.0' } },
  }, null, 2) + '\n')
  writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "fixture"\nversion = "0.1.0"\n')
  writeFileSync(path.join(root, 'Cargo.lock'), 'version = 4\n\n[[package]]\nname = "fixture"\nversion = "0.1.0"\n')
  git(root, ['init', '-b', 'main'])
  git(root, ['add', '.'])
  git(root, ['commit', '-m', 'initial'])
  return root
}

function git(root, args) {
  const result = spawnSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: 'test', GIT_AUTHOR_EMAIL: 'test@example.com',
      GIT_COMMITTER_NAME: 'test', GIT_COMMITTER_EMAIL: 'test@example.com',
    },
  })
  assert.equal(result.status, 0, result.stderr)
  return result.stdout.trim()
}

function run(root, version, extra = []) {
  return spawnSync(process.execPath, [script, version, ...extra], {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      DSH_TUI_RELEASE_ROOT: root,
      GIT_AUTHOR_NAME: 'test', GIT_AUTHOR_EMAIL: 'test@example.com',
      GIT_COMMITTER_NAME: 'test', GIT_COMMITTER_EMAIL: 'test@example.com',
    },
  })
}

test('bumps all package versions, commits, and tags', (t) => {
  const root = fixture(t)
  const result = run(root, '0.2.8')
  assert.equal(result.status, 0, result.stderr)
  assert.equal(JSON.parse(readFileSync(path.join(root, 'npm', 'package.json'), 'utf8')).version, '0.2.8')
  const packageLock = JSON.parse(readFileSync(path.join(root, 'npm', 'package-lock.json'), 'utf8'))
  assert.equal(packageLock.version, '0.2.8')
  assert.equal(packageLock.packages[''].version, '0.2.8')
  assert.match(readFileSync(path.join(root, 'Cargo.toml'), 'utf8'), /^version = "0\.2\.8"$/m)
  assert.match(readFileSync(path.join(root, 'Cargo.lock'), 'utf8'), /^version = "0\.2\.8"$/m)
  assert.equal(git(root, ['log', '-1', '--format=%s']), 'release: v0.2.8')
  assert.equal(git(root, ['tag', '--list']), 'v0.2.8')
  assert.equal(git(root, ['status', '--porcelain']), '')
})

test('bumps and tags a beta prerelease', (t) => {
  const root = fixture(t)
  const result = run(root, '0.2.17-beta.0')
  assert.equal(result.status, 0, result.stderr)
  assert.equal(JSON.parse(readFileSync(path.join(root, 'npm', 'package.json'), 'utf8')).version, '0.2.17-beta.0')
  assert.match(readFileSync(path.join(root, 'Cargo.toml'), 'utf8'), /^version = "0\.2\.17-beta\.0"$/m)
  assert.equal(git(root, ['log', '-1', '--format=%s']), 'release: v0.2.17-beta.0')
  assert.equal(git(root, ['tag', '--list']), 'v0.2.17-beta.0')
})

test('accepts a v-prefixed version and dry-run changes nothing', (t) => {
  const root = fixture(t)
  const result = run(root, 'v0.3.0', ['--dry-run'])
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /would bump/)
  assert.equal(git(root, ['tag', '--list']), '')
  assert.equal(git(root, ['status', '--porcelain']), '')
  assert.equal(JSON.parse(readFileSync(path.join(root, 'npm', 'package.json'), 'utf8')).version, '0.1.0')
})

test('refuses a dirty working tree', (t) => {
  const root = fixture(t)
  writeFileSync(path.join(root, 'Cargo.toml'), 'dirty\n')
  const result = run(root, '0.2.8')
  assert.equal(result.status, 1)
  assert.match(result.stderr, /working tree is not clean/)
})

test('refuses an existing tag', (t) => {
  const root = fixture(t)
  git(root, ['tag', 'v0.1.0'])
  const result = run(root, '0.1.0')
  assert.equal(result.status, 1)
  assert.match(result.stderr, /tag v0\.1\.0 already exists/)
})

test('refuses an already-bumped version', (t) => {
  const root = fixture(t)
  const result = run(root, '0.1.0')
  assert.equal(result.status, 1)
  assert.match(result.stderr, /already at 0\.1\.0/)
})

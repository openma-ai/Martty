import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const script = path.join(repoRoot, 'scripts', 'check-release-tag.mjs')

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-release-'))
  const packageJson = path.join(root, 'package.json')
  const cargoToml = path.join(root, 'Cargo.toml')
  writeFileSync(packageJson, JSON.stringify({ version: '0.1.0' }))
  writeFileSync(cargoToml, '[package]\nname = "fixture"\nversion = "0.1.0"\n')
  return { packageJson, cargoToml }
}

function run(tag, packageJson, cargoToml) {
  return spawnSync(process.execPath, [script, tag, packageJson, cargoToml], {
    cwd: repoRoot,
    encoding: 'utf8',
  })
}

test('accepts a release tag matching npm and Cargo versions', () => {
  const { packageJson, cargoToml } = fixture()
  const result = run('v0.1.0', packageJson, cargoToml)
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /release version 0\.1\.0/)
})

test('rejects a tag that does not match the npm version', () => {
  const { packageJson, cargoToml } = fixture()
  const result = run('v0.2.0', packageJson, cargoToml)
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /tag v0\.2\.0 does not match npm version 0\.1\.0/)
})

test('rejects npm and Cargo version drift', () => {
  const { packageJson, cargoToml } = fixture()
  writeFileSync(cargoToml, '[package]\nname = "fixture"\nversion = "0.2.0"\n')
  const result = run('v0.1.0', packageJson, cargoToml)
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /Cargo version 0\.2\.0 does not match npm version 0\.1\.0/)
})

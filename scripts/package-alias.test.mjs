import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  mkdirSync,
  mkdtempSync,
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const script = path.resolve(import.meta.dirname, 'package-alias.mjs')

test('creates a thin Martty package with platform optional dependencies', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  const destination = path.join(root, 'martty')
  mkdirSync(path.join(source, 'lib'), { recursive: true })
  mkdirSync(path.join(source, 'lib', 'node_modules', 'dep'), { recursive: true })
  mkdirSync(path.join(source, 'bin'), { recursive: true })
  mkdirSync(path.join(source, 'vendor', 'linux-x64'), { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version: '1.2.3',
    files: ['vendor', 'lib', 'bin'],
    bin: {
      martty: 'bin/martty.js',
      'dsh-tui': 'bin/martty.js',
    },
    scripts: {
      postinstall: 'node lib/install-migration.js',
    },
  }, null, 2) + '\n')
  writeFileSync(
    path.join(source, 'cordis.patch.yml'),
    "name: '@openma/deepseek-harness-tui/acp-host'\n",
  )
  writeFileSync(
    path.join(source, 'lib', 'meta.js'),
    "export const source = '@openma/deepseek-harness-tui/creator-overlay'\n",
  )
  writeFileSync(path.join(source, 'lib', 'untouched.js'), 'export const other = true\n')
  writeFileSync(path.join(source, 'lib', 'package-lock.json'), '{}\n')
  writeFileSync(path.join(source, 'lib', 'node_modules', 'dep', 'index.js'), 'module.exports = {}\n')
  writeFileSync(path.join(source, 'bin', 'martty.png'), Buffer.from([137, 80]))
  const binary = Buffer.from([0, 255, 1, 254])
  writeFileSync(path.join(source, 'vendor', 'linux-x64', 'martty'), binary)

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr)
  assert.deepEqual(
    JSON.parse(readFileSync(path.join(destination, 'package.json'), 'utf8')),
    {
      name: 'martty',
      version: '1.2.3',
      files: ['lib', 'bin'],
      bin: {
        'martty-cli': 'bin/martty.js',
      },
      scripts: {
        postinstall: 'node lib/install-migration.js',
      },
      optionalDependencies: {
        '@openma/martty-darwin-arm64': '1.2.3',
        '@openma/martty-darwin-x64': '1.2.3',
        '@openma/martty-linux-arm64': '1.2.3',
        '@openma/martty-linux-x64': '1.2.3',
        '@openma/martty-win32-x64': '1.2.3',
      },
    },
  )
  assert.match(readFileSync(path.join(destination, 'cordis.patch.yml'), 'utf8'), /martty\/acp-host/)
  assert.match(readFileSync(path.join(destination, 'lib', 'meta.js'), 'utf8'), /martty\/creator-overlay/)
  assert.equal(readFileSync(path.join(destination, 'lib', 'untouched.js'), 'utf8'), 'export const other = true\n')
  assert.equal(existsSync(path.join(destination, 'vendor')), false)
  assert.deepEqual(readFileSync(path.join(source, 'vendor', 'linux-x64', 'martty')), binary)
  assert.equal(
    JSON.parse(readFileSync(path.join(source, 'package.json'), 'utf8')).name,
    '@openma/deepseek-harness-tui',
  )
})

test('falls back to the canonical bin entry when the source has none', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  const destination = path.join(root, 'martty')
  mkdirSync(source, { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version: '1.2.3',
  }, null, 2) + '\n')

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr)
  assert.equal(
    JSON.parse(readFileSync(path.join(destination, 'package.json'), 'utf8')).bin['martty-cli'],
    'bin/martty.js',
  )
})

test('fails without all three arguments', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))

  for (const args of [[], [path.join(root, 'legacy')], [path.join(root, 'legacy'), path.join(root, 'martty')]]) {
    const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' })
    assert.equal(result.status, 1)
    assert.match(result.stderr, /usage: package-alias\.mjs <source> <destination> <alias-name>/)
  }
})

test('fails when the source package directory does not exist', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'missing')

  const result = spawnSync(process.execPath, [script, source, path.join(root, 'martty'), 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 1)
  assert.match(result.stderr, new RegExp(`source package directory not found: ${source}`))

  const fileSource = path.join(root, 'file-source')
  writeFileSync(fileSource, 'x')
  const fileResult = spawnSync(
    process.execPath,
    [script, fileSource, path.join(root, 'other'), 'martty'],
    { encoding: 'utf8' },
  )
  assert.equal(fileResult.status, 1)
  assert.match(fileResult.stderr, new RegExp(`source package directory not found: ${fileSource}`))
})

test('fails when the destination already exists', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  const destination = path.join(root, 'martty')
  mkdirSync(destination, { recursive: true })
  mkdirSync(source, { recursive: true })

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 1)
  assert.match(result.stderr, new RegExp(`destination already exists: ${destination}`))
  assert.match(result.stderr, /rm -rf /)
})

test('fails when the source package is not the legacy package', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  mkdirSync(source, { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: 'some-other-package',
    version: '1.2.3',
  }, null, 2) + '\n')

  const result = spawnSync(
    process.execPath,
    [script, source, path.join(root, 'martty'), 'martty'],
    { encoding: 'utf8' },
  )

  assert.equal(result.status, 1)
  assert.match(result.stderr, /expected source package @openma\/deepseek-harness-tui or martty, found some-other-package/)
})

test('accepts a source already named martty (post-rename checkout)', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'martty')
  const destination = path.join(root, 'out')
  mkdirSync(source, { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: 'martty',
    version: '1.2.3',
    bin: { 'martty-cli': 'bin/martty.js' },
  }, null, 2) + '\n')

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr)
  const out = JSON.parse(readFileSync(path.join(destination, 'package.json'), 'utf8'))
  assert.equal(out.name, 'martty')
  assert.deepEqual(out.bin, { 'martty-cli': 'bin/martty.js' })
})

test('fails when rewriting changes the alias version', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  const destination = path.join(root, 'martty')
  mkdirSync(source, { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version: '1.2.3-@openma/deepseek-harness-tui',
  }, null, 2) + '\n')

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 1)
  assert.match(result.stderr, /alias version 1\.2\.3-martty does not match source 1\.2\.3-@openma\/deepseek-harness-tui/)
})

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

// Real `npm install --global` runs against packed fixtures, covering the
// overlapping-bin scenarios from the package rename: previous
// `@openma/deepseek-harness-tui` releases declare `dsh-tui` (all versions)
// and `martty` (0.2.14/0.2.20), and an unrelated `dsh-tui` package owns the
// same command name.

const repoRoot = path.resolve(import.meta.dirname, '..')
const packageRoot = path.join(repoRoot, 'npm')
const aliasScript = path.join(repoRoot, 'scripts', 'package-alias.mjs')
const LEGACY_PACKAGE = '@openma/deepseek-harness-tui'
const STUB_BIN = '#!/usr/bin/env node\nconsole.log(process.argv[1])\n'

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    encoding: 'utf8',
    timeout: 120_000,
    ...options,
    env: { ...process.env, ...options.env },
  })
}

function npm(args, options = {}) {
  const npmCli = process.env.npm_execpath
  return npmCli === undefined
    ? run('npm', args, options)
    : run(process.execPath, [npmCli, ...args], options)
}

function requireOk(result, operation) {
  assert.equal(
    result.status,
    0,
    `${operation} failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  )
}

function pack(directory, destination) {
  const result = npm(
    ['pack', '--silent', '--ignore-scripts', '--pack-destination', destination],
    { cwd: directory },
  )
  requireOk(result, `pack ${directory}`)
  const filename = result.stdout.trim().split(/\r?\n/).at(-1)
  assert.ok(filename, `npm pack printed no filename for ${directory}`)
  return path.join(destination, filename)
}

function installGlobal(prefix, specifier, extraArgs = []) {
  return npm([
    'install', '--global', '--no-audit', '--no-fund',
    '--prefix', prefix, ...extraArgs, specifier,
  ])
}

function removeGlobal(prefix, specifier) {
  return npm(['remove', '--global', '--prefix', prefix, specifier])
}

function binPath(prefix, name) {
  return path.join(prefix, 'bin', name)
}

function writeFixture(dir, manifest, bins) {
  mkdirSync(path.join(dir, 'bin'), { recursive: true })
  writeFileSync(path.join(dir, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  for (const name of bins) {
    writeFileSync(path.join(dir, 'bin', `${name}.js`), STUB_BIN)
  }
}

function binEntry(bins) {
  return Object.fromEntries(bins.map((name) => [name, `bin/${name}.js`]))
}

// The martty fixture is the real package produced by package-alias.mjs, so
// the install under test carries the shipped bin/script fields and the real
// postinstall migration script. Dependencies are stripped to keep the
// install offline and fast.
function buildMarttyTarball(root, packs) {
  const aliasDir = path.join(root, 'npm-martty')
  const result = run(process.execPath, [aliasScript, packageRoot, aliasDir, 'martty'])
  requireOk(result, 'generate martty alias package')
  const manifestPath = path.join(aliasDir, 'package.json')
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  manifest.dependencies = {}
  manifest.devDependencies = {}
  manifest.optionalDependencies = {}
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  return pack(aliasDir, packs)
}

test('global installs with overlapping bins', async (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-global-collision-'))
  t.after(() => rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }))
  const packs = path.join(root, 'packs')
  mkdirSync(packs, { recursive: true })

  const fixtures = {
    'legacy-0.2.18': { name: LEGACY_PACKAGE, version: '0.2.18', bins: ['dsh-tui'] },
    'legacy-0.2.20': { name: LEGACY_PACKAGE, version: '0.2.20', bins: ['martty', 'dsh-tui'] },
    'martty-0.3.1': { name: 'martty', version: '0.3.1', bins: ['martty', 'dsh-tui'] },
    'third-party-dsh-tui': { name: 'dsh-tui', version: '1.0.0', bins: ['dsh-tui'] },
  }
  const tarballs = {}
  for (const [key, { name, version, bins }] of Object.entries(fixtures)) {
    const dir = path.join(root, key)
    writeFixture(dir, { name, version, bin: binEntry(bins) }, bins)
    tarballs[key] = pack(dir, packs)
  }
  const martty = buildMarttyTarball(root, packs)

  const prefix = (name) => path.join(root, `prefix-${name}`)

  await t.test('a legacy package owning only dsh-tui no longer blocks martty', () => {
    const dir = prefix('legacy-dsh-only')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.18']), 'install legacy 0.2.18')

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty over legacy')
    assert.match(result.stderr, /still installed/)
    assert.match(result.stderr, /npm rm -g @openma\/deepseek-harness-tui/)
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false,
      'the stale dsh-tui link should be retired by the migration postinstall')
  })

  await t.test('an unrelated dsh-tui package no longer blocks martty', () => {
    const dir = prefix('third-party')
    requireOk(installGlobal(dir, tarballs['third-party-dsh-tui']), 'install third-party dsh-tui')

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty over third-party dsh-tui')
    assert.doesNotMatch(result.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    const target = readlinkSync(binPath(dir, 'dsh-tui'))
    assert.match(target, /node_modules\/dsh-tui\//,
      'the third-party dsh-tui link is left untouched')
  })

  await t.test('a legacy package owning martty still needs the documented workaround', () => {
    const dir = prefix('legacy-martty-bin')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.20']), 'install legacy 0.2.20')

    const blocked = installGlobal(dir, martty)
    assert.notEqual(blocked.status, 0, 'npm must refuse the foreign-owned martty bin')
    assert.match(blocked.stderr, /EEXIST/)
    assert.match(blocked.stderr, /bin\/martty|bin\\martty/)

    // The documented migration path: remove the legacy package, then install.
    requireOk(removeGlobal(dir, LEGACY_PACKAGE), 'remove legacy package')
    requireOk(installGlobal(dir, martty), 'install martty after the workaround')
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })

  await t.test('a fresh martty install stays silent', () => {
    const dir = prefix('fresh')
    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty')
    assert.doesNotMatch(result.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })

  await t.test('upgrading martty retires its own dsh-tui link', () => {
    const dir = prefix('upgrade')
    requireOk(installGlobal(dir, tarballs['martty-0.3.1']), 'install martty 0.3.1')
    assert.ok(existsSync(binPath(dir, 'dsh-tui')), 'martty 0.3.1 owns a dsh-tui link')

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'upgrade martty')
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false,
      'npm removes the bin entries the previous martty version owned')
    assert.doesNotMatch(result.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })
})

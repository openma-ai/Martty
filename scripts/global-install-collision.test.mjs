import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  lstatSync,
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
// `@openma/deepseek-harness-tui` releases declare `dsh-tui` (all versions),
// `dsb` (0.2.18/0.2.19-era), and `martty` (0.2.14/0.2.20); `npm link`ed
// checkouts carry the same package name; and an unrelated `dsh-tui` package
// owns that command name. The published martty package declares only
// `martty-cli` — a name nothing else owns — and its postinstall creates the
// `martty` command itself, so none of these can EEXIST the install.

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
    'legacy-0.2.18': { name: LEGACY_PACKAGE, version: '0.2.18', bins: ['dsh-tui', 'dsb'] },
    'legacy-0.2.20': { name: LEGACY_PACKAGE, version: '0.2.20', bins: ['martty', 'dsh-tui', 'dsb'] },
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

  await t.test('a legacy package owning only dsh-tui/dsb no longer blocks martty', () => {
    const dir = prefix('legacy-dsh-only')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.18']), 'install legacy 0.2.18')

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty over legacy')
    assert.match(result.stderr, /still installed/)
    assert.match(result.stderr, /npm rm -g @openma\/deepseek-harness-tui/)
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false,
      'the stale dsh-tui link should be retired by the migration postinstall')
    assert.equal(existsSync(binPath(dir, 'dsb')), false,
      'the stale dsb link should be retired by the migration postinstall')
  })

  await t.test('a legacy package owning martty no longer blocks the install', () => {
    const dir = prefix('legacy-martty-bin')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.20']), 'install legacy 0.2.20')

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty over legacy 0.2.20')
    assert.match(result.stderr, /removed the stale `martty`.*command/)
    const target = readlinkSync(binPath(dir, 'martty'))
    assert.match(target, /node_modules\/martty\//,
      'the postinstall replaced the legacy-owned martty link with ours')

    // Removing the legacy package afterwards leaves the new command intact.
    requireOk(removeGlobal(dir, LEGACY_PACKAGE), 'remove legacy package')
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })

  await t.test('an npm-linked legacy checkout no longer blocks the install', () => {
    const dir = prefix('npm-link')
    // Same observable state as `npm link`: node_modules/<name> symlinks at
    // the checkout and the checkout's bins are linked into <prefix>/bin.
    const checkout = path.join(root, 'linked-checkout')
    writeFixture(
      checkout,
      { name: LEGACY_PACKAGE, version: '0.0.0', bin: { martty: 'bin/martty.js', 'dsh-tui': 'bin/martty.js' } },
      ['martty'],
    )
    const checkoutManifest = readFileSync(path.join(checkout, 'package.json'), 'utf8')
    const scopedDir = path.join(dir, 'lib', 'node_modules', '@openma')
    mkdirSync(scopedDir, { recursive: true })
    requireOk(
      run('ln', ['-s', checkout, path.join(scopedDir, 'deepseek-harness-tui')]),
      'link checkout into the global root',
    )
    mkdirSync(path.join(dir, 'bin'), { recursive: true })
    for (const name of ['martty', 'dsh-tui']) {
      requireOk(
        run('ln', ['-s', `../lib/node_modules/${LEGACY_PACKAGE}/bin/martty.js`, path.join(dir, 'bin', name)]),
        `link ${name} bin`,
      )
    }

    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty over an npm-linked checkout')
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false)
    assert.match(result.stderr, /still installed/)

    // The linked checkout's node_modules symlink was swapped for a stub
    // (its package.json sits inside the user's source tree and must not be
    // edited); a later `npm rm -g` can then remove it without touching the
    // new `martty` command.
    const linkPath = path.join(dir, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui')
    assert.equal(lstatSync(linkPath).isSymbolicLink(), false, 'the checkout symlink is replaced')
    const stub = JSON.parse(readFileSync(path.join(linkPath, 'package.json'), 'utf8'))
    assert.equal(stub.name, LEGACY_PACKAGE)
    assert.equal('bin' in stub, false)
    assert.equal(readFileSync(path.join(checkout, 'package.json'), 'utf8'), checkoutManifest,
      'the user\'s checkout manifest is left untouched')

    requireOk(removeGlobal(dir, LEGACY_PACKAGE), 'remove stubbed legacy link')
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//,
      'the martty command survives removing the legacy entry')
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
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })

  await t.test('a fresh martty install stays silent', () => {
    const dir = prefix('fresh')
    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty')
    assert.doesNotMatch(result.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
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
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
  })

  await t.test('removing martty leaves only the documented dangling link', () => {
    const dir = prefix('removal')
    requireOk(installGlobal(dir, martty), 'install martty')
    requireOk(removeGlobal(dir, 'martty'), 'remove martty')

    assert.equal(existsSync(binPath(dir, 'martty-cli')), false,
      'npm removes the declared martty-cli link itself')
    // npm never runs uninstall hooks, so the postinstall-created `martty`
    // link is left dangling until the next install refreshes it — the
    // documented cleanup wart.
    assert.equal(existsSync(binPath(dir, 'martty')), false)
    requireOk(installGlobal(dir, martty), 'reinstall martty')
    assert.match(readlinkSync(binPath(dir, 'martty')), /node_modules\/martty\//)
  })
})

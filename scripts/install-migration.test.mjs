import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  migrate,
  warnAboutLegacyPackage,
  ensureMarttyLink,
  sanitizeLegacyInstall,
  isLegacyOwned,
  binCandidates,
  legacyInstallDirs,
  LEGACY_PACKAGE,
} from '../npm/lib/install-migration.js'

const script = new URL('../npm/lib/install-migration.js', import.meta.url).pathname

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-install-migration-'))
  t.after(() => rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }))
  const prefix = path.join(root, 'prefix')
  const lines = []
  const write = (line) => lines.push(line)
  const env = (overrides = {}) => ({
    npm_config_global: 'true',
    npm_config_prefix: prefix,
    npm_package_name: 'martty',
    ...overrides,
  })
  const legacyDir = path.join(prefix, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui')
  return { root, prefix, lines, write, env, legacyDir }
}

function installLegacy(prefix, { link = false, names = ['martty', 'dsh-tui', 'dsb'] } = {}) {
  const legacyDir = path.join(prefix, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui')
  mkdirSync(path.join(legacyDir, 'bin'), { recursive: true })
  writeFileSync(path.join(legacyDir, 'package.json'), '{"name":"@openma/deepseek-harness-tui"}')
  const target = path.join(legacyDir, 'bin', 'martty.js')
  writeFileSync(target, '#!/usr/bin/env node\n')
  mkdirSync(path.join(prefix, 'bin'), { recursive: true })
  for (const name of names) {
    if (link) {
      symlinkSync(path.join('..', 'lib', 'node_modules', '@openma', 'deepseek-harness-tui', 'bin', 'martty.js'),
        path.join(prefix, 'bin', name))
    } else {
      symlinkSync(target, path.join(prefix, 'bin', name))
    }
  }
  return legacyDir
}

test('ignores non-global installs', (t) => {
  const { env, write, lines } = fixture(t)
  migrate(env({ npm_config_global: undefined }), write)
  migrate(env({ npm_config_global: 'false' }), write)
  assert.deepEqual(lines, [])
})

test('ignores global installs without a resolvable prefix', (t) => {
  const { env, write, lines } = fixture(t)
  migrate(env({ npm_config_prefix: undefined }), write)
  assert.deepEqual(lines, [])
})

test('points legacy-package installs at martty', (t) => {
  const { env, write, lines } = fixture(t)
  migrate(env({ npm_package_name: '@openma/deepseek-harness-tui' }), write)
  assert.equal(lines.length, 2)
  assert.match(lines[0], /previous name.*martty/)
  assert.match(lines[1], /npm rm -g @openma\/deepseek-harness-tui/)
})

test('stays silent when the legacy package is absent', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  migrate(env(), write)
  migrate(env({ npm_package_name: undefined }), write)
  assert.deepEqual(lines, [])
  // …but it did create the `martty` command: postinstall self-links it
  // because npm never declares it for this package.
  const content = readFileSync(path.join(prefix, 'bin', 'martty'), 'utf8')
  assert.match(content, /martty-command-shim/)
})

test('retires every legacy-owned bin, creates martty, and names the fix', (t) => {
  const { prefix, env, write, lines, legacyDir } = fixture(t)
  installLegacy(prefix, { link: true })

  migrate(env(), write)

  assert.equal(lines.length, 3)
  assert.match(lines[0], /removed the stale `martty`, `dsh-tui`, `dsb` commands/)
  assert.match(lines[1], /still installed/)
  assert.match(lines[2], /npm rm -g @openma\/deepseek-harness-tui/)
  const command = path.join(prefix, 'bin', 'martty')
  assert.equal(lstatSync(command).isFile(), true)
  assert.equal(lstatSync(command).isSymbolicLink(), false)
  assert.match(readFileSync(command, 'utf8'), /bin\/martty\.js/)
  assert.throws(() => lstatSync(path.join(prefix, 'bin', 'dsh-tui')), /ENOENT/)
  assert.throws(() => lstatSync(path.join(prefix, 'bin', 'dsb')), /ENOENT/)
  assert.ok(legacyDir.endsWith(path.join('@openma', 'deepseek-harness-tui')))
})

test('leaves foreign bins alone but still nudges', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  installLegacy(prefix, { names: ['dsh-tui'] })
  const foreign = path.join(prefix, 'lib', 'node_modules', 'dsh-tui', 'bin')
  mkdirSync(foreign, { recursive: true })
  writeFileSync(path.join(foreign, 'cli.js'), '#!/usr/bin/env node\n')
  rmSync(path.join(prefix, 'bin', 'dsh-tui'))
  symlinkSync(path.join(foreign, 'cli.js'), path.join(prefix, 'bin', 'dsh-tui'))

  migrate(env(), write)

  assert.equal(lines.length, 2)
  assert.match(lines[0], /still installed/)
  assert.equal(readlinkSync(path.join(prefix, 'bin', 'dsh-tui')), path.join(foreign, 'cli.js'))
  assert.match(readFileSync(path.join(prefix, 'bin', 'martty'), 'utf8'), /martty-command-shim/)
})

test('detects the legacy package in the Windows-style layout', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  const legacyDir = path.join(prefix, 'node_modules', '@openma', 'deepseek-harness-tui')
  mkdirSync(legacyDir, { recursive: true })
  // Windows shims are regular files that embed the package path.
  writeFileSync(
    path.join(prefix, 'dsh-tui.cmd'),
    `@"%~dp0\\node_modules\\@openma\\deepseek-harness-tui\\bin\\martty.js" %*\r\n`,
  )
  writeFileSync(path.join(prefix, 'dsh-tui'), '#!/bin/sh\necho third-party\n')

  migrate(env(), write, 'win32')

  assert.equal(lines.length, 3)
  assert.match(lines[0], /removed the stale `dsh-tui` command/)
  assert.throws(() => lstatSync(path.join(prefix, 'dsh-tui.cmd')), /ENOENT/)
  assert.equal(lstatSync(path.join(prefix, 'dsh-tui')).isFile(), true)
  // The win32 path wrote the graceful shim triplet in the prefix root,
  // each pointing at this package's bin file (the source checkout here).
  for (const name of ['martty', 'martty.cmd', 'martty.ps1']) {
    const content = readFileSync(path.join(prefix, name), 'utf8')
    assert.match(content, /martty-command-shim/)
    assert.match(content, /martty\.js/)
  }
  assert.match(readFileSync(path.join(prefix, 'martty.cmd'), 'utf8'), /%~dp0/)
  assert.match(readFileSync(path.join(prefix, 'martty.cmd'), 'utf8'), /del "%~f0"/)
  assert.match(readFileSync(path.join(prefix, 'martty.ps1'), 'utf8'), /pwsh/)
  assert.match(readFileSync(path.join(prefix, 'martty.ps1'), 'utf8'), /Remove-Item/)
})

test('cleans stale bins even when the legacy directory is gone', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  const dangling = path.join(prefix, 'bin')
  mkdirSync(dangling, { recursive: true })
  symlinkSync(
    path.join('..', 'lib', 'node_modules', '@openma', 'deepseek-harness-tui', 'bin', 'martty.js'),
    path.join(dangling, 'dsh-tui'),
  )

  migrate(env(), write)

  assert.equal(lines.length, 1)
  assert.match(lines[0], /removed the stale `dsh-tui` command/)
  assert.throws(() => lstatSync(path.join(dangling, 'dsh-tui')), /ENOENT/)
  assert.match(readFileSync(path.join(dangling, 'martty'), 'utf8'), /martty-command-shim/)
})

test('isLegacyOwned handles every entry kind', (t) => {
  const { prefix } = fixture(t)
  const root = path.join(prefix, 'bins')
  mkdirSync(root, { recursive: true })

  assert.equal(isLegacyOwned(path.join(root, 'missing')), false)

  const foreign = path.join(root, 'foreign.js')
  writeFileSync(foreign, '#!/usr/bin/env node\n')
  const foreignLink = path.join(root, 'foreign-link')
  symlinkSync(foreign, foreignLink)
  assert.equal(isLegacyOwned(foreignLink), false)

  assert.equal(isLegacyOwned(root), false)

  const unrelated = path.join(root, 'unrelated')
  writeFileSync(unrelated, 'echo hello\n')
  assert.equal(isLegacyOwned(unrelated), false)

  const shim = path.join(root, 'shim.cmd')
  writeFileSync(shim, 'node "%~dp0\\node_modules\\@openma\\deepseek-harness-tui\\bin\\martty.js" %*')
  assert.equal(isLegacyOwned(shim), true)

  const legacyLink = path.join(root, 'legacy-link')
  symlinkSync(
    path.join('..', 'lib', 'node_modules', '@openma', 'deepseek-harness-tui', 'bin', 'martty.js'),
    legacyLink,
  )
  assert.equal(isLegacyOwned(legacyLink), true)
})

test('a bin entry that cannot be inspected or removed never fails the install', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  installLegacy(prefix, { names: ['dsh-tui'] })
  const unreadable = path.join(prefix, 'bin', 'unreadable')
  writeFileSync(unreadable, 'x')
  chmodSync(unreadable, 0o000)
  try {
    assert.equal(isLegacyOwned(unreadable), false)
  } finally {
    chmodSync(unreadable, 0o644)
  }
  const blocked = path.join(prefix, 'bin', 'dsh-tui')
  rmSync(blocked)
  writeFileSync(blocked, 'deepseek-harness-tui')
  chmodSync(path.join(prefix, 'bin'), 0o555)
  try {
    migrate(env(), write)
  } finally {
    chmodSync(path.join(prefix, 'bin'), 0o755)
  }
  assert.equal(lines.length, 3)
  assert.match(lines[0], /could not create the `martty` command/)
  assert.match(lines[1], /still installed/)
  rmSync(blocked, { force: true })
})

test('helper paths cover both layouts', () => {
  assert.deepEqual(legacyInstallDirs('/p'), [
    path.join('/p', 'lib', 'node_modules', '@openma', 'deepseek-harness-tui'),
    path.join('/p', 'node_modules', '@openma', 'deepseek-harness-tui'),
  ])
  assert.deepEqual(binCandidates('/p', 'dsh-tui'), [
    path.join('/p', 'bin', 'dsh-tui'),
    path.join('/p', 'dsh-tui'),
    path.join('/p', 'dsh-tui.cmd'),
    path.join('/p', 'dsh-tui.ps1'),
  ])
  assert.equal(LEGACY_PACKAGE, '@openma/deepseek-harness-tui')
})

test('runs standalone under npm lifecycle environment variables', (t) => {
  const { prefix, legacyDir } = fixture(t)
  installLegacy(prefix, { link: true })
  const result = spawnSync(process.execPath, [script], {
    encoding: 'utf8',
    env: {
      PATH: process.env.PATH,
      npm_config_global: 'true',
      npm_config_prefix: prefix,
      npm_package_name: 'martty',
    },
  })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stderr, /removed the stale `martty`, `dsh-tui`, `dsb` commands/)
  assert.match(result.stderr, /npm rm -g @openma\/deepseek-harness-tui/)
  assert.match(readFileSync(path.join(prefix, 'bin', 'martty'), 'utf8'), /martty-command-shim/)
  assert.throws(() => lstatSync(path.join(prefix, 'bin', 'dsh-tui')), /ENOENT/)
  assert.ok(legacyDir)
})

// ensureMarttyLink: creates the user-facing `martty` command next to a
// `martty-cli` invocation (or inside an explicit binDir) without ever
// touching foreign-owned entries.

function linkFixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-ensure-link-'))
  t.after(() => rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }))
  const packageDir = path.join(root, 'node_modules', 'martty')
  const binDir = path.join(root, 'prefix', 'bin')
  mkdirSync(path.join(packageDir, 'bin'), { recursive: true })
  mkdirSync(binDir, { recursive: true })
  const lines = []
  return { root, packageDir, binDir, lines, write: (line) => lines.push(line) }
}

test('ensureMarttyLink creates the martty shim next to martty-cli', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)

  const created = ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write })

  assert.equal(created, true)
  const target = path.join(binDir, 'martty')
  assert.equal(lstatSync(target).isFile(), true)
  const content = readFileSync(target, 'utf8')
  assert.match(content, /martty-command-shim/)
  assert.match(content, /node_modules\/martty\/bin\/martty\.js/)
  assert.equal(lstatSync(target).mode & 0o111, 0o111, 'the shim is executable')
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink only acts on martty-cli invocations', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  for (const argv1 of [
    path.join(binDir, 'martty'),
    path.join(binDir, 'node'),
    '/usr/bin/martty.js',
    undefined,
  ]) {
    assert.equal(ensureMarttyLink({ argv1, packageDir, write }), false)
  }
  assert.throws(() => lstatSync(path.join(binDir, 'martty')), /ENOENT/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink accepts the Windows shim suffixes', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli.cmd'), packageDir, write }), true)
  rmSync(path.join(binDir, 'martty'))
  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli.ps1'), packageDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /martty-command-shim/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink is a no-op when the link is already ours', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  const binFile = path.join(packageDir, 'bin', 'martty.js')
  symlinkSync(path.relative(binDir, binFile), path.join(binDir, 'martty'))

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink refreshes a dangling link into the martty package', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  // Dangling symlink: the target no longer exists (e.g. after `npm rm -g
  // martty` removed the package but left this hand-created link).
  symlinkSync(
    path.join('..', 'lib', 'node_modules', 'martty', 'bin', 'martty.js'),
    path.join(binDir, 'martty'),
  )

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /bin\/martty\.js/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink replaces a legacy-owned martty link', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  symlinkSync(
    path.join('..', 'lib', 'node_modules', '@openma', 'deepseek-harness-tui', 'bin', 'martty.js'),
    path.join(binDir, 'martty'),
  )

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /node_modules\/martty\/bin\/martty\.js/)
})

test('ensureMarttyLink leaves a live link into another martty install alone', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  // A link that resolves inside some `martty/` directory — but not at this
  // package's own bin file — counts as another martty installation's.
  const otherBin = path.join(binDir, '..', 'lib', 'node_modules', 'martty', 'bin', 'older.js')
  mkdirSync(path.dirname(otherBin), { recursive: true })
  writeFileSync(otherBin, '#!/usr/bin/env node\n')
  symlinkSync(path.relative(binDir, otherBin), path.join(binDir, 'martty'))

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.match(readlinkSync(path.join(binDir, 'martty')), /martty\/bin\/older\.js$/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink replaces a legacy-owned martty shim file', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  // Windows-style shim content that embeds the legacy package path.
  writeFileSync(path.join(binDir, 'martty'),
    `@"%~dp0\\lib\\node_modules\\@openma\\deepseek-harness-tui\\bin\\martty.js" %*\r\n`)

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /martty-command-shim/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink leaves a shim file pointing into martty alone', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  writeFileSync(path.join(binDir, 'martty'),
    '@"%~dp0\\node_modules\\martty\\bin\\martty.js" %*\r\n')

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /node_modules\\martty\\bin\\martty\.js/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink resolves the package directory from import.meta', (t) => {
  const { root, write, lines } = linkFixture(t)
  const binDir = path.join(root, 'empty-bin')
  mkdirSync(binDir, { recursive: true })

  // No packageDir: falls back to this file's own package root (the npm/
  // source directory), still creating the command.
  assert.equal(ensureMarttyLink({ binDir, write }), true)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /martty-command-shim/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink never touches a foreign martty', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  const foreign = path.join(binDir, 'martty')
  writeFileSync(foreign, '#!/bin/sh\necho not martty\n')

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), false)
  assert.equal(readFileSync(foreign, 'utf8'), '#!/bin/sh\necho not martty\n')
  assert.match(lines[0], /belongs to another tool/)
  assert.match(lines[1], /martty-cli/)
})

test('ensureMarttyLink treats a directory at the command slot as foreign', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  mkdirSync(path.join(binDir, 'martty'), { recursive: true })

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), false)
  assert.match(lines[0], /belongs to another tool/)
})

test('ensureMarttyLink leaves an unreadable foreign file alone', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  const unreadable = path.join(binDir, 'martty')
  writeFileSync(unreadable, 'x')
  chmodSync(unreadable, 0o000)
  try {
    assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), false)
    assert.match(lines[0], /belongs to another tool/)
  } finally {
    chmodSync(unreadable, 0o644)
  }
})

test('ensureMarttyLink writes the Windows shim triplet on win32', (t) => {
  const { packageDir, write, lines, root } = linkFixture(t)
  const dir = path.join(root, 'winprefix')
  mkdirSync(dir, { recursive: true })

  assert.equal(
    ensureMarttyLink({ binDir: dir, packageDir, platform: 'win32', write }),
    true,
  )
  for (const name of ['martty', 'martty.cmd', 'martty.ps1']) {
    const content = readFileSync(path.join(dir, name), 'utf8')
    assert.match(content, /martty-command-shim/)
    assert.match(content, /node_modules.martty.bin.martty\.js/)
  }
  assert.match(readFileSync(path.join(dir, 'martty'), 'utf8'), /^#!\/bin\/sh/)
  assert.match(readFileSync(path.join(dir, 'martty.cmd'), 'utf8'), /@echo off/)
  assert.match(readFileSync(path.join(dir, 'martty.ps1'), 'utf8'), /pwsh/)
  assert.deepEqual(lines, [])
})

test('ensureMarttyLink reports write failures instead of throwing', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  chmodSync(binDir, 0o555)
  try {
    assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), false)
  } finally {
    chmodSync(binDir, 0o755)
  }
  assert.match(lines[0], /could not create the `martty` command/)
  assert.match(lines[0], /martty-cli/)
})

test('ensureMarttyLink keeps a foreign symlink untouched', (t) => {
  const { packageDir, binDir, write, lines } = linkFixture(t)
  const foreignTarget = path.join(binDir, 'other-tool.js')
  writeFileSync(foreignTarget, '#!/usr/bin/env node\n')
  symlinkSync(foreignTarget, path.join(binDir, 'martty'))

  assert.equal(ensureMarttyLink({ argv1: path.join(binDir, 'martty-cli'), packageDir, write }), false)
  assert.equal(readlinkSync(path.join(binDir, 'martty')), foreignTarget)
  assert.match(lines[0], /belongs to another tool/)
})

// sanitizeLegacyInstall: strips `bin` off the legacy package's installed
// manifest so a later `npm rm -g` (which deletes bin links by name without
// checking targets) cannot take the new `martty` command down with it.

function legacyManifestDir(prefix) {
  return path.join(prefix, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui')
}

test('sanitizeLegacyInstall strips bin from a real legacy manifest', (t) => {
  const { prefix, lines, write } = fixture(t)
  const legacyDir = legacyManifestDir(prefix)
  mkdirSync(legacyDir, { recursive: true })
  writeFileSync(path.join(legacyDir, 'package.json'), JSON.stringify({
    name: LEGACY_PACKAGE,
    version: '0.2.20',
    description: 'the previous name',
    bin: { martty: 'bin/martty.js', 'dsh-tui': 'bin/martty.js' },
  }))

  sanitizeLegacyInstall(prefix, write)

  assert.deepEqual(JSON.parse(readFileSync(path.join(legacyDir, 'package.json'), 'utf8')), {
    name: LEGACY_PACKAGE,
    version: '0.2.20',
    description: 'the previous name',
  })
  assert.deepEqual(lines, [])
})

test('sanitizeLegacyInstall replaces an npm-linked legacy dir with a stub', (t) => {
  const { prefix, root, lines, write } = fixture(t)
  const checkout = path.join(root, 'checkout')
  mkdirSync(checkout, { recursive: true })
  const checkoutManifest = JSON.stringify({
    name: LEGACY_PACKAGE,
    version: '0.3.0-dev',
    bin: { martty: 'bin/martty.js' },
  })
  writeFileSync(path.join(checkout, 'package.json'), checkoutManifest)
  const scoped = path.join(prefix, 'lib', 'node_modules', '@openma')
  mkdirSync(scoped, { recursive: true })
  const linkPath = path.join(scoped, 'deepseek-harness-tui')
  symlinkSync(checkout, linkPath, 'dir')

  sanitizeLegacyInstall(prefix, write)

  assert.equal(lstatSync(linkPath).isSymbolicLink(), false)
  assert.deepEqual(JSON.parse(readFileSync(path.join(linkPath, 'package.json'), 'utf8')), {
    name: LEGACY_PACKAGE,
    version: '0.3.0-dev',
  })
  assert.equal(readFileSync(path.join(checkout, 'package.json'), 'utf8'), checkoutManifest,
    'the user\'s checkout manifest is left untouched')
  assert.deepEqual(lines, [])
})

test('sanitizeLegacyInstall stubs a linked dir whose manifest cannot be read', (t) => {
  const { prefix, root, lines, write } = fixture(t)
  const scoped = path.join(prefix, 'lib', 'node_modules', '@openma')
  mkdirSync(scoped, { recursive: true })
  const linkPath = path.join(scoped, 'deepseek-harness-tui')
  symlinkSync(path.join(root, 'gone'), linkPath, 'dir')
  // A non-string version is dropped from the stub too: reuse the Windows
  // layout slot for a second linked manifest.
  const winCheckout = path.join(root, 'win-checkout')
  mkdirSync(winCheckout, { recursive: true })
  writeFileSync(path.join(winCheckout, 'package.json'), JSON.stringify({ name: LEGACY_PACKAGE, version: 7 }))
  const winScoped = path.join(prefix, 'node_modules', '@openma')
  mkdirSync(winScoped, { recursive: true })
  const winLink = path.join(winScoped, 'deepseek-harness-tui')
  symlinkSync(winCheckout, winLink, 'dir')

  sanitizeLegacyInstall(prefix, write)

  const stub = JSON.parse(readFileSync(path.join(linkPath, 'package.json'), 'utf8'))
  assert.equal(stub.name, LEGACY_PACKAGE)
  assert.equal('version' in stub, false)
  const winStub = JSON.parse(readFileSync(path.join(winLink, 'package.json'), 'utf8'))
  assert.equal(winStub.name, LEGACY_PACKAGE)
  assert.equal('version' in winStub, false)
  assert.deepEqual(lines, [])
})

test('sanitizeLegacyInstall warns when the stub cannot replace the link', (t) => {
  const { prefix, root, lines, write } = fixture(t)
  const checkout = path.join(root, 'checkout')
  mkdirSync(checkout, { recursive: true })
  writeFileSync(path.join(checkout, 'package.json'), JSON.stringify({ name: LEGACY_PACKAGE }))
  const scoped = path.join(prefix, 'lib', 'node_modules', '@openma')
  mkdirSync(scoped, { recursive: true })
  const linkPath = path.join(scoped, 'deepseek-harness-tui')
  symlinkSync(checkout, linkPath, 'dir')
  chmodSync(scoped, 0o555)

  sanitizeLegacyInstall(prefix, write)
  chmodSync(scoped, 0o755)

  assert.match(lines[0], /could not detach/)
})

test('sanitizeLegacyInstall skips non-directories and manifests without bin', (t) => {
  const { prefix, lines, write } = fixture(t)
  const legacyDir = legacyManifestDir(prefix)
  mkdirSync(path.dirname(legacyDir), { recursive: true })
  writeFileSync(legacyDir, 'not a directory')
  const winDir = path.join(prefix, 'node_modules', '@openma', 'deepseek-harness-tui')
  mkdirSync(winDir, { recursive: true })
  writeFileSync(path.join(winDir, 'package.json'), JSON.stringify({ name: LEGACY_PACKAGE, version: '0.2.19' }))

  sanitizeLegacyInstall(prefix, write)

  assert.equal(readFileSync(legacyDir, 'utf8'), 'not a directory')
  assert.deepEqual(JSON.parse(readFileSync(path.join(winDir, 'package.json'), 'utf8')).version, '0.2.19')
  assert.deepEqual(lines, [])
})

test('sanitizeLegacyInstall warns when the manifest cannot be rewritten', (t) => {
  const { prefix, lines, write } = fixture(t)
  const legacyDir = legacyManifestDir(prefix)
  mkdirSync(legacyDir, { recursive: true })
  // Unreadable manifests are skipped silently — npm cannot unlink bins it
  // cannot read either.
  writeFileSync(path.join(legacyDir, 'package.json'), 'not json {')
  sanitizeLegacyInstall(prefix, write)
  writeFileSync(path.join(legacyDir, 'package.json'), 'null')
  sanitizeLegacyInstall(prefix, write)
  assert.deepEqual(lines, [])

  writeFileSync(path.join(legacyDir, 'package.json'), JSON.stringify({ name: LEGACY_PACKAGE, bin: { martty: 'bin/martty.js' } }))
  const manifestPath = path.join(legacyDir, 'package.json')
  chmodSync(manifestPath, 0o444)

  sanitizeLegacyInstall(prefix, write)
  chmodSync(manifestPath, 0o644)

  assert.match(lines[0], /could not edit/)
})

test('migrating also strips bin declarations off the installed legacy manifest', (t) => {
  const { prefix, env, write, legacyDir } = fixture(t)
  installLegacy(prefix)
  writeFileSync(path.join(legacyDir, 'package.json'), JSON.stringify({
    name: LEGACY_PACKAGE,
    version: '0.2.20',
    bin: { martty: 'bin/martty.js' },
  }))

  migrate(env(), write)

  assert.equal('bin' in JSON.parse(readFileSync(path.join(legacyDir, 'package.json'), 'utf8')), false)
})

// warnAboutLegacyPackage: the launch-time check bin/martty.js runs. libDir
// points at the package's lib directory, so the package root is one level up.

function runtimeFixture(t, { pkgName = 'martty', scope = 'global' } = {}) {
  const { root } = { root: mkdtempSync(path.join(tmpdir(), 'martty-runtime-warn-')) }
  t.after(() => rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }))
  const nodeModules = scope === 'global'
    ? path.join(root, 'prefix', 'lib', 'node_modules')
    : path.join(root, 'profiles', scope, 'node_modules')
  const libDir = path.join(nodeModules, pkgName, 'lib')
  mkdirSync(libDir, { recursive: true })
  const lines = []
  return { root, nodeModules, libDir, lines, write: (line) => lines.push(line) }
}

test('warns at launch while the legacy package sits next to martty', (t) => {
  const { nodeModules, libDir, lines, write } = runtimeFixture(t)
  mkdirSync(path.join(nodeModules, '@openma', 'deepseek-harness-tui'), { recursive: true })

  warnAboutLegacyPackage({ libDir, write })

  assert.equal(lines.length, 2)
  assert.match(lines[0], /still installed/)
  assert.match(lines[1], /npm rm -g @openma\/deepseek-harness-tui/)
})

test('suggests the dsh plugin command inside a profile install', (t) => {
  const { nodeModules, libDir, lines, write } = runtimeFixture(t, { scope: 'martty' })
  mkdirSync(path.join(nodeModules, '@openma', 'deepseek-harness-tui'), { recursive: true })

  warnAboutLegacyPackage({ libDir, write })

  assert.equal(lines.length, 2)
  assert.match(lines[1], /dsh plugin --profile martty remove @openma\/deepseek-harness-tui/)
})

test('detects the legacy package through a linked bin path', (t) => {
  const { root, libDir, lines, write } = runtimeFixture(t)
  const prefix = path.join(root, 'linked-prefix')
  mkdirSync(path.join(prefix, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui'), { recursive: true })
  mkdirSync(path.join(prefix, 'bin'), { recursive: true })

  warnAboutLegacyPackage({ libDir, argv1: path.join(prefix, 'bin', 'martty'), write })

  assert.equal(lines.length, 2)
  assert.match(lines[1], /npm rm -g @openma\/deepseek-harness-tui/)
})

test('creates the martty command at launch when invoked as martty-cli', (t) => {
  const { root, libDir, lines, write } = runtimeFixture(t)
  const binDir = path.join(root, 'prefix', 'bin')
  mkdirSync(binDir, { recursive: true })

  warnAboutLegacyPackage({ libDir, argv1: path.join(binDir, 'martty-cli'), write })

  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /martty-command-shim/)
  assert.deepEqual(lines, [])
})

test('stays silent at launch when nothing is left to migrate', (t) => {
  const { libDir, lines, write } = runtimeFixture(t)
  warnAboutLegacyPackage({ libDir, argv1: '/usr/bin/node', write })
  warnAboutLegacyPackage({ libDir, argv1: path.join('no-bin-here', 'martty'), write })
  warnAboutLegacyPackage({ libDir, argv1: undefined, write })
  // A failure inside the link check is swallowed — the launch must never
  // break on it. A foreign-owned `martty` next to martty-cli routes through
  // the write call; making it throw exercises the guard.
  const { root } = runtimeFixture(t)
  const binDir = path.join(root, 'pfx', 'bin')
  mkdirSync(binDir, { recursive: true })
  writeFileSync(path.join(binDir, 'martty'), '#!/bin/sh\necho other\n')
  warnAboutLegacyPackage({
    libDir,
    argv1: path.join(binDir, 'martty-cli'),
    write: () => { throw new Error('stderr gone') },
  })
  // The repository checkout is not inside an installed package tree, so the
  // real defaults (own lib dir, stderr) resolve to a silent no-op here too.
  warnAboutLegacyPackage()
  assert.deepEqual(lines, [])
})

test('points launches of the legacy binary itself at martty', (t) => {
  const { libDir, lines, write } = runtimeFixture(t, { pkgName: 'deepseek-harness-tui' })

  warnAboutLegacyPackage({ libDir, write })

  assert.equal(lines.length, 2)
  assert.match(lines[0], /previous name.*martty/)
  assert.match(lines[1], /npm rm -g @openma\/deepseek-harness-tui/)
})

// The generated `martty` shim is a real file users can run: it execs the
// package binary while installed, and once `npm rm -g martty` has taken the
// package away it reports the reinstall hint and removes itself — no dead
// symlink is left behind.

test('the martty shim execs the package binary', (t) => {
  const { packageDir, binDir } = linkFixture(t)
  const binFile = path.join(packageDir, 'bin', 'martty.js')
  writeFileSync(binFile, '#!/usr/bin/env node\nconsole.log("shim-target-ran")\n')
  ensureMarttyLink({ binDir, packageDir, write: () => {} })

  const result = spawnSync(path.join(binDir, 'martty'), ['--version'], { encoding: 'utf8' })

  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /shim-target-ran/)
})

test('the martty shim reports the reinstall hint and removes itself', (t) => {
  const { packageDir, binDir } = linkFixture(t)
  ensureMarttyLink({ binDir, packageDir, write: () => {} })
  const shim = path.join(binDir, 'martty')

  const result = spawnSync(shim, [], { encoding: 'utf8' })

  assert.equal(result.status, 127)
  assert.match(result.stderr, /not installed/)
  assert.match(result.stderr, /npm i -g martty/)
  assert.throws(() => lstatSync(shim), /ENOENT/, 'the shim cleaned itself up')
})

// warnAboutLegacyPackage converges a global prefix at launch exactly like
// the postinstall would — this is the recovery path for installs whose
// lifecycle scripts never ran (`--ignore-scripts`, allowScripts policies).

test('launch as martty-cli converges the prefix like the postinstall', (t) => {
  const { root, libDir, lines, write } = runtimeFixture(t)
  const prefix = path.join(root, 'prefix')
  const legacyDir = installLegacy(prefix, { link: true })
  writeFileSync(path.join(legacyDir, 'package.json'), JSON.stringify({
    name: LEGACY_PACKAGE,
    version: '0.2.20',
    bin: { martty: 'bin/martty.js', 'dsh-tui': 'bin/martty.js' },
  }))
  const binDir = path.join(prefix, 'bin')

  warnAboutLegacyPackage({ libDir, argv1: path.join(binDir, 'martty-cli'), write })

  assert.throws(() => lstatSync(path.join(binDir, 'dsh-tui')), /ENOENT/)
  assert.throws(() => lstatSync(path.join(binDir, 'dsb')), /ENOENT/)
  assert.match(readFileSync(path.join(binDir, 'martty'), 'utf8'), /martty-command-shim/)
  assert.equal(
    'bin' in JSON.parse(readFileSync(path.join(legacyDir, 'package.json'), 'utf8')),
    false,
    'the legacy manifest was sanitized so `npm rm -g` cannot take martty down',
  )
  assert.match(lines.at(-1), /npm rm -g @openma\/deepseek-harness-tui/)
})

test('launch convergence covers the win32 argv[1] prefix layout', (t) => {
  const { root, libDir, lines, write } = runtimeFixture(t)
  const prefix = path.join(root, 'winprefix')
  mkdirSync(path.join(prefix, 'node_modules', '@openma', 'deepseek-harness-tui'), { recursive: true })
  writeFileSync(
    path.join(prefix, 'dsh-tui.cmd'),
    `@"%~dp0\\node_modules\\@openma\\deepseek-harness-tui\\bin\\martty.js" %*\r\n`,
  )

  warnAboutLegacyPackage({
    libDir,
    argv1: path.join(prefix, 'martty-cli.cmd'),
    write,
    platform: 'win32',
  })

  assert.throws(() => lstatSync(path.join(prefix, 'dsh-tui.cmd')), /ENOENT/)
  assert.match(readFileSync(path.join(prefix, 'martty'), 'utf8'), /martty-command-shim/)
  assert.match(readFileSync(path.join(prefix, 'martty.ps1'), 'utf8'), /Remove-Item/)
  assert.match(lines.at(-1), /npm rm -g @openma\/deepseek-harness-tui/)
})

test('a profile-scoped launch warns without touching the filesystem', (t) => {
  const { root, nodeModules, libDir, lines, write } = runtimeFixture(t, { scope: 'martty' })
  const legacyDir = path.join(nodeModules, '@openma', 'deepseek-harness-tui')
  mkdirSync(legacyDir, { recursive: true })
  const staleBin = path.join(root, 'profiles', 'martty', 'bin', 'dsh-tui')
  mkdirSync(path.dirname(staleBin), { recursive: true })
  writeFileSync(staleBin, `@"%~dp0\\node_modules\\@openma\\deepseek-harness-tui\\bin\\martty.js" %*\r\n`)

  warnAboutLegacyPackage({ libDir, argv1: path.join(root, 'profiles', 'martty', 'bin', 'martty-cli'), write })

  assert.match(lines[0], /still installed/)
  assert.match(lines[1], /dsh plugin --profile martty remove/)
  assert.equal(readFileSync(staleBin, 'utf8').includes('deepseek-harness-tui'), true,
    'profile bin entries are managed by dsh — the launch check never rewrites them')
})

test('launch converge swallows mid-migration failures and still warns', (t) => {
  const { root, libDir, lines } = runtimeFixture(t)
  const prefix = path.join(root, 'prefix')
  installLegacy(prefix, { link: true })
  // A write that throws once inside the converge must not break the
  // launch — the remaining steps are skipped but the warning still lands.
  let calls = 0
  const write = (line) => {
    calls += 1
    if (calls === 1) throw new Error('write failed')
    lines.push(line)
  }

  warnAboutLegacyPackage({ libDir, argv1: path.join(prefix, 'bin', 'martty-cli'), write })

  assert.match(lines.at(-1), /npm rm -g @openma\/deepseek-harness-tui/)
})

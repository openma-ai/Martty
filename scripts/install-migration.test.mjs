import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
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

function installLegacy(prefix, { link = false } = {}) {
  const legacyDir = path.join(prefix, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui')
  mkdirSync(path.join(legacyDir, 'bin'), { recursive: true })
  writeFileSync(path.join(legacyDir, 'package.json'), '{"name":"@openma/deepseek-harness-tui"}')
  const target = path.join(legacyDir, 'bin', 'martty.js')
  writeFileSync(target, '#!/usr/bin/env node\n')
  mkdirSync(path.join(prefix, 'bin'), { recursive: true })
  for (const name of ['martty', 'dsh-tui']) {
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
  const { env, write, lines } = fixture(t)
  migrate(env(), write)
  migrate(env({ npm_package_name: undefined }), write)
  assert.deepEqual(lines, [])
})

test('removes stale legacy dsh-tui links and names the uninstall command', (t) => {
  const { prefix, env, write, lines, legacyDir } = fixture(t)
  installLegacy(prefix, { link: true })

  migrate(env(), write)

  assert.equal(lines.length, 3)
  assert.match(lines[0], /removed the stale `dsh-tui` command/)
  assert.match(lines[1], /still installed/)
  assert.match(lines[2], /npm rm -g @openma\/deepseek-harness-tui/)
  assert.equal(lstatSync(path.join(prefix, 'bin', 'martty')).isSymbolicLink(), true)
  assert.throws(() => lstatSync(path.join(prefix, 'bin', 'dsh-tui')), /ENOENT/)
  assert.ok(legacyDir.endsWith(path.join('@openma', 'deepseek-harness-tui')))
})

test('leaves foreign dsh-tui bins alone but still nudges', (t) => {
  const { prefix, env, write, lines } = fixture(t)
  installLegacy(prefix)
  rmSync(path.join(prefix, 'bin', 'dsh-tui'))
  const foreign = path.join(prefix, 'lib', 'node_modules', 'dsh-tui', 'bin')
  mkdirSync(foreign, { recursive: true })
  writeFileSync(path.join(foreign, 'cli.js'), '#!/usr/bin/env node\n')
  symlinkSync(path.join(foreign, 'cli.js'), path.join(prefix, 'bin', 'dsh-tui'))

  migrate(env(), write)

  assert.equal(lines.length, 2)
  assert.match(lines[0], /still installed/)
  assert.equal(readlinkSync(path.join(prefix, 'bin', 'dsh-tui')), path.join(foreign, 'cli.js'))
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

  migrate(env(), write)

  assert.equal(lines.length, 3)
  assert.match(lines[0], /removed the stale `dsh-tui` command/)
  assert.throws(() => lstatSync(path.join(prefix, 'dsh-tui.cmd')), /ENOENT/)
  assert.equal(lstatSync(path.join(prefix, 'dsh-tui')).isFile(), true)
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
  installLegacy(prefix)
  const shim = path.join(prefix, 'bin', 'dsh-tui')
  rmSync(shim)
  const unreadable = path.join(prefix, 'bin', 'unreadable')
  writeFileSync(unreadable, 'x')
  chmodSync(unreadable, 0o000)
  try {
    assert.equal(isLegacyOwned(unreadable), false)
  } finally {
    chmodSync(unreadable, 0o644)
  }
  const blocked = path.join(prefix, 'bin', 'dsh-tui')
  writeFileSync(blocked, 'deepseek-harness-tui')
  chmodSync(path.join(prefix, 'bin'), 0o555)
  try {
    migrate(env(), write)
  } finally {
    chmodSync(path.join(prefix, 'bin'), 0o755)
  }
  assert.equal(lines.length, 2)
  assert.match(lines[0], /still installed/)
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
  assert.match(result.stderr, /removed the stale `dsh-tui` command/)
  assert.match(result.stderr, /npm rm -g @openma\/deepseek-harness-tui/)
  assert.throws(() => lstatSync(path.join(prefix, 'bin', 'dsh-tui')), /ENOENT/)
  assert.ok(legacyDir)
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

test('stays silent at launch when nothing is left to migrate', (t) => {
  const { libDir, lines, write } = runtimeFixture(t)
  warnAboutLegacyPackage({ libDir, argv1: '/usr/bin/node', write })
  warnAboutLegacyPackage({ libDir, argv1: path.join('no-bin-here', 'martty'), write })
  warnAboutLegacyPackage({ libDir, argv1: undefined, write })
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

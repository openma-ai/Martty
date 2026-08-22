import assert from 'node:assert/strict'
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const repoRoot = path.resolve(import.meta.dirname, '..')
const packageRoot = path.join(repoRoot, 'npm')
const acpRoot = path.resolve(
  process.env.DSH_TUI_ACP_ROOT ?? path.resolve(repoRoot, '../openma/deepseek-harness-acp'),
)
const smokeTui = path.join(import.meta.dirname, 'profile-smoke-tui.mjs')
const smokeBin = path.resolve(process.env.DSH_TUI_BIN ?? smokeTui)

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    encoding: 'utf8',
    timeout: 120_000,
    ...options,
    env: { ...process.env, ...options.env },
  })
}

function requireOk(result, operation) {
  assert.equal(
    result.status,
    0,
    `${operation} failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  )
}

function pack(directory, destination) {
  const result = run('npm', [
    'pack', '--silent', '--ignore-scripts', '--pack-destination', destination,
  ], { cwd: directory })
  requireOk(result, `pack ${directory}`)
  const filename = result.stdout.trim().split(/\r?\n/).at(-1)
  assert.ok(filename, `npm pack printed no filename for ${directory}`)
  return path.join(destination, filename)
}

function preparePackages(root) {
  const packs = path.join(root, 'packs')
  mkdirSync(packs, { recursive: true })
  const acp = pack(acpRoot, packs)

  // Keep the published dependency shape while pinning this E2E to the two
  // unpublished worktrees under test.  The user's command remains the exact
  // bare package command; only the test registry resolution is substituted.
  const fixture = path.join(root, 'tui-package')
  mkdirSync(fixture, { recursive: true })
  for (const name of [
    'LICENSE', 'README.md', 'bin', 'cordis.patch.yml', 'creator', 'lib', 'skills', 'vendor',
  ]) {
    cpSync(path.join(packageRoot, name), path.join(fixture, name), {
      recursive: true,
    })
  }
  const manifest = JSON.parse(readFileSync(path.join(packageRoot, 'package.json'), 'utf8'))
  manifest.dependencies = {
    ...manifest.dependencies,
    '@openma/deepseek-harness-acp': `file:${acp}`,
  }
  writeFileSync(
    path.join(fixture, 'package.json'),
    `${JSON.stringify(manifest, null, 2)}\n`,
  )
  return { acp, tui: pack(fixture, packs) }
}

function profileDir(home) {
  return path.join(home, 'profiles', 'tui')
}

function writeWorkspaceOverride(home, tuiPackage) {
  const dir = profileDir(home)
  mkdirSync(dir, { recursive: true })
  writeFileSync(
    path.join(dir, 'pnpm-workspace.yaml'),
    [
      'packages:',
      '  - .',
      '',
      'nodeLinker: hoisted',
      'autoInstallPeers: false',
      'overrides:',
      `  "@openma/deepseek-harness-tui": ${JSON.stringify(`file:${tuiPackage}`)}`,
      '',
    ].join('\n'),
  )
}

function runDsh(home, args, cwd = repoRoot, timeout = 120_000) {
  return run('dsh', args, {
    cwd,
    timeout,
    env: {
      DSH_HOME: home,
      DSH_TELEMETRY_DISABLED: '1',
      DSH_TUI_BIN: smokeBin,
      DSH_TUI_AGENT: 'dsh-tui-matrix-must-not-spawn-an-acp-process',
      DEEPSEEK_API_KEY: 'sk-profile-smoke-not-a-real-key',
    },
  })
}

function installTui(home) {
  const result = runDsh(home, [
    'plugin', '--profile', 'tui', 'add', '@openma/deepseek-harness-tui',
  ])
  requireOk(result, 'install TUI')
}

function initExistingProfile(home) {
  const result = runDsh(home, ['plugin', '--profile', 'tui', 'list', '--depth', '0'])
  requireOk(result, 'initialize existing profile')
}

function installAcp(home, specifier) {
  const result = runDsh(home, [
    'plugin', '--profile', 'tui', 'add', specifier, '--ignore-scripts',
  ])
  requireOk(result, `install ACP ${specifier}`)
}

function manifest(home) {
  return JSON.parse(readFileSync(path.join(profileDir(home), 'package.json'), 'utf8'))
}

function assertSingleTuiBundle(home) {
  const bundles = manifest(home).dsh.profile.bundles
  assert.equal(
    bundles.filter((name) => name === '@openma/deepseek-harness-tui').length,
    1,
  )
}

function smokeProfile(home) {
  const result = runDsh(
    home,
    ['--profile', 'tui'],
    path.join(home, 'workspace'),
    20_000,
  )
  requireOk(result, 'boot profile and complete ACP initialize/session-new')
  assert.match(result.stdout, /"marker":"PROFILE_SMOKE_OK"/)
  assert.match(result.stdout, /"preset":"standard"/)
}

test('one TUI command handles the complete profile installation matrix', async (t) => {
  assert.ok(existsSync(smokeTui), 'profile smoke painter fixture is missing')
  assert.ok(existsSync(smokeBin), 'profile smoke painter executable is missing')
  assert.ok(existsSync(path.join(acpRoot, 'dist', 'server.js')), 'build ACP before matrix test')
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-profile-matrix-'))
  const packages = preparePackages(root)
  const selected = process.env.DSH_TUI_MATRIX_CASE
  const matrixCase = (key, name, callback) => t.test(name, {
    skip: selected !== undefined && selected !== key,
  }, callback)

  try {
    await matrixCase('missing', 'profile does not exist', () => {
      const home = path.join(root, 'missing')
      writeWorkspaceOverride(home, packages.tui)
      assert.equal(existsSync(path.join(profileDir(home), 'package.json')), false)
      installTui(home)
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('base-only', 'profile exists without ACP or TUI', () => {
      const home = path.join(root, 'base-only')
      writeWorkspaceOverride(home, packages.tui)
      initExistingProfile(home)
      installTui(home)
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('current-acp', 'profile already has current ACP', () => {
      const home = path.join(root, 'with-acp')
      writeWorkspaceOverride(home, packages.tui)
      initExistingProfile(home)
      installAcp(home, `file:${packages.acp}`)
      installTui(home)
      assertSingleTuiBundle(home)
      assert.equal(
        manifest(home).dsh.profile.bundles
          .filter((name) => name === '@openma/deepseek-harness-acp').length,
        1,
      )
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('twice', 'installing TUI twice is idempotent', () => {
      const home = path.join(root, 'twice')
      writeWorkspaceOverride(home, packages.tui)
      installTui(home)
      installTui(home)
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('old-acp', 'a pinned old ACP remains owned by the profile but does not block TUI', () => {
      const home = path.join(root, 'old-acp')
      writeWorkspaceOverride(home, packages.tui)
      initExistingProfile(home)
      installAcp(home, '@openma/deepseek-harness-acp@0.4.8')
      assert.equal(manifest(home).dependencies['@openma/deepseek-harness-acp'], '0.4.8')
      installTui(home)
      assert.equal(
        manifest(home).dependencies['@openma/deepseek-harness-acp'],
        '0.4.8',
        'installing another plugin must not rewrite a pinned direct dependency',
      )
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

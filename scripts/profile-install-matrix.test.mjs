import assert from 'node:assert/strict'
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { pathToFileURL } from 'node:url'

const repoRoot = path.resolve(import.meta.dirname, '..')
const packageRoot = path.join(repoRoot, 'npm')
const acpRoot = path.resolve(
  process.env.DSH_TUI_ACP_ROOT ?? path.resolve(repoRoot, '../openma/deepseek-harness-acp'),
)
const smokeTui = path.join(import.meta.dirname, 'profile-smoke-tui.mjs')
const smokeBin = path.resolve(process.env.MARTTY_BIN ?? process.env.DSH_TUI_BIN ?? smokeTui)
const dshCli = path.join(packageRoot, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js')

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
  const npmCli = process.env.npm_execpath
  const result = npmCli === undefined
    ? run('npm', [
      'pack', '--silent', '--ignore-scripts', '--pack-destination', destination,
    ], { cwd: directory })
    : run(process.execPath, [
      npmCli, 'pack', '--silent', '--ignore-scripts', '--pack-destination', destination,
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
  // unpublished worktrees under test. The installer reads the canonical
  // package name and bundle metadata from these exact tarballs.
  const fixture = path.join(root, 'tui-package')
  mkdirSync(fixture, { recursive: true })
  for (const name of [
    'LICENSE', 'README.md', 'bin', 'cordis.patch.yml', 'creator', 'lib', 'skills',
  ]) {
    cpSync(path.join(packageRoot, name), path.join(fixture, name), {
      recursive: true,
    })
  }
  const vendor = path.join(packageRoot, 'vendor')
  if (existsSync(vendor)) {
    cpSync(vendor, path.join(fixture, 'vendor'), { recursive: true })
  }
  const manifest = JSON.parse(readFileSync(path.join(packageRoot, 'package.json'), 'utf8'))
  manifest.dependencies = {
    ...manifest.dependencies,
    '@openma/deepseek-harness-acp': pathToFileURL(acp).href,
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

function localDependencyOverrides() {
  if (process.env.DSH_TUI_LOCAL_DEPS !== '1') return []

  const packageDirs = new Map()
  const scanNodeModules = (nodeModules) => {
    if (!existsSync(nodeModules)) return
    for (const entry of readdirSync(nodeModules, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name.startsWith('.')) continue
      const entryPath = path.join(nodeModules, entry.name)
      const candidates = entry.name.startsWith('@')
        ? readdirSync(entryPath, { withFileTypes: true })
          .filter((child) => child.isDirectory())
          .map((child) => path.join(entryPath, child.name))
        : [entryPath]
      for (const directory of candidates) {
        const packageJson = path.join(directory, 'package.json')
        if (!existsSync(packageJson)) continue
        const { name } = JSON.parse(readFileSync(packageJson, 'utf8'))
        if (typeof name === 'string' && !packageDirs.has(name)) {
          packageDirs.set(name, directory)
        }
        scanNodeModules(path.join(directory, 'node_modules'))
      }
    }
  }
  scanNodeModules(path.join(packageRoot, 'node_modules'))

  packageDirs.delete('@openma/deepseek-harness-acp')
  return [...packageDirs].map(([name, directory]) => [name, pathToFileURL(directory).href])
}

function writeWorkspaceOverride(home, packages) {
  const dir = profileDir(home)
  mkdirSync(dir, { recursive: true })
  const localDependencies = localDependencyOverrides()
  const overrides = [
    ['@openma/deepseek-harness-acp', pathToFileURL(packages.acp).href],
    ['@openma/deepseek-harness-tui', pathToFileURL(packages.tui).href],
  ]
  writeFileSync(
    path.join(dir, 'pnpm-workspace.yaml'),
    [
      'packages:',
      '  - .',
      '',
      'nodeLinker: hoisted',
      'autoInstallPeers: false',
      'ignoreWorkspaceRootCheck: true',
      'overrides:',
      ...overrides.map(([name, specifier]) => (
        `  ${JSON.stringify(name)}: ${JSON.stringify(specifier)}`
      )),
      '',
    ].join('\n'),
  )
  if (localDependencies.length > 0) {
    const localSpecifiers = Object.fromEntries([...localDependencies, ...overrides])
    writeFileSync(
      path.join(dir, '.pnpmfile.cjs'),
      [
        `const localSpecifiers = ${JSON.stringify(localSpecifiers, null, 2)}`,
        '',
        'module.exports = {',
        '  hooks: {',
        '    readPackage(pkg) {',
        "      for (const section of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {",
        '        for (const name of Object.keys(pkg[section] ?? {})) {',
        '          if (localSpecifiers[name]) pkg[section][name] = localSpecifiers[name]',
        '        }',
        '      }',
        '      return pkg',
        '    },',
        '  },',
        '}',
        '',
      ].join('\n'),
    )
  }
}

function runDsh(home, args, cwd = repoRoot, timeout = 120_000) {
  return run(process.execPath, [dshCli, ...args], {
    cwd,
    timeout,
    env: {
      DSH_HOME: home,
      DSH_TELEMETRY_DISABLED: '1',
      MARTTY_BIN: smokeBin,
      DSH_TUI_AGENT: 'dsh-tui-matrix-must-not-spawn-an-acp-process',
      DEEPSEEK_API_KEY: 'sk-profile-smoke-not-a-real-key',
      npm_config_ignore_workspace_root_check: 'true',
    },
  })
}

function installTui(home, packageTarball) {
  const result = runDsh(home, [
    'plugin', '--profile', 'tui', 'add', pathToFileURL(packageTarball).href,
  ], repoRoot, 300_000)
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

function assertPackedTuiInstalled(home) {
  const installed = path.join(
    profileDir(home), 'node_modules', '@openma', 'deepseek-harness-tui', 'lib', 'acp-host.js',
  )
  assert.equal(
    readFileSync(installed, 'utf8'),
    readFileSync(path.join(packageRoot, 'lib', 'acp-host.js'), 'utf8'),
    'profile must boot the package produced by this checkout',
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
  const matrixTmp = path.resolve(process.env.DSH_TUI_MATRIX_TMP ?? tmpdir())
  mkdirSync(matrixTmp, { recursive: true })
  const root = mkdtempSync(path.join(matrixTmp, 'dsh-tui-profile-matrix-'))
  const packages = preparePackages(root)
  const selected = process.env.DSH_TUI_MATRIX_CASE
  const matrixCase = (key, name, callback) => t.test(name, {
    skip: selected !== undefined && selected !== key,
  }, callback)

  try {
    await matrixCase('missing', 'profile does not exist', () => {
      const home = path.join(root, 'missing')
      writeWorkspaceOverride(home, packages)
      assert.equal(existsSync(path.join(profileDir(home), 'package.json')), false)
      installTui(home, packages.tui)
      assertSingleTuiBundle(home)
      assertPackedTuiInstalled(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('base-only', 'profile exists without ACP or TUI', () => {
      const home = path.join(root, 'base-only')
      writeWorkspaceOverride(home, packages)
      initExistingProfile(home)
      installTui(home, packages.tui)
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('current-acp', 'profile already has current ACP', () => {
      const home = path.join(root, 'with-acp')
      writeWorkspaceOverride(home, packages)
      initExistingProfile(home)
      installAcp(home, pathToFileURL(packages.acp).href)
      installTui(home, packages.tui)
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
      writeWorkspaceOverride(home, packages)
      installTui(home, packages.tui)
      installTui(home, packages.tui)
      assertSingleTuiBundle(home)
      mkdirSync(path.join(home, 'workspace'), { recursive: true })
      smokeProfile(home)
    })

    await matrixCase('old-acp', 'a pinned old ACP remains owned by the profile but does not block TUI', () => {
      const home = path.join(root, 'old-acp')
      writeWorkspaceOverride(home, packages)
      initExistingProfile(home)
      installAcp(home, '@openma/deepseek-harness-acp@0.4.8')
      assert.equal(manifest(home).dependencies['@openma/deepseek-harness-acp'], '0.4.8')
      installTui(home, packages.tui)
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
    if (process.env.DSH_TUI_KEEP_MATRIX === '1') {
      process.stderr.write(`kept profile matrix fixture at ${root}\n`)
    } else {
      rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 })
    }
  }
})

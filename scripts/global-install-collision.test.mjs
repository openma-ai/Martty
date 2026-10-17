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
  realpathSync,
  rmSync,
  symlinkSync,
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
// `martty-cli` — a name nothing else owns — and ships no install lifecycle
// script (pnpm-managed installs such as `dsh plugin add` refuse those). The
// first `martty-cli` launch writes the `martty` command and converges the
// prefix, so none of these can EEXIST the install.

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

// The `martty` command the first launch writes is an executable shim file
// (not an npm-managed symlink): it execs the package's bin/martty.js and
// removes itself with a reinstall hint once the package is gone.
function assertMarttyShim(dir) {
  const file = binPath(dir, 'martty')
  assert.equal(lstatSync(file).isFile(), true)
  const content = readFileSync(file, 'utf8')
  assert.match(content, /martty-command-shim/)
  assert.match(content, /node_modules.martty.bin.martty\.js/)
}

// The first real launch of the installed `martty-cli`: the converge runs
// before anything else, so it still happens although the fixture (packed
// without dependencies or a native binary) then exits 1. The source tree's
// node_modules stands in for the stripped dependencies.
function firstLaunch(dir, name = 'martty-cli') {
  const installed = path.join(dir, 'lib', 'node_modules', 'martty', 'node_modules')
  if (!existsSync(installed)) symlinkSync(path.join(packageRoot, 'node_modules'), installed)
  const result = run(binPath(dir, name), ['--version'])
  assert.match(result.stderr, /native package/, `launch reached the native lookup:\n${result.stderr}`)
  return result
}

function assertNoMarttyYet(dir) {
  assert.equal(existsSync(binPath(dir, 'martty')), false,
    'no install-time script: `martty` appears only after the first martty-cli launch')
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
// the install under test carries the shipped bin/script fields. Dependencies
// are stripped to keep the install offline and fast.
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

    requireOk(installGlobal(dir, martty, ['--foreground-scripts']), 'install martty over legacy')
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    assertNoMarttyYet(dir)

    const launch = firstLaunch(dir)
    assert.match(launch.stderr, /still installed/)
    assert.match(launch.stderr, /npm rm -g @openma\/deepseek-harness-tui/)
    assertMarttyShim(dir)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false,
      'the stale dsh-tui link should be retired by the first launch')
    assert.equal(existsSync(binPath(dir, 'dsb')), false,
      'the stale dsb link should be retired by the first launch')
  })

  await t.test('a legacy package owning martty no longer blocks the install', () => {
    const dir = prefix('legacy-martty-bin')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.20']), 'install legacy 0.2.20')

    requireOk(installGlobal(dir, martty), 'install martty over legacy 0.2.20')
    assert.match(readlinkSync(binPath(dir, 'martty')), /deepseek-harness-tui/,
      'until the first launch `martty` is still the legacy link')

    // The launch rewrites the legacy-owned `martty` link as the shim (it
    // verifiably belongs to the legacy package) and retires the rest.
    const launch = firstLaunch(dir)
    assert.match(launch.stderr, /removed the stale `dsh-tui`, `dsb` commands/)
    assert.match(launch.stderr, /still installed/)
    assertMarttyShim(dir)

    // Removing the legacy package afterwards leaves the new command intact.
    requireOk(removeGlobal(dir, LEGACY_PACKAGE), 'remove legacy package')
    assertMarttyShim(dir)
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

    requireOk(installGlobal(dir, martty), 'install martty over an npm-linked checkout')
    const launch = firstLaunch(dir)
    assertMarttyShim(dir)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false)
    assert.match(launch.stderr, /still installed/)

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
    assertMarttyShim(dir)
  })

  await t.test('an unrelated dsh-tui package no longer blocks martty', () => {
    const dir = prefix('third-party')
    requireOk(installGlobal(dir, tarballs['third-party-dsh-tui']), 'install third-party dsh-tui')

    requireOk(installGlobal(dir, martty), 'install martty over third-party dsh-tui')
    const launch = firstLaunch(dir)
    assert.doesNotMatch(launch.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    const target = readlinkSync(binPath(dir, 'dsh-tui'))
    assert.match(target, /node_modules\/dsh-tui\//,
      'the third-party dsh-tui link is left untouched')
    assertMarttyShim(dir)
  })

  await t.test('a fresh install links martty-cli; its first launch adds martty', () => {
    const dir = prefix('fresh')
    const result = installGlobal(dir, martty, ['--foreground-scripts'])
    requireOk(result, 'install martty')
    assert.doesNotMatch(result.stderr, /install-migration|postinstall/,
      'the package runs nothing at install time')
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    // The documented trade-off of shipping no lifecycle script.
    assertNoMarttyYet(dir)

    const launch = firstLaunch(dir)
    assert.doesNotMatch(launch.stderr, new RegExp(LEGACY_PACKAGE.replace('/', '\\/')))
    assertMarttyShim(dir)
    // From now on `martty` itself runs the same package binary.
    const viaShim = firstLaunch(dir, 'martty')
    assert.doesNotMatch(viaShim.stderr, /not installed/)
  })

  await t.test('upgrading over a shim-carrying install keeps martty working', () => {
    // martty <= 0.3.2 wrote the shim from its postinstall: a regular file
    // npm does not manage, so an upgrade neither collides with nor removes it.
    const dir = prefix('shim-upgrade')
    requireOk(installGlobal(dir, martty), 'install martty')
    firstLaunch(dir)
    assertMarttyShim(dir)
    requireOk(installGlobal(dir, martty), 'reinstall (upgrade) martty — no EEXIST on the shim')
    assertMarttyShim(dir)
    const viaShim = firstLaunch(dir, 'martty')
    assert.doesNotMatch(viaShim.stderr, /not installed/)
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
    firstLaunch(dir)
    assertMarttyShim(dir)
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
  })

  await t.test('removing martty leaves a self-cleaning shim, not a dead link', () => {
    const dir = prefix('removal')
    requireOk(installGlobal(dir, martty), 'install martty')
    firstLaunch(dir)
    requireOk(removeGlobal(dir, 'martty'), 'remove martty')

    assert.equal(existsSync(binPath(dir, 'martty-cli')), false,
      'npm removes the declared martty-cli link itself')
    // npm never runs uninstall hooks, so the launch-created `martty`
    // command stays behind — but it is a shim that notices the package is
    // gone: the next invocation prints the reinstall hint and deletes
    // itself instead of leaving a dead symlink.
    const shim = binPath(dir, 'martty')
    assert.equal(lstatSync(shim).isFile(), true)
    const gone = run(shim, [])
    assert.equal(gone.status, 127, gone.stderr)
    assert.match(gone.stderr, /not installed/)
    assert.equal(existsSync(shim), false, 'the shim removed itself')

    requireOk(installGlobal(dir, martty), 'reinstall martty')
    firstLaunch(dir)
    assertMarttyShim(dir)
  })

  await t.test('--ignore-scripts converges on the first martty-cli launch', () => {
    const dir = prefix('ignore-scripts')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.20']), 'install legacy 0.2.20')

    requireOk(
      installGlobal(dir, martty, ['--ignore-scripts']),
      'install martty with scripts skipped',
    )
    // Same result as a default install: martty-cli is linked, everything
    // else is untouched until the first launch.
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    assert.equal(existsSync(binPath(dir, 'martty')), true, 'legacy martty still there')
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), true, 'legacy dsh-tui still there')

    // The first real launch (what `martty-cli` runs before the TUI boots)
    // converges the prefix.
    const launch = firstLaunch(dir)
    assert.match(launch.stderr, /still installed/)

    assertMarttyShim(dir)
    assert.equal(existsSync(binPath(dir, 'dsh-tui')), false, 'stale dsh-tui retired at launch')
    assert.equal(existsSync(binPath(dir, 'dsb')), false, 'stale dsb retired at launch')
    const legacyManifest = JSON.parse(readFileSync(
      path.join(dir, 'lib', 'node_modules', '@openma', 'deepseek-harness-tui', 'package.json'),
      'utf8',
    ))
    assert.equal('bin' in legacyManifest, false, 'legacy manifest sanitized at launch')

    requireOk(removeGlobal(dir, LEGACY_PACKAGE), 'remove legacy package')
    assertMarttyShim(dir)
  })

  await t.test('an interactive TTY launch defers the notice to exit', (t) => {
    // The TUI repaints the whole terminal for the session, so on a TTY the
    // migration notice is held until process exit — otherwise it sits
    // behind the first frame the entire time. `script(1)` allocates a real
    // pty for the child; without one (piped stdio) it prints immediately.
    if (run('script', ['-qec', 'true', '/dev/null']).status !== 0) {
      t.skip('script(1) with -qec is unavailable for a pty run')
      return
    }
    const dir = prefix('tty-defer')
    requireOk(installGlobal(dir, tarballs['legacy-0.2.20']), 'install legacy 0.2.20')
    requireOk(installGlobal(dir, martty), 'install martty over legacy')
    // The fixture tarball strips dependencies for an offline install; give
    // the installed bin the source tree's node_modules so the real
    // bin/martty.js loads. It still exits 1 — the fixture ships no native
    // binary — which makes the notice's ORDER observable: deferred output
    // lands after the failure line, immediate output before it.
    symlinkSync(
      path.join(packageRoot, 'node_modules'),
      path.join(dir, 'lib', 'node_modules', 'martty', 'node_modules'),
    )
    const tty = run('script', ['-qec', `"${binPath(dir, 'martty-cli')}" --version`, '/dev/null'])
    assert.equal(tty.status, 1, `pty launch should exit 1 (no vendored binary):\n${tty.stdout}`)
    const missing = tty.stdout.indexOf('native package')
    const notice = tty.stdout.indexOf('still installed')
    assert.ok(missing >= 0 && notice > missing,
      `the TTY launch defers the notice past the failure output:\n${tty.stdout}`)

    const piped = run(binPath(dir, 'martty-cli'), ['--version'])
    assert.ok(
      piped.stderr.indexOf('still installed') >= 0
        && piped.stderr.indexOf('still installed') < piped.stderr.indexOf('native package'),
      `piped stderr still gets the notice first:\n${piped.stderr}`,
    )
  })

  // `npm link` of a modern checkout: the source package is named `martty`,
  // so a linked checkout and the published package are the SAME package to
  // npm — each replaces the other instead of colliding on bin names.
  const linkCheckout = path.join(root, 'source-checkout')
  cpSync(packageRoot, linkCheckout, {
    recursive: true,
    filter: (src) => !['node_modules', 'package-lock.json'].includes(path.basename(src)),
  })

  await t.test('npm link of a source checkout, then install', () => {
    const dir = prefix('link-then-install')
    requireOk(
      npm(['link', '--ignore-scripts'], {
        cwd: linkCheckout,
        env: { npm_config_prefix: dir },
      }),
      'npm link source checkout',
    )
    assert.equal(
      lstatSync(path.join(dir, 'lib', 'node_modules', 'martty')).isSymbolicLink(),
      true,
      'the checkout is linked into the global root',
    )

    requireOk(installGlobal(dir, martty), 'install martty over the link')
    assert.equal(
      lstatSync(path.join(dir, 'lib', 'node_modules', 'martty')).isSymbolicLink(),
      false,
      'the published package replaces the link — no EEXIST',
    )
    firstLaunch(dir)
    assertMarttyShim(dir)
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    assert.doesNotMatch(realpathSync(binPath(dir, 'martty-cli')), /source-checkout/,
      'the published bin is the real package, not the checkout')
  })

  await t.test('install, then npm link of a source checkout', () => {
    const dir = prefix('install-then-link')
    requireOk(installGlobal(dir, martty), 'install martty first')
    assert.equal(
      lstatSync(path.join(dir, 'lib', 'node_modules', 'martty')).isSymbolicLink(),
      false,
    )

    requireOk(
      npm(['link', '--ignore-scripts'], {
        cwd: linkCheckout,
        env: { npm_config_prefix: dir },
      }),
      'npm link source checkout over the install',
    )
    assert.equal(
      lstatSync(path.join(dir, 'lib', 'node_modules', 'martty')).isSymbolicLink(),
      true,
      'the link replaces the install — no EEXIST',
    )
    assert.match(readlinkSync(binPath(dir, 'martty-cli')), /node_modules\/martty\//)
    assert.match(realpathSync(binPath(dir, 'martty-cli')), /source-checkout/,
      'the relative bin link now resolves into the linked checkout')
  })
})

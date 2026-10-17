#!/usr/bin/env node
/**
 * End-to-end check of the dsh plugin install path against the GLOBAL dsh on
 * PATH (CI installs `@deepseek-ai/dsh@<dist-tag>` globally per matrix leg).
 *
 * 1. Pack this checkout as the published `martty` package.
 * 2. `dsh plugin --profile martty add <tarball>` — dsh's own plugin manager,
 *    i.e. pnpm, with strict dependency build scripts. Asserts it succeeds,
 *    reports no ignored builds, and records `martty` in the profile bundles.
 * 3. `dsh --profile martty` headless with the smoke painter: every entry
 *    activates (no "did not activate") and ACP initialize + session/new
 *    completes against the profile's Host.
 * 4. `martty --dsh martty` from a global npm install of the same tarball
 *    hands off to that profile.
 * 5. `martty --dsh martty` on a dsh below the floor falls back to the bundled
 *    harness and says why.
 * 6. A profile pinned to an older dsh-acp whose exact-version list excludes
 *    this dsh shows the in-TUI explanation instead of pending entries; on a
 *    dsh that list still covers it boots normally.
 *
 * Usage: node scripts/dsh-plugin-install.mjs [--keep]
 * Env:   MARTTY_ACP_TARBALL  override @openma/deepseek-harness-acp (local runs)
 *        DSH_PLUGIN_CI_TMP   scratch root (default: os.tmpdir())
 */

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const repoRoot = path.resolve(import.meta.dirname, '..')
const smokeTui = path.join(import.meta.dirname, 'profile-smoke-tui.mjs')
const keep = process.argv.includes('--keep')
const root = mkdtempSync(path.join(process.env.DSH_PLUGIN_CI_TMP ?? tmpdir(), 'martty-dsh-plugin-'))
const home = path.join(root, 'dsh-home')
const workspace = path.join(root, 'workspace')
mkdirSync(workspace, { recursive: true })

const baseEnv = {
  ...process.env,
  DSH_HOME: home,
  MARTTY_HOME: path.join(root, 'martty-home'),
  DSH_TELEMETRY_DISABLED: '1',
  MARTTY_BIN: smokeTui,
  DEEPSEEK_API_KEY: 'sk-dsh-plugin-ci-not-a-real-key',
  // pnpm treats any dependency build script as an error under this flag —
  // the strictest setting dsh's plugin manager can meet.
  npm_config_strict_dep_builds: 'true',
}
for (const name of ['DSH_PATH', 'DSH_ACP_HOST', 'DSH_TUI_AGENT', 'DSH_TUI_BIN']) delete baseEnv[name]

function run(label, command, args, { env = {}, cwd = workspace, timeout = 300_000 } = {}) {
  const result = spawnSync(command, args, {
    cwd, env: { ...baseEnv, ...env }, encoding: 'utf8', timeout, shell: process.platform === 'win32',
  })
  const out = `${result.stdout ?? ''}${result.stderr ?? ''}`
  console.log(`::group::${label}\n$ ${command} ${args.join(' ')}\n${out}\nexit=${result.status}${result.signal ? ` signal=${result.signal}` : ''}\n::endgroup::`)
  return { ...result, out }
}

function step(name) {
  console.log(`\n== ${name}`)
}

const assertActivated = (result, label) => {
  assert.doesNotMatch(result.out, /did not activate/, `${label}: an entry did not activate`)
  assert.doesNotMatch(result.out, /startup failed/, `${label}: startup failed`)
}

try {
  const dshVersion = run('dsh --version', 'dsh', ['--version']).stdout.trim().split(/\s+/).at(-1)
  assert.match(dshVersion ?? '', /^\d+\.\d+\.\d+/, 'a global dsh must be on PATH')
  const pnpmVersion = run('pnpm --version', 'pnpm', ['--version']).stdout.trim()
  console.log(`global dsh ${dshVersion}, pnpm ${pnpmVersion}`)

  step('pack the published package from this checkout')
  const stage = path.join(root, 'npm-martty')
  const alias = run('package-alias', process.execPath, [path.join(import.meta.dirname, 'package-alias.mjs'), path.join(repoRoot, 'npm'), stage, 'martty'], { cwd: repoRoot })
  assert.equal(alias.status, 0, 'package-alias failed')
  if (process.env.MARTTY_ACP_TARBALL) {
    const manifestPath = path.join(stage, 'package.json')
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
    manifest.dependencies['@openma/deepseek-harness-acp'] = pathToFileURL(path.resolve(process.env.MARTTY_ACP_TARBALL)).href
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  }
  const packs = path.join(root, 'packs')
  mkdirSync(packs)
  const packed = run('npm pack', 'npm', ['pack', stage, '--silent', '--pack-destination', packs], { cwd: repoRoot })
  assert.equal(packed.status, 0, 'npm pack failed')
  const tarball = path.join(packs, packed.stdout.trim().split(/\r?\n/).at(-1))
  const { scripts = {} } = JSON.parse(readFileSync(path.join(stage, 'package.json'), 'utf8'))
  for (const hook of ['preinstall', 'install', 'postinstall']) assert.equal(scripts[hook], undefined, `package declares ${hook}`)

  step('dsh plugin --profile martty add <tarball> (pnpm, strict build scripts)')
  const added = run('dsh plugin add', 'dsh', ['plugin', '--profile', 'martty', 'add', pathToFileURL(tarball).href])
  assert.equal(added.status, 0, 'dsh plugin add failed')
  assert.doesNotMatch(added.out, /IGNORED_BUILDS|Ignored build scripts/)
  const profile = JSON.parse(readFileSync(path.join(home, 'profiles', 'martty', 'package.json'), 'utf8'))
  assert.ok(profile.dsh?.profile?.bundles?.includes('martty'), 'martty is not a bundle of the profile')

  step('dsh --profile martty: every entry activates, ACP round trip completes')
  const booted = run('dsh --profile martty', 'dsh', ['--profile', 'martty'], { timeout: 90_000 })
  assertActivated(booted, 'dsh --profile martty')
  assert.equal(booted.status, 0, 'profile boot failed')
  assert.match(booted.stdout, /"marker":"PROFILE_SMOKE_OK"/)

  step('global npm install of the same tarball, then martty --dsh martty')
  const prefix = path.join(root, 'npm-global')
  const installed = run('npm install -g', 'npm', ['install', '-g', '--prefix', prefix, '--no-audit', '--no-fund', tarball])
  assert.equal(installed.status, 0, 'npm install -g failed')
  const bin = process.platform === 'win32' ? prefix : path.join(prefix, 'bin')
  const cli = path.join(bin, process.platform === 'win32' ? 'martty-cli.cmd' : 'martty-cli')
  const attached = run('martty --dsh martty', cli, ['--dsh', 'martty'], { timeout: 90_000 })
  assertActivated(attached, 'martty --dsh martty')
  assert.equal(attached.status, 0, '--dsh profile hand-off failed')
  assert.match(attached.stdout, /"marker":"PROFILE_SMOKE_OK"/)

  step('martty --dsh martty on a dsh below the floor falls back to the bundled harness')
  const fakeBin = path.join(root, 'old-dsh-bin')
  mkdirSync(fakeBin)
  const fake = path.join(fakeBin, 'dsh')
  writeFileSync(fake, '#!/bin/sh\necho "dsh 0.1.0"\n')
  chmodSync(fake, 0o755)
  writeFileSync(`${fake}.cmd`, '@echo dsh 0.1.0\r\n')
  const PATH = `${fakeBin}${path.delimiter}${process.env.PATH}`
  const fellBack = run('martty --dsh martty (dsh 0.1.0)', cli, ['--dsh', 'martty'], { env: { PATH }, timeout: 90_000 })
  assert.match(fellBack.out, /global dsh 0\.1\.0 is not supported .*bundled harness/)
  assert.equal(fellBack.status, 0, 'bundled fallback failed')
  assert.match(fellBack.stdout, /"marker":"PROFILE_SMOKE_OK"/)

  step('a profile pinned to dsh-acp 0.4.38 (exact-version list)')
  const pinnedMartty = run('dsh plugin add martty (pinned)', 'dsh', ['plugin', '--profile', 'pinned', 'add', pathToFileURL(tarball).href])
  assert.equal(pinnedMartty.status, 0, 'pinned profile: martty install failed')
  const legacyList = ['0.1.7-rc.2', '0.2.0-rc.1', '0.2.0-rc.2']
  if (!legacyList.includes(dshVersion)) {
    // dsh refuses an incompatible plugin unless the user accepts the risk;
    // this is the state a user who did so (or upgraded dsh) ends up in.
    const allowed = run('dsh plugin allow-version (pinned)', 'dsh', ['plugin', '--profile', 'pinned', 'allow-version', '@openma/deepseek-harness-acp@0.4.38', '--dsh-version', dshVersion, '--accept-risk'])
    assert.equal(allowed.status, 0, 'pinned profile: allow-version failed')
  }
  const pinnedAcp = run('dsh plugin add dsh-acp 0.4.38 (pinned)', 'dsh', ['plugin', '--profile', 'pinned', 'add', '@openma/deepseek-harness-acp@0.4.38'])
  assert.equal(pinnedAcp.status, 0, 'pinned profile: dsh-acp 0.4.38 install failed')
  const pinned = run('dsh --profile pinned', 'dsh', ['--profile', 'pinned'], { timeout: 90_000 })
  assertActivated(pinned, 'dsh --profile pinned')
  if (legacyList.includes(dshVersion)) {
    assert.match(pinned.stdout, /"marker":"PROFILE_SMOKE_OK"/)
  } else {
    assert.match(pinned.stdout, /"marker":"PROFILE_SMOKE_NOTICE"/)
    assert.match(pinned.stdout, new RegExp(`Installed dsh: \`${dshVersion.replace(/\./g, '\\.')}\``))
    assert.match(pinned.stdout, /npx martty/)
  }

  console.log(`\nDSH_PLUGIN_CI_OK dsh=${dshVersion} pnpm=${pnpmVersion}`)
} finally {
  if (keep) console.log(`kept ${root}`)
  else rmSync(root, { recursive: true, force: true })
}

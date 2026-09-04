#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { resolveDependencyStack } from '../lib/agent.js'
import { bootClient, parseClientArgv, painterArgs, uiSettingsPath } from '../lib/boot.js'
import { runHarnessCommand } from '../lib/harnesses.js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const platformKey = process.platform + '-' + process.arch
const vendorDir = path.join(__dirname, '..', 'vendor')
const binaryName = process.platform === 'win32' ? 'martty.exe' : 'martty'
const packaged = path.join(vendorDir, platformKey, binaryName)
const configuredBin = process.env.MARTTY_BIN || process.env.DSH_TUI_BIN
let binaryPath = packaged
if (typeof configuredBin === 'string' && configuredBin.length > 0) {
  if (fs.existsSync(configuredBin)) {
    binaryPath = configuredBin
  } else {
    // Never silently fall back: the user believes they are running their
    // own build (devlocalinstall.sh has caught this confusion before).
    console.error(
      `martty: ${configuredBin} (from ${process.env.MARTTY_BIN ? 'MARTTY_BIN' : 'DSH_TUI_BIN'})`
        + ' does not exist — falling back to the bundled binary',
    )
  }
}

const argv = process.argv.slice(2)
const wantDemoSkin = argv.includes('--demo-skin')
const wantDemo = argv.includes('--demo')
const wantHelp = argv.includes('-h') || argv.includes('--help')
const wantVersion = argv.includes('-V') || argv.includes('--version')

if (argv[0] === 'harness') {
  let defaults = []
  try {
    const agent = resolveDependencyStack()
    defaults = [{
      id: 'builtin-dsh',
      label: 'Bundled DeepSeek Harness',
      ...agent,
      source: 'builtin',
    }]
  } catch {
    // Source checkouts without installed dependencies still list PATH entries.
  }
  try {
    const result = runHarnessCommand(argv.slice(1), {
      settingsPath: uiSettingsPath(),
      defaults,
    })
    if (result.stdout) process.stdout.write(result.stdout)
    if (result.stderr) process.stderr.write(result.stderr)
    process.exit(result.code)
  } catch (error) {
    console.error(`martty harness: ${error instanceof Error ? error.message : String(error)}`)
    process.exit(1)
  }
}

if (!fs.existsSync(binaryPath)) {
  let available = []
  try {
    available = fs
      .readdirSync(vendorDir, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name)
  } catch {
    // vendor dir missing entirely
  }
  console.error('martty: no bundled binary for this platform (' + platformKey + ').')
  if (available.length > 0) {
    console.error('Bundled platforms: ' + available.join(', '))
  } else {
    console.error('No platform binaries are bundled in this install.')
  }
  console.error('Hint: run scripts/build-npm.sh on this machine to build and package a native binary.')
  process.exit(1)
}

function exitFromSpawn(result) {
  if (result.error) {
    // A failed spawn (ENOENT, EACCES) sets status/signal to null; without
    // a diagnostic the process would just exit 1 with no explanation.
    console.error('martty: failed to launch the native binary: ' + result.error.message)
  }
  process.exit(
    result.status
      ?? (result.signal === 'SIGINT' ? 130 : result.signal === 'SIGTERM' ? 143 : 1),
  )
}

if (wantHelp || wantVersion || wantDemo) {
  exitFromSpawn(spawnSync(binaryPath, argv, { stdio: 'inherit' }))
}

if (wantDemoSkin) {
  process.env.MARTTY_BIN ??= binaryPath
  const demoSkinPath = path.join(__dirname, '..', 'lib', 'demo-skin.js')
  const args = argv.filter((arg) => arg !== '--demo-skin')
  exitFromSpawn(spawnSync(process.execPath, [demoSkinPath, ...args], {
    stdio: 'inherit',
    env: process.env,
  }))
}

process.env.MARTTY_BIN ??= binaryPath
const parsed = parseClientArgv(argv)
await bootClient({ agent: parsed.agent, extraArgs: painterArgs(parsed) })

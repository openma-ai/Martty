#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { bootClient, parseClientArgv, painterArgs } from '../lib/boot.js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const platformKey = process.platform + '-' + process.arch
const vendorDir = path.join(__dirname, '..', 'vendor')
const binaryName = process.platform === 'win32' ? 'dsh-tui.exe' : 'dsh-tui'
const packaged = path.join(vendorDir, platformKey, binaryName)
const envBin = process.env.DSH_TUI_BIN
const binaryPath = typeof envBin === 'string' && envBin.length > 0 && fs.existsSync(envBin)
  ? envBin
  : packaged

const argv = process.argv.slice(2)
const wantDemoSkin = argv.includes('--demo-skin')
const wantDemo = argv.includes('--demo')
const wantHelp = argv.includes('-h') || argv.includes('--help')
const wantVersion = argv.includes('-V') || argv.includes('--version')

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
  console.error('dsh-tui: no bundled binary for this platform (' + platformKey + ').')
  if (available.length > 0) {
    console.error('Bundled platforms: ' + available.join(', '))
  } else {
    console.error('No platform binaries are bundled in this install.')
  }
  console.error('Hint: run scripts/build-npm.sh on this machine to build and package a native binary.')
  process.exit(1)
}

function exitFromSpawn(result) {
  process.exit(
    result.status ??
      (result.signal === 'SIGINT' ? 130 : result.signal === 'SIGTERM' ? 143 : 1),
  )
}

if (wantHelp || wantVersion || wantDemo) {
  exitFromSpawn(spawnSync(binaryPath, argv, { stdio: 'inherit' }))
}

if (wantDemoSkin) {
  process.env.DSH_TUI_BIN ??= binaryPath
  const demoSkinPath = path.join(__dirname, '..', 'lib', 'demo-skin.js')
  const args = argv.filter((arg) => arg !== '--demo-skin')
  exitFromSpawn(spawnSync(process.execPath, [demoSkinPath, ...args], {
    stdio: 'inherit',
    env: process.env,
  }))
}

process.env.DSH_TUI_BIN ??= binaryPath
const parsed = parseClientArgv(argv)
await bootClient({ agent: parsed.agent, extraArgs: painterArgs(parsed) })

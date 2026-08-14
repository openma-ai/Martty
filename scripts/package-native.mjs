#!/usr/bin/env node

import { chmodSync, copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs'
import path from 'node:path'

const supported = [
  { platform: 'darwin', arch: 'arm64', file: 'dsh-tui' },
  { platform: 'darwin', arch: 'x64', file: 'dsh-tui' },
  { platform: 'linux', arch: 'x64', file: 'dsh-tui' },
  { platform: 'win32', arch: 'x64', file: 'dsh-tui.exe' },
]

function options(args) {
  const out = new Map()
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]
    const value = args[i + 1]
    if (!key?.startsWith('--') || value === undefined) {
      throw new Error(`invalid option list near ${key ?? '<end>'}`)
    }
    out.set(key.slice(2), value)
  }
  return out
}

function required(values, name) {
  const value = values.get(name)
  if (!value) throw new Error(`--${name} is required`)
  return value
}

function platformSpec(platform, arch) {
  const spec = supported.find((item) => item.platform === platform && item.arch === arch)
  if (!spec) throw new Error(`unsupported native target: ${platform}-${arch}`)
  return spec
}

function stage(values) {
  const source = path.resolve(required(values, 'source'))
  const platform = required(values, 'platform')
  const arch = required(values, 'arch')
  const vendorRoot = path.resolve(required(values, 'vendor-root'))
  const spec = platformSpec(platform, arch)
  if (!existsSync(source) || !statSync(source).isFile()) {
    throw new Error(`native binary not found: ${source}`)
  }

  const destinationDir = path.join(vendorRoot, `${platform}-${arch}`)
  const destination = path.join(destinationDir, spec.file)
  mkdirSync(destinationDir, { recursive: true })
  copyFileSync(source, destination)
  if (platform !== 'win32') chmodSync(destination, 0o755)
  process.stdout.write(`${destination}\n`)
}

function verify(values) {
  const vendorRoot = path.resolve(required(values, 'vendor-root'))
  const missing = supported
    .map((spec) => path.join(vendorRoot, `${spec.platform}-${spec.arch}`, spec.file))
    .filter((file) => !existsSync(file) || !statSync(file).isFile() || statSync(file).size === 0)
  if (missing.length > 0) {
    throw new Error(`release package is missing native binaries:\n${missing.join('\n')}`)
  }
  process.stdout.write('native package set complete\n')
}

const [command, ...args] = process.argv.slice(2)
const values = options(args)

try {
  if (command === 'stage') stage(values)
  else if (command === 'verify') verify(values)
  else throw new Error('usage: package-native.mjs <stage|verify> [options]')
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

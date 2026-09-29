#!/usr/bin/env node

import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import { nativeTargets } from './native-targets.mjs'

const licenseFile = path.resolve(import.meta.dirname, '..', 'LICENSE')

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
  const spec = nativeTargets.find((item) => item.platform === platform && item.arch === arch)
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
  const temporary = `${destination}.tmp-${process.pid}`
  mkdirSync(destinationDir, { recursive: true })
  try {
    copyFileSync(source, temporary)
    if (platform !== 'win32') chmodSync(temporary, 0o755)
    // Never overwrite a Mach-O that may already have been executed. macOS
    // caches its code signature by vnode and can SIGKILL a binary whose
    // contents changed in place. rename swaps in a fresh inode atomically.
    renameSync(temporary, destination)
  } finally {
    rmSync(temporary, { force: true })
  }
  process.stdout.write(`${destination}\n`)
}

function verify(values) {
  const vendorRoot = path.resolve(required(values, 'vendor-root'))
  const missing = nativeTargets
    .map((spec) => path.join(vendorRoot, `${spec.platform}-${spec.arch}`, spec.file))
    .filter((file) => !existsSync(file) || !statSync(file).isFile() || statSync(file).size === 0)
  if (missing.length > 0) {
    throw new Error(`release package is missing native binaries:\n${missing.join('\n')}`)
  }
  process.stdout.write('native package set complete\n')
}

function packagePlatforms(values) {
  const vendorRoot = path.resolve(required(values, 'vendor-root'))
  const packageRoot = path.resolve(required(values, 'package-root'))
  const version = required(values, 'version')
  verify(values)
  rmSync(packageRoot, { recursive: true, force: true })
  for (const spec of nativeTargets) {
    const key = `${spec.platform}-${spec.arch}`
    const directory = path.join(packageRoot, key)
    const bin = path.join(directory, 'bin', spec.file)
    mkdirSync(path.dirname(bin), { recursive: true })
    copyFileSync(path.join(vendorRoot, key, spec.file), bin)
    copyFileSync(licenseFile, path.join(directory, 'LICENSE'))
    if (spec.platform !== 'win32') chmodSync(bin, 0o755)
    writeFileSync(path.join(directory, 'package.json'), `${JSON.stringify({
      name: `@openma/martty-${key}`,
      version,
      description: `Martty native binary for ${key}`,
      license: 'MIT',
      repository: { type: 'git', url: 'git+https://github.com/openma-ai/Martty.git' },
      os: [spec.platform],
      cpu: [spec.arch],
      engines: { node: '>=22.19.0' },
      files: ['bin', 'LICENSE'],
      publishConfig: { access: 'public' },
    }, null, 2)}\n`)
  }
  process.stdout.write('platform packages complete\n')
}

const [command, ...args] = process.argv.slice(2)
const values = options(args)

try {
  if (command === 'stage') stage(values)
  else if (command === 'verify') verify(values)
  else if (command === 'package') packagePlatforms(values)
  else throw new Error('usage: package-native.mjs <stage|verify|package> [options]')
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

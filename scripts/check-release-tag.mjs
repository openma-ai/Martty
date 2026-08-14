#!/usr/bin/env node

import { readFileSync } from 'node:fs'

const [tag, packagePath, cargoPath] = process.argv.slice(2)

function cargoPackageVersion(toml) {
  let inPackage = false
  for (const line of toml.split(/\r?\n/)) {
    const section = line.trim().match(/^\[([^\]]+)\]$/)
    if (section) {
      inPackage = section[1] === 'package'
      continue
    }
    if (!inPackage) continue
    const version = line.match(/^\s*version\s*=\s*["']([^"']+)["']/)
    if (version) return version[1]
  }
  throw new Error(`Cargo package version not found in ${cargoPath}`)
}

try {
  if (!tag || !packagePath || !cargoPath) {
    throw new Error('usage: check-release-tag.mjs <tag> <package.json> <Cargo.toml>')
  }
  const packageJson = JSON.parse(readFileSync(packagePath, 'utf8'))
  if (tag !== `v${packageJson.version}`) {
    throw new Error(`tag ${tag} does not match npm version ${packageJson.version}`)
  }
  const cargoVersion = cargoPackageVersion(readFileSync(cargoPath, 'utf8'))
  if (cargoVersion !== packageJson.version) {
    throw new Error(
      `Cargo version ${cargoVersion} does not match npm version ${packageJson.version}`,
    )
  }
  process.stdout.write(`release version ${packageJson.version}\n`)
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

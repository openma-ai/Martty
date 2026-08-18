#!/usr/bin/env node

// Bump both package versions, commit, and tag a release.
//
// Usage: node scripts/release.mjs <version> [--dry-run]
//
// The version may be "0.2.8" or "v0.2.8"; the tag is always v<version>.
// Requires a clean git working tree and refuses to run on a non-main
// branch or when a tag for the version already exists.

import { execFileSync, spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const moduleDir = path.dirname(fileURLToPath(import.meta.url))
// Overridable so tests can run against a throwaway git repo.
const repoRoot = process.env.DSH_TUI_RELEASE_ROOT
  ? path.resolve(process.env.DSH_TUI_RELEASE_ROOT)
  : path.resolve(moduleDir, '..')
const npmPackage = path.join(repoRoot, 'npm', 'package.json')
const cargoToml = path.join(repoRoot, 'Cargo.toml')
const verifyScript = path.join(moduleDir, 'check-release-tag.mjs')

function git(args) {
  const result = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8' })
  if (result.status !== 0) {
    throw new Error(`git ${args.join(' ')} failed: ${result.stderr.trim()}`)
  }
  return result.stdout.trim()
}

function bumpCargo(toml, version) {
  return toml.replace(/^(\[package\][\s\S]*?^version\s*=\s*")[^"]+(")/m, `$1${version}$2`)
}

function fail(message) {
  process.stderr.write(`${message}\n`)
  process.exitCode = 1
}

try {
  const rawVersion = process.argv[2]
  const dryRun = process.argv.includes('--dry-run')
  if (!rawVersion) throw new Error('usage: release.mjs <version> [--dry-run]')

  const match = rawVersion.match(/^v?(\d+\.\d+\.\d+)$/)
  if (!match) throw new Error(`invalid version: ${rawVersion} (expected X.Y.Z)`)
  const version = match[1]
  const tag = `v${version}`

  const branch = git(['branch', '--show-current'])
  if (branch !== 'main') throw new Error(`refusing to release from branch "${branch}" (expected main)`)

  const status = git(['status', '--porcelain'])
  if (status) throw new Error(`working tree is not clean:\n${status}`)

  const existing = git(['tag', '--list', tag])
  if (existing) throw new Error(`tag ${tag} already exists`)

  const packageJson = JSON.parse(readFileSync(npmPackage, 'utf8'))
  if (packageJson.version === version) {
    throw new Error(`npm/package.json is already at ${version}`)
  }

  const cargoBefore = readFileSync(cargoToml, 'utf8')
  const cargoAfter = bumpCargo(cargoBefore, version)
  if (cargoAfter === cargoBefore) {
    throw new Error(`could not locate the version line in Cargo.toml`)
  }

  if (dryRun) {
    process.stdout.write(`[dry-run] would bump npm/package.json + Cargo.toml to ${version} and create tag ${tag}\n`)
    process.exit(0)
  }

  packageJson.version = version
  writeFileSync(npmPackage, `${JSON.stringify(packageJson, null, 2)}\n`)
  writeFileSync(cargoToml, cargoAfter)

  const verify = spawnSync(process.execPath, [
    verifyScript,
    tag,
    npmPackage,
    cargoToml,
  ], { cwd: repoRoot, encoding: 'utf8' })
  if (verify.status !== 0) {
    throw new Error(`self-check failed: ${verify.stderr.trim()}`)
  }

  git(['add', path.relative(repoRoot, npmPackage), path.relative(repoRoot, cargoToml)])
  git(['commit', '-m', `release: ${tag}`])
  git(['tag', tag])

  process.stdout.write(`release ${tag} created.\n`)
  process.stdout.write(`push with: git push origin main --tags\n`)
} catch (error) {
  fail(error.message)
}

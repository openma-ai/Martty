#!/usr/bin/env node

// Releases go through an issue and a release pull request; nothing is pushed
// to main directly (see RELEASING.md).
//
// Usage:
//   node scripts/release.mjs <version> [--dry-run]
//     From a clean, up-to-date main: create branch release/v<version>, bump
//     npm/package.json, npm/package-lock.json, Cargo.toml and Cargo.lock, and
//     commit "release: v<version>" there. Push the branch and open the
//     release PR; nothing is tagged.
//   node scripts/release.mjs <version> --tag
//     After the release PR is merged, from a clean main whose versions equal
//     <version>: create tag v<version> on HEAD. Pushing the tag publishes.
//
// The version may be "0.2.8" or "v0.2.8"; the tag is always v<version>.

import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const moduleDir = path.dirname(fileURLToPath(import.meta.url))
// Overridable so tests can run against a throwaway git repo.
const repoRoot = process.env.DSH_TUI_RELEASE_ROOT
  ? path.resolve(process.env.DSH_TUI_RELEASE_ROOT)
  : path.resolve(moduleDir, '..')
const npmPackage = path.join(repoRoot, 'npm', 'package.json')
const npmPackageLock = path.join(repoRoot, 'npm', 'package-lock.json')
const cargoToml = path.join(repoRoot, 'Cargo.toml')
const cargoLock = path.join(repoRoot, 'Cargo.lock')
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

function cargoPackageName(toml) {
  const match = toml.match(/^\[package\][\s\S]*?^name\s*=\s*"([^"]+)"/m)
  if (!match) throw new Error('could not locate the package name in Cargo.toml')
  return match[1]
}

function bumpCargoLock(lock, packageName, version) {
  let found = false
  const updated = lock.replace(
    /^\[\[package\]\]\r?\n[\s\S]*?(?=^\[\[package\]\]\r?$|(?![\s\S]))/gm,
    (block) => {
      const name = block.match(/^name\s*=\s*"([^"]+)"$/m)?.[1]
      if (name !== packageName) return block
      found = true
      return block.replace(/^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`)
    },
  )
  if (!found) throw new Error(`could not locate ${packageName} in Cargo.lock`)
  return updated
}

function fail(message) {
  process.stderr.write(`${message}\n`)
  process.exitCode = 1
}

try {
  const rawVersion = process.argv[2]
  const dryRun = process.argv.includes('--dry-run')
  if (!rawVersion) throw new Error('usage: release.mjs <version> [--dry-run]')

  const match = rawVersion.match(/^v?(\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?)$/)
  if (!match) throw new Error(`invalid version: ${rawVersion} (expected X.Y.Z or X.Y.Z-{alpha,beta,rc}.N)`)
  const version = match[1]
  const tag = `v${version}`

  const tagMode = process.argv.includes('--tag')
  const branch = git(['branch', '--show-current'])
  if (branch !== 'main') throw new Error(`refusing to release from branch "${branch}" (expected main)`)

  const status = git(['status', '--porcelain'])
  if (status) throw new Error(`working tree is not clean:\n${status}`)

  const existing = git(['tag', '--list', tag])
  if (existing) throw new Error(`tag ${tag} already exists`)

  const packageJson = JSON.parse(readFileSync(npmPackage, 'utf8'))
  if (tagMode) {
    // The release PR is merged: tag the merge on main, never a local commit.
    if (packageJson.version !== version) {
      throw new Error(`npm/package.json is at ${packageJson.version}, not ${version}; merge the release PR first`)
    }
    const verified = spawnSync(process.execPath, [verifyScript, tag, npmPackage, cargoToml], { cwd: repoRoot, encoding: 'utf8' })
    if (verified.status !== 0) throw new Error(`version check failed: ${verified.stderr.trim()}`)
    if (dryRun) {
      process.stdout.write(`[dry-run] would tag HEAD as ${tag}\n`)
      process.exit(0)
    }
    git(['tag', tag])
    process.stdout.write(`tag ${tag} created on main.\npublish with: git push origin ${tag}\n`)
    process.exit(0)
  }
  const releaseBranch = `release/${tag}`
  if (git(['branch', '--list', releaseBranch])) throw new Error(`branch ${releaseBranch} already exists`)
  if (packageJson.version === version) {
    throw new Error(`npm/package.json is already at ${version}`)
  }
  const packageLockJson = JSON.parse(readFileSync(npmPackageLock, 'utf8'))
  const packageLockRoot = packageLockJson.packages?.['']
  if (packageLockJson.name !== packageJson.name || packageLockRoot?.name !== packageJson.name) {
    throw new Error('npm/package-lock.json root package does not match npm/package.json')
  }

  const cargoBefore = readFileSync(cargoToml, 'utf8')
  const cargoAfter = bumpCargo(cargoBefore, version)
  if (cargoAfter === cargoBefore) {
    throw new Error(`could not locate the version line in Cargo.toml`)
  }
  const lockBefore = readFileSync(cargoLock, 'utf8')
  const lockAfter = bumpCargoLock(lockBefore, cargoPackageName(cargoBefore), version)
  if (lockAfter === lockBefore) {
    throw new Error(`Cargo.lock is already at ${version}`)
  }

  // A stale local alias package is a publish hazard: it is gitignored and
  // generated per release, so an old checkout copy would silently ship an
  // old version if published without regenerating. Warn loudly.
  const aliasPackage = path.join(repoRoot, 'npm-martty', 'package.json')
  if (existsSync(aliasPackage)) {
    const aliasVersion = JSON.parse(readFileSync(aliasPackage, 'utf8')).version
    if (aliasVersion !== version) {
      process.stdout.write(
        `warning: local npm-martty/package.json is at ${aliasVersion} — regenerate the alias`
          + ` (rm -rf npm-martty && node scripts/package-alias.mjs npm npm-martty martty) before publishing it\n`,
      )
    }
  }

  if (dryRun) {
    process.stdout.write(`[dry-run] would create ${releaseBranch} and bump npm/package.json + npm/package-lock.json + Cargo.toml + Cargo.lock to ${version}\n`)
    process.exit(0)
  }

  git(['switch', '-c', releaseBranch])
  packageJson.version = version
  packageLockJson.version = version
  packageLockRoot.version = version
  writeFileSync(npmPackage, `${JSON.stringify(packageJson, null, 2)}\n`)
  writeFileSync(npmPackageLock, `${JSON.stringify(packageLockJson, null, 2)}\n`)
  writeFileSync(cargoToml, cargoAfter)
  writeFileSync(cargoLock, lockAfter)

  const verify = spawnSync(process.execPath, [
    verifyScript,
    tag,
    npmPackage,
    cargoToml,
  ], { cwd: repoRoot, encoding: 'utf8' })
  if (verify.status !== 0) {
    throw new Error(`self-check failed: ${verify.stderr.trim()}`)
  }

  git(['add',
    path.relative(repoRoot, npmPackage),
    path.relative(repoRoot, npmPackageLock),
    path.relative(repoRoot, cargoToml),
    path.relative(repoRoot, cargoLock),
  ])
  git(['commit', '-m', `release: ${tag}`])

  process.stdout.write(`release commit for ${tag} created on ${releaseBranch}.\n`)
  process.stdout.write(`next: git push -u origin ${releaseBranch} && gh pr create --title "release: ${tag}" --body "Closes #<release issue>"\n`)
  process.stdout.write(`after merge: git switch main && git pull && node scripts/release.mjs ${version} --tag && git push origin ${tag}\n`)
} catch (error) {
  fail(error.message)
}

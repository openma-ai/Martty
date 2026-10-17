#!/usr/bin/env node

import {
  cpSync,
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import { nativeTargets } from './native-targets.mjs'

const legacyName = '@openma/deepseek-harness-tui'
const textExtensions = new Set(['.js', '.json', '.md', '.mjs', '.yaml', '.yml'])
// Never carried into the alias package: installed dependencies and the
// lockfile are build artifacts of the source checkout, not shippable
// content — rewriting (or packing) them would leak the alias rename into
// dependency internals.
const skippedEntries = new Set(['node_modules', 'package-lock.json'])

// npm and pnpm both treat these as "build scripts" of an installed package.
const INSTALL_LIFECYCLE = ['preinstall', 'install', 'postinstall']

function installLifecycleScripts(manifest) {
  const scripts = manifest?.scripts
  if (scripts === null || typeof scripts !== 'object') return []
  return INSTALL_LIFECYCLE.filter((name) => typeof scripts[name] === 'string')
}

function rewriteTextFiles(root, aliasName) {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const file = path.join(root, entry.name)
    if (entry.isDirectory()) {
      if (entry.name !== 'vendor' && !skippedEntries.has(entry.name)) {
        rewriteTextFiles(file, aliasName)
      }
      continue
    }
    if (skippedEntries.has(entry.name)) continue
    if (!entry.isFile() || !textExtensions.has(path.extname(entry.name))) continue
    const before = readFileSync(file, 'utf8')
    const after = before.replaceAll(legacyName, aliasName)
    if (after !== before) writeFileSync(file, after)
  }
}

try {
  const [sourceArg, destinationArg, aliasName] = process.argv.slice(2)
  if (!sourceArg || !destinationArg || !aliasName) {
    throw new Error('usage: package-alias.mjs <source> <destination> <alias-name>')
  }
  const source = path.resolve(sourceArg)
  const destination = path.resolve(destinationArg)
  if (!existsSync(source) || !statSync(source).isDirectory()) {
    throw new Error(`source package directory not found: ${source}`)
  }
  if (existsSync(destination)) {
    throw new Error(
      `destination already exists: ${destination}\n`
      + `  the alias is generated fresh per release — delete the stale copy first:\n`
      + `  rm -rf ${destination}`,
    )
  }
  const packageJson = JSON.parse(readFileSync(path.join(source, 'package.json'), 'utf8'))
  // The published package must stay installable under pnpm's build-script
  // policy: `dsh plugin --profile <name> add martty` runs pnpm, which fails
  // with ERR_PNPM_IGNORED_BUILDS on any dependency that declares an install
  // lifecycle script it has not been allowed to run. Migration happens at
  // launch instead (lib/install-migration.js).
  const lifecycle = installLifecycleScripts(packageJson)
  if (lifecycle.length > 0) {
    throw new Error(
      `${packageJson.name} declares install lifecycle scripts (${lifecycle.join(', ')}); `
      + 'the published package must ship none — pnpm-managed installs such as '
      + '`dsh plugin add` refuse them. Converge at launch instead.',
    )
  }
  if (packageJson.name !== legacyName && packageJson.name !== aliasName) {
    throw new Error(`expected source package ${legacyName} or ${aliasName}, found ${packageJson.name}`)
  }

  cpSync(source, destination, {
    recursive: true,
    errorOnExist: true,
    force: false,
    filter(file) {
      const topLevel = path.relative(source, file).split(path.sep)[0]
      return !['vendor', 'node_modules', 'package-lock.json'].includes(topLevel)
    },
  })
  rewriteTextFiles(destination, aliasName)
  // The alias is generated fresh for every release: if it were ever
  // published stale (an old checkout lingering on disk), the version and
  // dependency ranges would silently diverge from the source package.
  const aliasJson = JSON.parse(readFileSync(path.join(destination, 'package.json'), 'utf8'))
  if (aliasJson.version !== packageJson.version) {
    throw new Error(
      `alias version ${aliasJson.version} does not match source ${packageJson.version}`,
    )
  }
  aliasJson.optionalDependencies = Object.fromEntries(nativeTargets.map(({ platform, arch }) => [
    `@openma/martty-${platform}-${arch}`, aliasJson.version,
  ]))
  // The published package declares only `<alias>-cli`, a name nothing else
  // owns: npm refuses to link a global bin already owned by another package
  // (and links bins before running lifecycle scripts), so declaring `martty`
  // or `dsh-tui` would EEXIST against every legacy install and the unrelated
  // `dsh-tui` package. The binary writes the real `martty` command itself
  // on first launch (lib/install-migration.js); `martty-cli` is what npm
  // links and what `npx` resolves.
  aliasJson.bin = {
    [`${aliasName}-cli`]: packageJson.bin?.[`${aliasName}-cli`] ?? packageJson.bin?.[aliasName] ?? 'bin/martty.js',
  }
  if (Array.isArray(aliasJson.files)) aliasJson.files = aliasJson.files.filter((entry) => entry !== 'vendor')
  writeFileSync(path.join(destination, 'package.json'), `${JSON.stringify(aliasJson, null, 2)}\n`)
  process.stdout.write(`${destination}\n`)
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

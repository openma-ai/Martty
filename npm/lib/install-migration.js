// Migration aid for the package rename `@openma/deepseek-harness-tui` ->
// `martty`.
//
// npm refuses to link a global bin owned by another package, so every bin
// collision surface matters: `martty` releases no longer declare `dsh-tui`
// (which also clashes with an unrelated third-party `dsh-tui` package), and
// published `@openma/deepseek-harness-tui` versions may still own `dsh-tui`
// or `martty` links. Lifecycle scripts run after npm links bins, so the
// postinstall entry point cannot prevent an EEXIST on `martty` — but when
// the install does succeed it can retire the legacy package's leftover
// `dsh-tui` command, and the launch-time entry point can name the package
// still sitting in the same install root.
//
// The legacy name is intentionally assembled: `scripts/package-alias.mjs`
// rewrites the literal `@openma/deepseek-harness-tui` to `martty` in every
// shipped .js file, which would silently break detection inside the `martty`
// package itself.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const LEGACY_PACKAGE = ['@openma', 'deepseek-harness-tui'].join('/')
const scopedParts = LEGACY_PACKAGE.split('/')
const LEGACY_PACKAGE_DIR = scopedParts[scopedParts.length - 1]

// Bin names only ever shipped by the legacy package. `martty` is excluded:
// when the postinstall entry point runs, the `martty` link in the bin
// directory belongs to the package that was just installed.
const LEGACY_ONLY_BINS = ['dsh-tui']

const stderrLine = (line) => process.stderr.write(`${line}\n`)

// Global installs land in <prefix>/lib/node_modules on POSIX and in
// <prefix>/node_modules on Windows; npm link uses the same layout.
export const legacyInstallDirs = (prefix) => [
  path.join(prefix, 'lib', 'node_modules', ...scopedParts),
  path.join(prefix, 'node_modules', ...scopedParts),
]

// POSIX global bins live in <prefix>/bin as symlinks; Windows creates a
// msys shim, a .cmd shim, and a .ps1 shim directly under the prefix.
export const binCandidates = (prefix, name) => [
  path.join(prefix, 'bin', name),
  path.join(prefix, name),
  path.join(prefix, `${name}.cmd`),
  path.join(prefix, `${name}.ps1`),
]

// A bin entry belongs to the legacy package when its symlink target (POSIX)
// or its shim contents (Windows) point into the legacy install directory.
// Anything else — a third-party `dsh-tui` package, a hand-written script —
// is left untouched.
export function isLegacyOwned(file) {
  let stat
  try {
    stat = fs.lstatSync(file)
  } catch {
    return false
  }
  try {
    if (stat.isSymbolicLink()) {
      return fs.readlinkSync(file).includes(LEGACY_PACKAGE_DIR)
    }
    if (!stat.isFile()) return false
    return fs.readFileSync(file, 'utf8').includes(LEGACY_PACKAGE_DIR)
  } catch {
    return false
  }
}

function legacyNotice(write) {
  write(`${LEGACY_PACKAGE} is the previous name of this package — it now ships as \`martty\`.`)
  write(`To migrate: npm rm -g ${LEGACY_PACKAGE} && npm install --global martty`)
}

function marttyNotice(write, hint) {
  write(`martty: ${LEGACY_PACKAGE} is still installed. Remove it to finish the rename:`)
  write(`martty:   ${hint}`)
}

// postinstall entry point (`npm_package_name`/`npm_config_*` env): clean up
// the legacy package's stale `dsh-tui` links and nudge the user to remove
// the old package. Never fails the install.
export function migrate(env, write = stderrLine) {
  if (env.npm_config_global !== 'true') return
  const prefix = env.npm_config_prefix
  if (!prefix) return
  const packageName = env.npm_package_name ?? ''

  if (packageName === LEGACY_PACKAGE) {
    legacyNotice(write)
    return
  }

  const legacyInstalled = legacyInstallDirs(prefix).some((dir) => fs.existsSync(dir))

  const removed = new Set()
  for (const name of LEGACY_ONLY_BINS) {
    for (const file of binCandidates(prefix, name)) {
      if (!isLegacyOwned(file)) continue
      try {
        fs.rmSync(file, { force: true })
        removed.add(name)
      } catch {
        // An undeletable shim must not fail the install; the nudge below
        // still names the blocking package.
      }
    }
  }

  if (removed.size > 0) {
    const list = [...removed].map((name) => `\`${name}\``).join(', ')
    write(`martty: removed the stale ${list} command left by ${LEGACY_PACKAGE} — the TUI now runs as \`martty\`.`)
  }
  if (legacyInstalled) {
    marttyNotice(write, `npm rm -g ${LEGACY_PACKAGE}`)
  }
}

// Launch-time entry point: npm hides postinstall output unless it fails, so
// the binary re-checks its own install root and warns while the legacy
// package is still present next to it.
export function warnAboutLegacyPackage({ argv1, libDir, write = stderrLine } = {}) {
  const selfLibDir = libDir ?? path.dirname(fileURLToPath(import.meta.url))
  const packageRoot = path.dirname(selfLibDir)
  const candidateDirs = [path.join(path.dirname(packageRoot), ...scopedParts)]
  // An `npm link`ed binary resolves to the checkout, not the install root;
  // the prefix is still recoverable from the bin symlink path in argv[1].
  if (argv1 && path.basename(path.dirname(argv1)) === 'bin') {
    candidateDirs.push(...legacyInstallDirs(path.dirname(path.dirname(argv1))))
  }
  const legacyDir = candidateDirs.find((dir) => fs.existsSync(dir))

  if (path.basename(packageRoot) === LEGACY_PACKAGE_DIR) {
    legacyNotice(write)
    return
  }
  if (!legacyDir) return

  const segments = legacyDir.split(path.sep)
  const profileAt = segments.lastIndexOf('profiles')
  const hint = profileAt >= 0
    ? `dsh plugin --profile ${segments[profileAt + 1]} remove ${LEGACY_PACKAGE}`
    : `npm rm -g ${LEGACY_PACKAGE}`
  marttyNotice(write, hint)
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  migrate(process.env)
}

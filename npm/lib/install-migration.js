// Migration aid for the package rename `@openma/deepseek-harness-tui` ->
// `martty`.
//
// npm refuses to link a global bin owned by another package, and it links
// bins before running any lifecycle script — so no script inside this
// package can stop an EEXIST on a name the legacy package may already own
// (`martty`, `dsh-tui`, `dsb`). The published `martty` package therefore
// declares only `martty-cli` — a name nothing else owns — and this
// postinstall step creates the user-facing `martty` command itself after
// retiring any legacy-owned links. It also strips `bin` from the legacy
// package's installed manifest, because `npm rm -g` deletes bin links by
// name without checking what they point at — left in place, removing the
// old package would take the fresh `martty` command down with it.
//
// `martty` is a small shim file, not a symlink: `npm rm -g martty` removes
// the package but not this command, and a shim can degrade gracefully —
// when its package is gone it prints a one-line hint and deletes itself,
// instead of leaving a dead symlink behind.
//
// npm hides postinstall output on success, so the binary re-checks its
// install root at launch: invoked as a real `martty`/`martty-cli` command
// it converges the prefix the same way the postinstall does (retire legacy
// links, recreate `martty`, sanitize) and warns while the legacy package is
// still present — which is also what recovers a `--ignore-scripts` install.
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

const COMMAND_NAME = 'martty'
const DECLARED_BIN_NAME = 'martty-cli'
const BIN_FILE = path.join('bin', 'martty.js')

// Bin names the legacy package may own globally: every published version
// ships `dsh-tui`, 0.2.14/0.2.19-era releases add `dsb`, and 0.2.14/0.2.20
// also claim `martty` itself. Links are only removed when they verifiably
// belong to the legacy package — an installed `martty` package keeps its
// own `martty` command and a third-party `dsh-tui` stays untouched.
const LEGACY_BIN_NAMES = [COMMAND_NAME, 'dsh-tui', 'dsb']

// Marks the `martty` shims this package writes so ownership checks can tell
// them from a foreign file that happens to mention a martty path.
const SHIM_MARKER = 'martty-command-shim'
const NOT_INSTALLED_LINE =
  'martty: the martty package is not installed - reinstall with: npm i -g martty'

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

// Classify the entry at `file` for the `martty` command slot:
//   'none'    — nothing there
//   'ours'    — resolves to (or points at) this package's bin file
//   'legacy'  — belongs to the legacy package (isLegacyOwned)
//   'foreign' — something else; never overwritten
function commandOwner(file, binFile) {
  let stat
  try {
    stat = fs.lstatSync(file)
  } catch {
    return 'none'
  }
  try {
    if (stat.isSymbolicLink()) {
      const target = fs.readlinkSync(file)
      if (path.resolve(path.dirname(file), target) === binFile) return 'ours'
      if (target.includes(LEGACY_PACKAGE_DIR)) return 'legacy'
      // A live link whose target names a `martty/` directory points at some
      // martty installation and is left alone; a dangling one is broken
      // anyway and safe to replace. Anything else belongs to another tool.
      if (/martty[/\\]/.test(target)) {
        return fs.existsSync(path.resolve(path.dirname(file), target)) ? 'ours' : 'none'
      }
      return 'foreign'
    }
    if (!stat.isFile()) return 'foreign'
    const content = fs.readFileSync(file, 'utf8')
    if (content.includes(LEGACY_PACKAGE_DIR)) return 'legacy'
    if (content.includes(SHIM_MARKER) || /node_modules[/\\]martty[/\\]/.test(content)) {
      return 'ours'
    }
    return 'foreign'
  } catch {
    return 'foreign'
  }
}

// The POSIX `martty` command is a shim file, not a symlink: `npm rm -g
// martty` removes the package but not this command, and a shim can notice
// its target is gone — it prints a one-line hint and deletes itself instead
// of leaving a dead symlink behind.
function posixShim(binFile) {
  const quoted = `'${binFile.replaceAll("'", "'\\''")}'`
  return `#!/bin/sh
# ${SHIM_MARKER} — written by the martty package; not managed by npm.
target=${quoted}
if [ -f "$target" ]; then
  exec node "$target" "$@"
fi
echo "${NOT_INSTALLED_LINE}" >&2
rm -f "$0" 2>/dev/null
exit 127
`
}

// Windows receives the same shim triplet npm generates (msys shell wrapper,
// .cmd, .ps1), with the same degrade-and-remove behavior. `rel` is the bin
// file's path relative to the shim directory with forward slashes;
// `backRel` is the backslash variant for cmd.exe.
function windowsShims(dir, binFile) {
  const rel = path.relative(dir, binFile).split(path.sep).join('/')
  const backRel = rel.split('/').join('\\')
  return {
    [COMMAND_NAME]: `#!/bin/sh
# ${SHIM_MARKER} — written by the martty package; not managed by npm.
basedir=$(dirname "$(echo "$0" | sed -e 's,\\\\,/,g')")

case \`uname\` in
    *CYGWIN*|*MINGW*|*MSYS*)
        if command -v cygpath > /dev/null 2>&1; then
            basedir=\`cygpath -w "$basedir"\`
        fi
    ;;
esac

target="$basedir/${rel}"
if [ -f "$target" ]; then
  exec node "$target" "$@"
fi
echo "${NOT_INSTALLED_LINE}" >&2
rm -f "$0" 2>/dev/null
exit 127
`,
    [`${COMMAND_NAME}.cmd`]: `@echo off
rem ${SHIM_MARKER} - written by the martty package; not managed by npm.
set "target=%~dp0${backRel}"
if not exist "%target%" goto missing
node "%target%" %*
exit /b %ERRORLEVEL%
:missing
>&2 echo ${NOT_INSTALLED_LINE}
del "%~f0" 2>nul
exit /b 127
`,
    [`${COMMAND_NAME}.ps1`]: `#!/usr/bin/env pwsh
# ${SHIM_MARKER} - written by the martty package; not managed by npm.
$target = Join-Path (Split-Path $MyInvocation.MyCommand.Definition -Parent) '${rel}'
if (Test-Path -LiteralPath $target) {
  & node $target @args
  exit $LASTEXITCODE
}
[Console]::Error.WriteLine('${NOT_INSTALLED_LINE}')
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
exit 127
`,
  }
}

// Create the `martty` command next to wherever `martty-cli` was invoked
// from (`argv[1]`) or inside `binDir` — which keeps it correct for global
// prefixes, `npm link`, and a project's node_modules/.bin alike. Existing
// entries are only replaced when they are ours or verifiably legacy-owned;
// a foreign `martty` is left untouched with a note about `martty-cli`.
export function ensureMarttyLink({ argv1, binDir, packageDir, platform = process.platform, write = stderrLine } = {}) {
  const selfPackageDir = packageDir ?? path.dirname(path.dirname(fileURLToPath(import.meta.url)))
  const binFile = path.join(selfPackageDir, BIN_FILE)
  let dir = binDir ?? null
  if (!dir) {
    // Only an invocation under the declared `martty-cli` name earns a
    // `martty` sibling — a plain `node bin/martty.js` must not scribble a
    // link next to whatever script the caller pointed node at.
    const invokedAs = argv1 ? path.basename(argv1).replace(/\.(cmd|ps1)$/i, '') : null
    if (invokedAs !== DECLARED_BIN_NAME) return false
    dir = path.dirname(argv1)
  }

  const target = path.join(dir, COMMAND_NAME)
  const owner = commandOwner(target, binFile)
  if (owner === 'ours') return true
  if (owner === 'foreign') {
    write(`martty: \`martty\` in ${dir} belongs to another tool — leaving it untouched.`)
    write('martty: this package\'s command is `martty-cli`.')
    return false
  }

  try {
    fs.mkdirSync(dir, { recursive: true })
    if (platform === 'win32') {
      for (const [name, content] of Object.entries(windowsShims(dir, binFile))) {
        fs.writeFileSync(path.join(dir, name), content)
      }
    } else {
      fs.rmSync(target, { force: true })
      fs.writeFileSync(target, posixShim(binFile))
      fs.chmodSync(target, 0o755)
    }
  } catch {
    write(`martty: could not create the \`martty\` command in ${dir} — run \`martty-cli\` instead.`)
    return false
  }
  return true
}

function legacyNotice(write) {
  write(`${LEGACY_PACKAGE} is the previous name of this package — it now ships as \`martty\`.`)
  write(`To migrate: npm rm -g ${LEGACY_PACKAGE} && npm install --global martty`)
}

function marttyNotice(write, hint) {
  write(`martty: ${LEGACY_PACKAGE} is still installed. Remove it to finish the rename:`)
  write(`martty:   ${hint}`)
}

// Remove the bin entries the legacy package owns in `prefix` (POSIX links
// and Windows shims alike). Foreign entries never qualify; undeletable
// ones are skipped — they must never fail an install or a launch.
function retireLegacyBins(prefix, write) {
  const removed = new Set()
  for (const name of LEGACY_BIN_NAMES) {
    for (const file of binCandidates(prefix, name)) {
      if (!isLegacyOwned(file)) continue
      try {
        fs.rmSync(file, { force: true })
        removed.add(name)
      } catch {
        // An undeletable link must not fail the install; the nudge below
        // still names the blocking package.
      }
    }
  }
  if (removed.size > 0) {
    const list = [...removed].map((name) => `\`${name}\``).join(', ')
    write(`martty: removed the stale ${list} command${removed.size > 1 ? 's' : ''} left by ${LEGACY_PACKAGE} — the TUI now runs as \`martty\`.`)
  }
}

// postinstall entry point (`npm_package_name`/`npm_config_*` env): retire
// the legacy package's bin links, create the `martty` command, and nudge
// the user to remove the old package. Never fails the install.
export function migrate(env, write = stderrLine, platform = process.platform) {
  if (env.npm_config_global !== 'true') return
  const prefix = env.npm_config_prefix
  if (!prefix) return
  const packageName = env.npm_package_name ?? ''

  if (packageName === LEGACY_PACKAGE) {
    legacyNotice(write)
    return
  }

  const legacyInstalled = legacyInstallDirs(prefix).some((dir) => fs.existsSync(dir))

  retireLegacyBins(prefix, write)

  const packageDir = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
  const binDir = platform === 'win32' ? prefix : path.join(prefix, 'bin')
  ensureMarttyLink({ binDir, packageDir, platform, write })

  if (legacyInstalled) {
    sanitizeLegacyInstall(prefix, write)
    marttyNotice(write, `npm rm -g ${LEGACY_PACKAGE}`)
  }
}

// Neutralize the legacy package's `bin` declarations so a later
// `npm rm -g @openma/deepseek-harness-tui` — which deletes bin links by
// name without checking what they point at — cannot take the new `martty`
// command down with it. A real install gets the `bin` field stripped from
// its installed manifest; an `npm link`ed one is a symlink into the user's
// checkout — editing that file would dirty their worktree, so the symlink
// is replaced with a stub package (name + version only) which `npm rm -g`
// still removes cleanly. Never fails the install.
export function sanitizeLegacyInstall(prefix, write = stderrLine) {
  for (const legacyDir of legacyInstallDirs(prefix)) {
    let stat
    try {
      stat = fs.lstatSync(legacyDir)
    } catch {
      continue
    }

    if (stat.isSymbolicLink()) {
      let version
      try {
        const manifest = JSON.parse(fs.readFileSync(path.join(legacyDir, 'package.json'), 'utf8'))
        if (manifest && typeof manifest.version === 'string') version = manifest.version
      } catch {
        // Unreadable linked manifest — the stub below omits the version.
      }
      try {
        fs.rmSync(legacyDir, { force: true })
        fs.mkdirSync(legacyDir, { recursive: true })
        const stub = { name: LEGACY_PACKAGE, ...(version === undefined ? {} : { version }) }
        fs.writeFileSync(path.join(legacyDir, 'package.json'), `${JSON.stringify(stub, null, 2)}\n`)
      } catch {
        write(`martty: could not detach the linked ${LEGACY_PACKAGE} — remove it with \`npm rm -g ${LEGACY_PACKAGE}\`.`)
      }
      continue
    }

    if (!stat.isDirectory()) continue
    const manifestPath = path.join(legacyDir, 'package.json')
    let manifest
    try {
      manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
    } catch {
      // A missing or unreadable manifest has no readable bin table for a
      // later `npm rm -g` to act on either — nothing to strip.
      continue
    }
    try {
      if (manifest && typeof manifest === 'object' && 'bin' in manifest) {
        delete manifest.bin
        fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
      }
    } catch {
      write(`martty: could not edit the installed ${LEGACY_PACKAGE} manifest — remove it with \`npm rm -g ${LEGACY_PACKAGE}\`.`)
    }
  }
}

// Launch-time entry point: npm hides postinstall output unless it fails, so
// the binary re-checks its install root on every run. Invoked through a
// real command name (`martty`, `martty-cli`, `npx`) it converges the global
// prefix the same way the postinstall does — retire the legacy links,
// (re)create `martty`, sanitize the legacy install — which is also what
// recovers an install whose scripts were skipped. A bare `node
// bin/martty.js` or a dsh profile layout only gets the warning, never the
// filesystem changes.
export function warnAboutLegacyPackage({ argv1, libDir, write = stderrLine, platform = process.platform } = {}) {
  const selfLibDir = libDir ?? path.dirname(fileURLToPath(import.meta.url))
  const packageDir = path.dirname(selfLibDir)
  if (path.basename(packageDir) === LEGACY_PACKAGE_DIR) {
    legacyNotice(write)
    return
  }

  try {
    ensureMarttyLink({ argv1, packageDir, platform, write })
  } catch {
    // A read-only prefix or a missing bin dir must not break the launch.
  }

  // Candidate prefixes to converge: the global root this package sits in
  // (lib/node_modules layout), plus the root behind the bin that launched
  // us (covers `npm link` where the package resolves to a checkout).
  const prefixes = []
  const nodeModules = path.dirname(packageDir)
  if (path.basename(nodeModules) === 'node_modules') {
    const layoutDir = path.dirname(nodeModules)
    prefixes.push(path.basename(layoutDir) === 'lib' ? path.dirname(layoutDir) : layoutDir)
  }
  if (argv1) {
    const invokedAs = path.basename(argv1).replace(/\.(cmd|ps1)$/i, '')
    const binDir = path.dirname(argv1)
    if (
      (invokedAs === COMMAND_NAME || invokedAs === DECLARED_BIN_NAME)
      && (path.basename(binDir) === 'bin' || platform === 'win32')
    ) {
      prefixes.push(platform === 'win32' && path.basename(binDir) !== 'bin' ? binDir : path.dirname(binDir))
    }
  }

  let hint = null
  for (const prefix of new Set(prefixes)) {
    if (!legacyInstallDirs(prefix).some((dir) => fs.existsSync(dir))) continue
    const segments = prefix.split(path.sep)
    const profileAt = segments.lastIndexOf('profiles')
    if (profileAt >= 0) {
      // A dsh profile: warn with the plugin-removal hint, touch nothing —
      // profile layouts are managed by dsh, not npm.
      hint ??= `dsh plugin --profile ${segments[profileAt + 1]} remove ${LEGACY_PACKAGE}`
      continue
    }
    hint ??= `npm rm -g ${LEGACY_PACKAGE}`
    try {
      retireLegacyBins(prefix, write)
      const binDir = platform === 'win32' ? prefix : path.join(prefix, 'bin')
      ensureMarttyLink({ binDir, packageDir, platform, write })
      sanitizeLegacyInstall(prefix, write)
    } catch {
      // Best effort: nothing here may break the launch.
    }
  }

  if (hint) marttyNotice(write, hint)
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  migrate(process.env)
}

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
// npm hides postinstall output on success, so the binary re-checks its
// install root at launch: it warns while the legacy package is still
// present and (re)creates the `martty` command when scripts were skipped
// (`--ignore-scripts`) or the link went missing.
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
    if (/node_modules[/\\]martty[/\\]/.test(content)) return 'ours'
    return 'foreign'
  } catch {
    return 'foreign'
  }
}

// Windows receives the same shim triplet npm generates (msys shell wrapper,
// .cmd, .ps1). `rel` is the bin file's path relative to the shim directory
// with forward slashes; `backRel` is the backslash variant for cmd.exe.
function windowsShims(dir, binFile) {
  const rel = path.relative(dir, binFile).split(path.sep).join('/')
  const backRel = rel.split('/').join('\\')
  return {
    [COMMAND_NAME]: `#!/bin/sh
basedir=$(dirname "$(echo "$0" | sed -e 's,\\\\,/,g')")

case \`uname\` in
    *CYGWIN*|*MINGW*|*MSYS*)
        if command -v cygpath > /dev/null 2>&1; then
            basedir=\`cygpath -w "$basedir"\`
        fi
    ;;
esac

if [ -x "$basedir/node" ]; then
  exec "$basedir/node"  "$basedir/${rel}" "$@"
else
  exec node  "$basedir/${rel}" "$@"
fi
`,
    [`${COMMAND_NAME}.cmd`]: `@SETLOCAL
@IF EXIST "%~dp0\\node.exe" (
  "%~dp0\\node.exe"  "%~dp0\\${backRel}" %*
) ELSE (
  @SET PATHEXT=%PATHEXT:;.JS;=;%
  node  "%~dp0\\${backRel}" %*
)
`,
    [`${COMMAND_NAME}.ps1`]: `#!/usr/bin/env pwsh
$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent

$exe=""
if ($PSVersionTable.PSVersion -lt "6.0" -or $IsWindows) { $exe=".exe" }
$node="$basedir/node$exe"
if (Test-Path $node) {
  & $node  "$basedir/${rel}" $args
} else {
  & node  "$basedir/${rel}" $args
}
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
      fs.symlinkSync(path.relative(dir, binFile), target)
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
// the binary re-checks its own install root, recreates the `martty` command
// when needed, and warns while the legacy package is still present.
export function warnAboutLegacyPackage({ argv1, libDir, write = stderrLine } = {}) {
  const selfLibDir = libDir ?? path.dirname(fileURLToPath(import.meta.url))
  const packageDir = path.dirname(selfLibDir)
  const candidateDirs = [path.join(path.dirname(packageDir), ...scopedParts)]
  // An `npm link`ed binary resolves to the checkout, not the install root;
  // the prefix is still recoverable from the bin symlink path in argv[1].
  if (argv1 && path.basename(path.dirname(argv1)) === 'bin') {
    candidateDirs.push(...legacyInstallDirs(path.dirname(path.dirname(argv1))))
  }
  const legacyDir = candidateDirs.find((dir) => fs.existsSync(dir))

  if (path.basename(packageDir) === LEGACY_PACKAGE_DIR) {
    legacyNotice(write)
    return
  }

  try {
    ensureMarttyLink({ argv1, packageDir, write })
  } catch {
    // A read-only prefix or a missing bin dir must not break the launch.
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

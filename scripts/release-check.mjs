#!/usr/bin/env node

// Release gate for Martty.
//
// Pull requests always print a non-failing notice listing pull requests
// already merged to the base branch since the previous reachable v* tag.
// Those commits are unreleased until a tag publish.
//
// A pull request that changes npm/package.json "version" also fails unless:
// - npm/package.json, npm/package-lock.json (root and packages[""]),
//   Cargo.toml, and Cargo.lock agree
// - README.md, README.en.md, and npm/README.md do not pin a different
//   package version (no pin is fine)
// - the bump is a patch, or a prerelease on that same core version
//   (vX.Y.Z-beta.N, and the alpha/rc forms release.mjs already accepts);
//   a minor bump needs the release:minor label and a major bump needs
//   release:major
// - RELEASE_NOTES_FILE, when set, lists every merged pull request. CI does
//   not set it: the notes gate runs on the tag, where the notes exist.
//
// `node scripts/release-check.mjs --tag` (alias `--publish`) runs on a v*
// tag push, before npm publish. The tag must equal v plus the package
// version. Release notes are the body `gh release create --generate-notes`
// will publish; every pull request merged since the previous reachable v*
// tag must appear there as #N or /pull/N (#100 does not count as #10).
//
// `node scripts/release-check.mjs --audit` reports the same version and
// notes rules for existing tags. It does not apply bump labels, which are
// new. Run it from a full clone; it is not a CI job.
//
// Release flow is unchanged: scripts/release.mjs still bumps versions and
// tags, and package-npm.yml still publishes and creates the GitHub release.

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const VERSION_RE = /^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.(\d+))?$/
const TAG_RE = /^v\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/
const PRE_RANK = { alpha: 0, beta: 1, rc: 2 }
const README_PATHS = ['README.md', 'README.en.md', 'npm/README.md']
const BOLD_VERSION = /\*\*v(\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?)\*\*/g
const PACKAGE_PIN = /(?:^|[\s`'"(])(?:martty|@openma\/deepseek-harness-tui|@openma\/martty)@(\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?)(?![\w.-])/g

export function parseVersion(version) {
  const match = VERSION_RE.exec(version)
  if (!match) {
    throw new Error(`Version must be x.y.z or x.y.z-{alpha,beta,rc}.N, got ${JSON.stringify(version)}`)
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    pre: match[4] ? { kind: match[4], n: Number(match[5]) } : null,
    raw: version,
  }
}

/** Stable is greater than any prerelease of the same core version. */
function comparePrerelease(left, right) {
  if (!left && !right) return 0
  if (!left) return 1
  if (!right) return -1
  if (left.kind !== right.kind) return PRE_RANK[left.kind] - PRE_RANK[right.kind]
  return left.n - right.n
}

/**
 * @returns {{ kind: 'major' | 'minor' | 'patch' | 'prerelease' | 'same' | 'invalid', reason?: string }}
 * `prerelease` is a beta/alpha/rc increment or a graduation to the same core
 * stable version. A prerelease that raises minor or major is `minor` / `major`.
 */
export function classifyBump(from, to) {
  let previous
  let next
  try {
    previous = parseVersion(from)
    next = parseVersion(to)
  } catch (error) {
    return {
      kind: 'invalid',
      reason: error instanceof Error ? error.message : String(error),
    }
  }
  if (next.major !== previous.major) {
    return next.major > previous.major
      ? { kind: 'major' }
      : { kind: 'invalid', reason: downgradeReason(from, to) }
  }
  if (next.minor !== previous.minor) {
    return next.minor > previous.minor
      ? { kind: 'minor' }
      : { kind: 'invalid', reason: downgradeReason(from, to) }
  }
  if (next.patch !== previous.patch) {
    return next.patch > previous.patch
      ? { kind: 'patch' }
      : { kind: 'invalid', reason: downgradeReason(from, to) }
  }
  const order = comparePrerelease(previous.pre, next.pre)
  if (order < 0) return { kind: 'prerelease' }
  if (order === 0) return { kind: 'same', reason: `version ${from} -> ${to} does not increase the version` }
  return { kind: 'invalid', reason: downgradeReason(from, to) }
}

function downgradeReason(from, to) {
  return `Version change ${from} -> ${to} is not a patch, minor, major, or beta increment`
}

/**
 * @param {{ kind: string, reason?: string }} bump
 * @param {readonly string[]} labels
 */
export function bumpLabelErrors(bump, labels, previous, next) {
  const present = new Set(labels)
  if (bump.kind === 'patch' || bump.kind === 'prerelease') return []
  if (bump.kind === 'minor') {
    if (present.has('release:minor')) return []
    return [`Minor bump ${previous} -> ${next} requires the release:minor label`]
  }
  if (bump.kind === 'major') {
    if (present.has('release:major')) return []
    return [`Major bump ${previous} -> ${next} requires the release:major label`]
  }
  return [bump.reason ?? `version ${previous} -> ${next} does not increase the version`]
}

/** Squash `(#N)` subjects and `Merge pull request #N` subjects. */
export function extractPrNumbers(message) {
  const numbers = []
  const seen = new Set()
  const re = /\(#(\d+)\)|\bMerge pull request #(\d+)\b/g
  for (const match of message.matchAll(re)) {
    const number = Number(match[1] ?? match[2])
    if (seen.has(number)) continue
    seen.add(number)
    numbers.push(number)
  }
  return numbers
}

/** `#10` does not match `#100`. `/pull/10` does not match `/pull/100`. */
export function mentionsPullRequest(notes, number) {
  return new RegExp(`(?:#|/pull/)${number}(?!\\d)`).test(notes)
}

/** @param {readonly number[]} numbers */
export function missingPullRequests(notes, numbers) {
  return numbers.filter((number) => !mentionsPullRequest(notes, number))
}

export function notesErrors(notes, notesName, numbers) {
  if (notes == null) {
    if (numbers.length === 0) return []
    return [
      `${notesName} do not exist`,
      ...numbers.map((number) => `${notesName} do not mention #${number}`),
    ]
  }
  return missingPullRequests(notes, numbers).map((number) => `${notesName} do not mention #${number}`)
}

/**
 * Pins of this package only. Historical prose ("Starting with 0.2.13",
 * an example tag `v0.1.0`) and other packages (`@openma/pi-acp@0.1.4`)
 * are not pins. No pin is success.
 */
export function readmeVersionErrors(text, version, filePath) {
  const errors = []
  for (const match of text.matchAll(new RegExp(BOLD_VERSION.source, 'g'))) {
    if (match[1] !== version) {
      errors.push(`${filePath} pins **v${match[1]}**, expected **v${version}**`)
    }
  }
  for (const match of text.matchAll(new RegExp(PACKAGE_PIN.source, 'g'))) {
    if (match[1] !== version) {
      errors.push(`${filePath} pins @${match[1]}, expected @${version}`)
    }
  }
  return errors
}

export function cargoPackageVersion(toml) {
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
  return null
}

export function cargoPackageName(toml) {
  const match = toml.match(/^\[package\][\s\S]*?^name\s*=\s*"([^"]+)"/m)
  return match ? match[1] : null
}

export function cargoLockVersion(lock, packageName) {
  if (!packageName) return null
  const match = lock.match(new RegExp(
    `\\[\\[package\\]\\]\\r?\\nname = "${packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}"\\r?\\nversion = "([^"]+)"`,
  ))
  return match ? match[1] : null
}

/**
 * @param {{
 *   npm: string,
 *   lock: { present: boolean, root: string | null, nested: string | null },
 *   cargo: string | null,
 *   cargoLock: { present: boolean, version: string | null },
 *   tag?: string | null,
 *   requireLock: boolean,
 * }} manifest
 */
export function versionManifestErrors(manifest) {
  const errors = []
  const { npm } = manifest
  if (!manifest.lock.present) {
    if (manifest.requireLock) errors.push('npm/package-lock.json is missing')
  } else {
    if (manifest.lock.root !== npm) {
      errors.push(`npm/package-lock.json version is ${shown(manifest.lock.root)}, expected ${npm}`)
    }
    if (manifest.lock.nested !== npm) {
      errors.push(`npm/package-lock.json packages[""].version is ${shown(manifest.lock.nested)}, expected ${npm}`)
    }
  }
  if (manifest.cargo == null) errors.push('Cargo.toml package version is missing')
  else if (manifest.cargo !== npm) {
    errors.push(`Cargo.toml version is ${manifest.cargo}, expected ${npm}`)
  }
  if (!manifest.cargoLock.present) {
    if (manifest.requireLock) errors.push('Cargo.lock is missing')
  } else if (manifest.cargoLock.version !== npm) {
    errors.push(`Cargo.lock version is ${shown(manifest.cargoLock.version)}, expected ${npm}`)
  }
  if (manifest.tag != null && manifest.tag !== `v${npm}`) {
    errors.push(`tag ${manifest.tag} does not match npm version ${npm} (expected v${npm})`)
  }
  return errors
}

/**
 * @param {{
 *   previousVersion: string | null,
 *   nextVersion: string,
 *   labels: readonly string[],
 *   checkLabels: boolean,
 *   notes: string | null,
 *   notesName: string,
 *   requireNotes: boolean,
 *   mergedPrNumbers: readonly number[],
 *   manifest: Parameters<typeof versionManifestErrors>[0],
 *   readmes: readonly { path: string, text: string }[],
 * }} input
 */
export function evaluateRelease(input) {
  const errors = [
    ...versionManifestErrors(input.manifest),
  ]
  for (const readme of input.readmes) {
    errors.push(...readmeVersionErrors(readme.text, input.nextVersion, readme.path))
  }
  if (input.previousVersion) {
    const bump = classifyBump(input.previousVersion, input.nextVersion)
    if (input.checkLabels) errors.push(...bumpLabelErrors(bump, input.labels, input.previousVersion, input.nextVersion))
    else if (bump.kind === 'invalid' || bump.kind === 'same') errors.push(bump.reason)
  }
  if (input.requireNotes) {
    errors.push(...notesErrors(input.notes, input.notesName, input.mergedPrNumbers))
  }
  return errors
}

export function unreleasedNotice(baseName, previousTag, numbers) {
  if (!previousTag) {
    return `No previous v* tag is reachable from ${baseName}; unreleased pull requests were not listed.`
  }
  if (numbers.length === 0) return null
  const listed = numbers.map((number) => `#${number}`).join(', ')
  return `Unreleased PRs on ${baseName} since ${previousTag}: ${listed}`
}

function shown(value) {
  if (value == null) return 'missing'
  return JSON.stringify(value)
}

function git(cwd, args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8' })
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || result.error?.message || '').trim()
    throw new Error(`git ${args.join(' ')} failed: ${detail}`)
  }
  return (result.stdout ?? '').replace(/\n$/, '')
}

function gitOk(cwd, args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8' })
  return result.status === 0 ? (result.stdout ?? '').replace(/\n$/, '') : null
}

function gh(cwd, args, env) {
  const result = spawnSync('gh', args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, ...env },
  })
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || result.error?.message || '').trim()
    throw new Error(`gh ${args[0]} failed: ${detail}`)
  }
  return result.stdout ?? ''
}

function repoSlug(cwd, env) {
  if (env.GITHUB_REPOSITORY) return env.GITHUB_REPOSITORY
  const url = gitOk(cwd, ['remote', 'get-url', 'origin'])
  if (!url) return null
  const match = url.match(/github\.com[:/]([^/]+)\/([^/.]+?)(?:\.git)?$/)
  if (!match) return null
  return `${match[1]}/${match[2]}`
}

function useGithub(env, slug) {
  return env.RELEASE_CHECK_OFFLINE !== '1' && Boolean(slug)
}

function labelsFrom(env) {
  return (env.PR_LABELS ?? env.RELEASE_LABELS ?? '')
    .split(',')
    .map((label) => label.trim())
    .filter(Boolean)
}

function resolveBase(cwd, env) {
  const name = [env.BASE_REF, env.GITHUB_BASE_REF, 'main'].find((value) => value && value.length > 0) ?? 'main'
  for (const ref of [`origin/${name}`, name]) {
    if (gitOk(cwd, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`])) return { name, ref }
  }
  throw new Error(`Cannot resolve base ref ${name}`)
}

/** Latest reachable v* tag. Tag mode passes the parent so the tag under test is not itself. */
function previousTag(cwd, rev) {
  const described = gitOk(cwd, ['describe', '--tags', '--abbrev=0', '--match', 'v[0-9]*', rev])
  if (described && TAG_RE.test(described)) return described
  return null
}

function readJsonFile(file) {
  return JSON.parse(readFileSync(file, 'utf8'))
}

function headManifest(cwd) {
  const packagePath = path.join(cwd, 'npm', 'package.json')
  if (!existsSync(packagePath)) throw new Error('npm/package.json is missing')
  const npm = readJsonFile(packagePath).version
  if (typeof npm !== 'string' || npm.length === 0) throw new Error('npm/package.json version must be a non-empty string')
  const lockPath = path.join(cwd, 'npm', 'package-lock.json')
  const lock = existsSync(lockPath) ? readJsonFile(lockPath) : null
  const cargoPath = path.join(cwd, 'Cargo.toml')
  if (!existsSync(cargoPath)) throw new Error('Cargo.toml is missing')
  const cargoToml = readFileSync(cargoPath, 'utf8')
  const lockfilePath = path.join(cwd, 'Cargo.lock')
  const cargoLockText = existsSync(lockfilePath) ? readFileSync(lockfilePath, 'utf8') : null
  return {
    npm,
    lock: lock
      ? { present: true, root: lock.version ?? null, nested: lock.packages?.['']?.version ?? null }
      : { present: false, root: null, nested: null },
    cargo: cargoPackageVersion(cargoToml),
    cargoLock: cargoLockText == null
      ? { present: false, version: null }
      : { present: true, version: cargoLockVersion(cargoLockText, cargoPackageName(cargoToml)) },
    requireLock: true,
  }
}

function manifestAt(cwd, rev) {
  const packageJson = JSON.parse(git(cwd, ['show', `${rev}:npm/package.json`]))
  const npm = packageJson.version
  if (typeof npm !== 'string' || npm.length === 0) {
    throw new Error(`${rev}:npm/package.json version must be a non-empty string`)
  }
  const lockRaw = gitOk(cwd, ['show', `${rev}:npm/package-lock.json`])
  const cargoToml = git(cwd, ['show', `${rev}:Cargo.toml`])
  const cargoLockText = gitOk(cwd, ['show', `${rev}:Cargo.lock`])
  let lock = { present: false, root: null, nested: null }
  if (lockRaw != null) {
    const parsed = JSON.parse(lockRaw)
    lock = { present: true, root: parsed.version ?? null, nested: parsed.packages?.['']?.version ?? null }
  }
  return {
    npm,
    lock,
    cargo: cargoPackageVersion(cargoToml),
    cargoLock: cargoLockText == null
      ? { present: false, version: null }
      : { present: true, version: cargoLockVersion(cargoLockText, cargoPackageName(cargoToml)) },
    requireLock: false,
  }
}

function readmesAtHead(cwd) {
  return README_PATHS.filter((file) => existsSync(path.join(cwd, file))).map((file) => ({
    path: file,
    text: readFileSync(path.join(cwd, file), 'utf8'),
  }))
}

function readmesAt(cwd, rev) {
  const readmes = []
  for (const file of README_PATHS) {
    const text = gitOk(cwd, ['show', `${rev}:${file}`])
    if (text != null) readmes.push({ path: file, text })
  }
  return readmes
}

function versionAt(cwd, rev) {
  const raw = git(cwd, ['show', `${rev}:npm/package.json`])
  const version = JSON.parse(raw).version
  if (typeof version !== 'string' || version.length === 0) {
    throw new Error(`${rev}:npm/package.json version must be a non-empty string`)
  }
  return version
}

function commitsSince(cwd, previous, rev) {
  const range = previous ? `${previous}..${rev}` : rev
  const raw = git(cwd, ['log', '--reverse', '--format=%H%x09%s', range])
  if (!raw) return []
  return raw.split('\n').filter(Boolean).map((line) => {
    const tab = line.indexOf('\t')
    if (tab === -1) return { sha: line, subject: '' }
    return { sha: line.slice(0, tab), subject: line.slice(tab + 1) }
  })
}

function offlineNumbers(commits) {
  const seen = new Set()
  const numbers = []
  for (const commit of commits) {
    for (const number of extractPrNumbers(commit.subject)) {
      if (seen.has(number)) continue
      seen.add(number)
      numbers.push(number)
    }
  }
  return numbers
}

function githubMergedPullRequests(cwd, env, slug) {
  const raw = gh(cwd, [
    'pr', 'list',
    '--repo', slug,
    '--state', 'merged',
    '--limit', '1000',
    '--json', 'number,mergeCommit,labels',
  ], env)
  const parsed = JSON.parse(raw)
  if (!Array.isArray(parsed)) throw new Error('gh pr list returned unexpected output')
  if (parsed.length >= 1000) throw new Error('gh pr list hit the 1000 pull request cap')
  return parsed
    .filter((pull) => pull.mergeCommit && typeof pull.mergeCommit.oid === 'string')
    .map((pull) => ({
      number: pull.number,
      sha: pull.mergeCommit.oid,
      labels: (pull.labels ?? []).map((label) => label.name).filter(Boolean),
    }))
}

function numbersInRange(commits, merged) {
  if (!merged) return offlineNumbers(commits)
  const index = new Map(commits.map((commit, position) => [commit.sha, position]))
  return merged
    .filter((pull) => index.has(pull.sha))
    .sort((left, right) => index.get(left.sha) - index.get(right.sha))
    .map((pull) => pull.number)
}

function labelsInRange(commits, merged, envLabels) {
  const labels = new Set(envLabels)
  if (!merged) return [...labels]
  const shas = new Set(commits.map((commit) => commit.sha))
  for (const pull of merged) {
    if (!shas.has(pull.sha)) continue
    for (const label of pull.labels) labels.add(label)
  }
  return [...labels]
}

function notesFromFile(cwd, env) {
  if (!env.RELEASE_NOTES_FILE) return undefined
  return readFileSync(path.resolve(cwd, env.RELEASE_NOTES_FILE), 'utf8')
}

function generateNotes(cwd, env, slug, tag, head) {
  const raw = gh(cwd, [
    'api',
    `repos/${slug}/releases/generate-notes`,
    '-f', `tag_name=${tag}`,
    '-f', `target_commitish=${head}`,
  ], env)
  const parsed = JSON.parse(raw)
  if (typeof parsed.body !== 'string') throw new Error('generate-notes returned no body')
  return parsed.body
}

function publishedNotes(cwd, env, slug, tag) {
  const result = spawnSync('gh', [
    'release', 'view', tag,
    '--repo', slug,
    '--json', 'body',
  ], { cwd, encoding: 'utf8', env: { ...process.env, ...env } })
  if (result.status !== 0) return null
  const parsed = JSON.parse(result.stdout || '{}')
  return typeof parsed.body === 'string' ? parsed.body : ''
}

function workflowLine(kind, message, file) {
  const inActions = process.env.GITHUB_ACTIONS === 'true'
  const escape = (value) => value.replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A')
  const text = inActions ? escape(message) : message
  const target = file ? ` file=${file}` : ''
  return `::${kind}${target}::${text}`
}

function reportResult(errors, notices) {
  const stdout = []
  const stderr = []
  for (const notice of notices) {
    if (notice) stdout.push(notice.startsWith('::') ? notice : workflowLine('notice', notice))
  }
  for (const error of errors) {
    stdout.push(workflowLine('error', error))
    stderr.push(`release-check: ${error}`)
  }
  return { errors, stdout, stderr, code: errors.length > 0 ? 1 : 0 }
}

function writeResult(result) {
  if (result.stdout.length > 0) process.stdout.write(`${result.stdout.join('\n')}\n`)
  if (result.stderr.length > 0) process.stderr.write(`${result.stderr.join('\n')}\n`)
  return result.code
}

function loadMerged(cwd, env) {
  const slug = repoSlug(cwd, env)
  if (!useGithub(env, slug)) return { slug, merged: null }
  return { slug, merged: githubMergedPullRequests(cwd, env, slug) }
}

function checkPullRequest(cwd, env) {
  const base = resolveBase(cwd, env)
  const previous = previousTag(cwd, base.ref)
  const commits = commitsSince(cwd, previous, base.ref)
  const { merged } = loadMerged(cwd, env)
  const numbers = numbersInRange(commits, merged)
  const notice = unreleasedNotice(base.name, previous, numbers)
  const manifest = headManifest(cwd)
  const previousVersion = versionAt(cwd, base.ref)
  const versionChanged = manifest.npm !== previousVersion
  // Notes completeness is enforced on the tag push, which is what publish
  // runs. A pull request can still supply RELEASE_NOTES_FILE to preview it.
  const fromFile = versionChanged ? notesFromFile(cwd, env) : undefined
  const errors = evaluateRelease({
    previousVersion: versionChanged ? previousVersion : null,
    nextVersion: manifest.npm,
    labels: labelsFrom(env),
    checkLabels: versionChanged,
    notes: fromFile ?? null,
    notesName: 'GitHub release notes',
    requireNotes: fromFile != null,
    mergedPrNumbers: numbers,
    manifest: { ...manifest, tag: null },
    readmes: readmesAtHead(cwd),
  })
  if (!versionChanged && errors.length === 0) {
    const result = reportResult([], notice ? [notice] : [])
    if (!notice) result.stdout.push(`No unreleased pull requests on ${base.name} since ${previous}.`)
    result.stdout.push(`npm version is unchanged (${manifest.npm}); release gate skipped.`)
    return result
  }
  const result = reportResult(errors, notice ? [notice] : [])
  if (errors.length === 0) {
    result.stdout.push(`release check passed for ${previousVersion} -> ${manifest.npm}.`)
  }
  return result
}

function tagName(cwd, env) {
  const ref = env.GITHUB_REF ?? ''
  if (ref.startsWith('refs/tags/')) return ref.slice('refs/tags/'.length)
  if (env.GITHUB_REF_NAME && TAG_RE.test(env.GITHUB_REF_NAME)) return env.GITHUB_REF_NAME
  const exact = gitOk(cwd, ['describe', '--tags', '--exact-match', 'HEAD'])
  if (exact && TAG_RE.test(exact)) return exact
  throw new Error('Cannot determine the release tag (expected GITHUB_REF refs/tags/v*)')
}

function checkTag(cwd, env) {
  const tag = tagName(cwd, env)
  const head = git(cwd, ['rev-parse', 'HEAD'])
  const parent = gitOk(cwd, ['rev-parse', '--verify', '--quiet', `${head}^`])
  const previous = parent ? previousTag(cwd, parent) : null
  const commits = commitsSince(cwd, previous, head)
  const { slug, merged } = loadMerged(cwd, env)
  const numbers = numbersInRange(commits, merged)
  const manifest = { ...headManifest(cwd), tag }
  const previousVersion = previous ? versionAt(cwd, previous) : null
  const fromFile = notesFromFile(cwd, env)
  let notes
  if (fromFile != null) notes = fromFile
  else if (useGithub(env, slug)) notes = generateNotes(cwd, env, slug, tag, head)
  else throw new Error('Cannot load GitHub release notes (set RELEASE_NOTES_FILE or run with gh)')
  const labels = labelsInRange(commits, merged, labelsFrom(env))
  const errors = evaluateRelease({
    previousVersion,
    nextVersion: manifest.npm,
    labels,
    checkLabels: true,
    notes,
    notesName: 'GitHub release notes',
    requireNotes: true,
    mergedPrNumbers: numbers,
    manifest,
    readmes: readmesAtHead(cwd),
  })
  const result = reportResult(errors, [])
  if (errors.length === 0) {
    const since = previous ?? 'the beginning of history'
    result.stdout.push(`Tag ${tag} matches ${manifest.npm}. Release notes since ${since} include ${numbers.length} pull request(s).`)
    result.stdout.push('release tag check passed.')
  }
  return result
}

function versionTags(cwd) {
  const raw = git(cwd, ['for-each-ref', '--sort=creatordate', '--format=%(refname:short)', 'refs/tags'])
  if (!raw) return []
  return raw.split('\n').filter((name) => TAG_RE.test(name))
}

function auditOne(cwd, env, slug, merged, tag) {
  const commit = git(cwd, ['rev-parse', `${tag}^{}`])
  const parent = gitOk(cwd, ['rev-parse', '--verify', '--quiet', `${commit}^`])
  const previous = parent ? previousTag(cwd, parent) : null
  const commits = commitsSince(cwd, previous, commit)
  const numbers = numbersInRange(commits, merged)
  const manifest = { ...manifestAt(cwd, commit), tag, requireLock: false }
  const previousVersion = previous ? versionAt(cwd, previous) : null
  const notes = publishedNotes(cwd, env, slug, tag)
  return {
    previous,
    numbers,
    errors: evaluateRelease({
      previousVersion,
      nextVersion: manifest.npm,
      labels: [],
      checkLabels: false,
      notes,
      notesName: `GitHub release notes for ${tag}`,
      requireNotes: true,
      mergedPrNumbers: numbers,
      manifest,
      readmes: readmesAt(cwd, commit),
    }),
  }
}

function auditHistory(cwd, env) {
  const slug = repoSlug(cwd, env)
  if (!slug) throw new Error('Cannot audit without a GitHub remote')
  const merged = githubMergedPullRequests(cwd, env, slug)
  const tags = versionTags(cwd)
  let pass = 0
  let fail = 0
  const lines = []
  for (const tag of tags) {
    try {
      const { previous, numbers, errors } = auditOne(cwd, env, slug, merged, tag)
      if (errors.length === 0) {
        pass += 1
        lines.push(`PASS ${tag} previous=${previous ?? '(none)'} prs=${numbers.join(',') || '-'}`)
      } else {
        fail += 1
        lines.push(`FAIL ${tag} previous=${previous ?? '(none)'} prs=${numbers.join(',') || '-'}`)
        for (const error of errors) lines.push(`  - ${error}`)
      }
    } catch (error) {
      fail += 1
      const message = error instanceof Error ? error.message : String(error)
      lines.push(`FAIL ${tag}`)
      lines.push(`  - ${message}`)
    }
  }
  lines.push(`summary pass=${pass} fail=${fail} tags=${tags.length}`)
  return { code: 0, stdout: lines, stderr: [] }
}

function unknownArguments(argv) {
  return argv.filter((arg) => arg.startsWith('-') && !['--tag', '--publish', '--audit'].includes(arg))
}

export function runReleaseCheck(cwd, env, argv) {
  const unknown = unknownArguments(argv)
  if (unknown.length > 0) {
    return reportResult([`Unknown arguments: ${unknown.join(' ')}`], [])
  }
  if (argv.includes('--audit')) {
    if (argv.length !== 1) return reportResult(['--audit does not take other arguments'], [])
    return auditHistory(cwd, env)
  }
  if (argv.includes('--tag') || argv.includes('--publish')) return checkTag(cwd, env)
  if (argv.length > 0) return reportResult([`Unknown arguments: ${argv.join(' ')}`], [])
  return checkPullRequest(cwd, env)
}

const invokedDirectly = typeof process.argv[1] === 'string'
  && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href

if (invokedDirectly) {
  try {
    const result = runReleaseCheck(process.cwd(), process.env, process.argv.slice(2))
    process.exitCode = writeResult(result)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    process.stderr.write(`release-check: ${message}\n`)
    process.exitCode = 1
  }
}

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  bumpLabelErrors,
  cargoLockVersion,
  cargoPackageVersion,
  classifyBump,
  evaluateRelease,
  extractPrNumbers,
  mentionsPullRequest,
  missingPullRequests,
  notesErrors,
  parseVersion,
  readmeVersionErrors,
  unreleasedNotice,
  versionManifestErrors,
} from './release-check.mjs'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const script = path.join(repoRoot, 'scripts', 'release-check.mjs')

function manifest(version, extras = {}) {
  const lockVersion = extras.lock ?? version
  const nested = extras.nested ?? lockVersion
  const cargo = extras.cargo ?? version
  const cargoLock = extras.cargoLock ?? version
  return {
    npm: version,
    lock: extras.lockPresent === false
      ? { present: false, root: null, nested: null }
      : { present: true, root: lockVersion, nested },
    cargo,
    cargoLock: extras.cargoLockPresent === false
      ? { present: false, version: null }
      : { present: true, version: cargoLock },
    tag: extras.tag ?? null,
    requireLock: extras.requireLock ?? true,
  }
}

test('parses stable and beta versions, and rejects anything else', () => {
  assert.deepEqual(parseVersion('0.3.1'), {
    major: 0, minor: 3, patch: 1, pre: null, raw: '0.3.1',
  })
  assert.deepEqual(parseVersion('0.3.1-beta.0').pre, { kind: 'beta', n: 0 })
  assert.equal(parseVersion('1.2.3-rc.4').pre.kind, 'rc')
  assert.equal(parseVersion('1.2.3-alpha.1').pre.kind, 'alpha')
  assert.throws(() => parseVersion('v0.3.1'), /x\.y\.z/)
  assert.throws(() => parseVersion('0.3.1-beta'), /x\.y\.z/)
  assert.throws(() => parseVersion('0.3.1-dev.1'), /x\.y\.z/)
})

test('classifies patch, minor, major, and beta increments', () => {
  assert.equal(classifyBump('0.3.0', '0.3.1').kind, 'patch')
  assert.equal(classifyBump('0.3.1', '0.3.10').kind, 'patch')
  assert.equal(classifyBump('0.2.40', '0.3.0').kind, 'minor')
  assert.equal(classifyBump('0.3.1', '1.0.0').kind, 'major')
  assert.equal(classifyBump('0.3.0', '0.3.1-beta.0').kind, 'patch')
  assert.equal(classifyBump('0.2.40', '0.3.0-beta.0').kind, 'minor')
  assert.equal(classifyBump('0.3.1', '1.0.0-beta.2').kind, 'major')
  assert.equal(classifyBump('0.2.2-beta.0', '0.2.2-beta.1').kind, 'prerelease')
  assert.equal(classifyBump('0.2.2-beta.3', '0.2.2').kind, 'prerelease')
  assert.equal(classifyBump('0.3.2-alpha.1', '0.3.2-beta.0').kind, 'prerelease')
  assert.equal(classifyBump('0.3.1', '0.3.1').kind, 'same')
  assert.equal(classifyBump('0.3.1', '0.3.1-beta.0').kind, 'invalid')
  assert.equal(classifyBump('0.3.2-rc.0', '0.3.2-beta.0').kind, 'invalid')
  assert.equal(classifyBump('0.3.1', '0.3.0').kind, 'invalid')
})

test('allows a patch or beta without a label and requires one for minor or major', () => {
  assert.deepEqual(bumpLabelErrors({ kind: 'patch' }, [], '0.3.0', '0.3.1'), [])
  assert.deepEqual(bumpLabelErrors({ kind: 'prerelease' }, [], '0.3.1-beta.0', '0.3.1'), [])
  assert.deepEqual(bumpLabelErrors({ kind: 'minor' }, [], '0.2.40', '0.3.0'), [
    'Minor bump 0.2.40 -> 0.3.0 requires the release:minor label',
  ])
  assert.deepEqual(bumpLabelErrors({ kind: 'minor' }, ['release:major'], '0.2.40', '0.3.0'), [
    'Minor bump 0.2.40 -> 0.3.0 requires the release:minor label',
  ])
  assert.deepEqual(bumpLabelErrors({ kind: 'minor' }, ['other', 'release:minor'], '0.2.40', '0.3.0'), [])
  assert.deepEqual(bumpLabelErrors({ kind: 'major' }, ['release:minor'], '0.3.1', '1.0.0'), [
    'Major bump 0.3.1 -> 1.0.0 requires the release:major label',
  ])
  assert.deepEqual(bumpLabelErrors({ kind: 'major' }, ['release:major'], '0.3.1', '1.0.0'), [])
})

test('reads squash and merge subjects, and does not treat #100 as #10', () => {
  assert.deepEqual(extractPrNumbers('feat: one (#10)'), [10])
  assert.deepEqual(extractPrNumbers('Merge pull request #133 from anish0509/fix'), [133])
  assert.deepEqual(extractPrNumbers('chore: refs #10 and (#9) plus (#10)'), [9, 10])
  assert.deepEqual(extractPrNumbers('issue #49 only'), [])
  assert.equal(mentionsPullRequest('see #100 and /pull/101', 10), false)
  assert.equal(mentionsPullRequest('see #10', 1), false)
  assert.equal(mentionsPullRequest('https://github.com/openma-ai/Martty/pull/136', 136), true)
  assert.equal(mentionsPullRequest('https://github.com/openma-ai/Martty/pull/1360', 136), false)
  assert.equal(mentionsPullRequest('landed in #10 (`077db8f`)', 10), true)
  assert.deepEqual(missingPullRequests('#100\n/pull/12', [10, 12, 100]), [10])
})

test('ignores historical prose and other packages in READMEs', () => {
  const prose = [
    'Starting with `0.2.13`, `martty`',
    'for example, `v0.1.0`) publishes',
    '`@openma/pi-acp@0.1.4`',
    'dsh plugin --profile martty add martty@latest',
    'CI uses 10.2.0',
  ].join('\n')
  assert.deepEqual(readmeVersionErrors(prose, '0.3.1', 'README.en.md'), [])
  assert.deepEqual(readmeVersionErrors('> **v0.1.2** · official package\n', '0.1.2', 'README.md'), [])
  assert.deepEqual(readmeVersionErrors('> **v0.1.1** · official package\n', '0.1.2', 'README.md'), [
    'README.md pins **v0.1.1**, expected **v0.1.2**',
  ])
  assert.deepEqual(readmeVersionErrors('install martty@0.2.0\n', '0.3.1', 'README.md'), [
    'README.md pins @0.2.0, expected @0.3.1',
  ])
})

test('current READMEs do not pin a different package version', () => {
  const version = JSON.parse(readFileSync(path.join(repoRoot, 'npm', 'package.json'), 'utf8')).version
  for (const file of ['README.md', 'README.en.md', 'npm/README.md']) {
    const text = readFileSync(path.join(repoRoot, file), 'utf8')
    assert.deepEqual(readmeVersionErrors(text, version, file), [])
  }
})

test('version files must match, and a missing historical lockfile is allowed', () => {
  assert.deepEqual(versionManifestErrors(manifest('0.3.1', { tag: 'v0.3.1' })), [])
  assert.deepEqual(versionManifestErrors(manifest('0.3.1', { cargo: '0.2.21', cargoLock: '0.2.21', tag: 'v0.3.1' })), [
    'Cargo.toml version is 0.2.21, expected 0.3.1',
    'Cargo.lock version is "0.2.21", expected 0.3.1',
  ])
  assert.ok(versionManifestErrors(manifest('0.3.1', { lock: '0.3.0' })).some((error) => error.includes('package-lock.json version')))
  assert.ok(versionManifestErrors(manifest('0.3.1', { nested: '0.3.0' })).some((error) => error.includes('packages[""]')))
  assert.deepEqual(versionManifestErrors(manifest('0.1.0', {
    lockPresent: false,
    requireLock: false,
    tag: 'v0.1.0',
  })), [])
  assert.ok(versionManifestErrors(manifest('0.3.1', { lockPresent: false })).some((error) => error.includes('package-lock.json is missing')))
  assert.equal(cargoPackageVersion('[package]\nname = "demo"\nversion = "0.2.22-beta.0"\n'), '0.2.22-beta.0')
  assert.equal(cargoLockVersion('[[package]]\nname = "deepseek-harness-tui"\nversion = "0.3.1"\n', 'deepseek-harness-tui'), '0.3.1')
})

test('historical release shapes: complete notes pass, omissions and drift fail', () => {
  const readme = { path: 'README.en.md', text: 'Starting with `0.2.13`.\nExample `v0.1.0`.\n' }
  assert.deepEqual(evaluateRelease({
    previousVersion: '0.3.0',
    nextVersion: '0.3.1',
    labels: [],
    checkLabels: false,
    notes: '## What\'s Changed\n* Package by @hrhrng in https://github.com/openma-ai/Martty/pull/136\n',
    notesName: 'GitHub release notes for v0.3.1',
    requireNotes: true,
    mergedPrNumbers: [136],
    manifest: manifest('0.3.1', { tag: 'v0.3.1', requireLock: false }),
    readmes: [readme],
  }), [])

  const omitted = notesErrors(null, 'GitHub release notes for v0.2.9', [20, 22])
  assert.deepEqual(omitted, [
    'GitHub release notes for v0.2.9 do not exist',
    'GitHub release notes for v0.2.9 do not mention #20',
    'GitHub release notes for v0.2.9 do not mention #22',
  ])

  const stableAfterBeta = evaluateRelease({
    previousVersion: '0.2.27-beta.0',
    nextVersion: '0.2.27',
    labels: [],
    checkLabels: false,
    notes: [
      '* copy (#43) in https://github.com/openma-ai/Martty/pull/48',
      '* tree in https://github.com/openma-ai/Martty/pull/50',
      '* release in https://github.com/openma-ai/Martty/pull/52',
    ].join('\n'),
    notesName: 'GitHub release notes for v0.2.27',
    requireNotes: true,
    mergedPrNumbers: [52],
    manifest: manifest('0.2.27', { tag: 'v0.2.27', requireLock: false }),
    readmes: [readme],
  })
  assert.deepEqual(stableAfterBeta, [])

  const readmeDrift = evaluateRelease({
    previousVersion: '0.1.1',
    nextVersion: '0.1.2',
    labels: [],
    checkLabels: false,
    notes: '',
    notesName: 'GitHub release notes for v0.1.2',
    requireNotes: true,
    mergedPrNumbers: [],
    manifest: manifest('0.1.2', { tag: 'v0.1.2', lockPresent: false, requireLock: false }),
    readmes: [{ path: 'README.md', text: '> **v0.1.1** · official package\n' }],
  })
  assert.deepEqual(readmeDrift, ['README.md pins **v0.1.1**, expected **v0.1.2**'])

  assert.equal(unreleasedNotice('main', 'v0.3.1', [137, 138, 140]), 'Unreleased PRs on main since v0.3.1: #137, #138, #140')
  assert.equal(unreleasedNotice('main', 'v0.3.1', []), null)
})

test('pull request notices do not fail when the version is unchanged', (t) => {
  const cwd = repoWithPullRequests(t)
  const result = run(cwd)
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /::notice::Unreleased PRs on main since v0\.1\.5: #5, #6/)
  assert.match(result.stdout, /release gate skipped/)
  assert.doesNotMatch(result.stdout, /::error/)
})

test('a patch release whose notes name every pull request passes', (t) => {
  const cwd = repoWithPullRequests(t)
  release(cwd, '0.1.6')
  writeFileSync(path.join(cwd, 'notes.md'), [
    '* one in https://github.com/openma-ai/Martty/pull/5',
    '* two (#6)',
    '',
  ].join('\n'))
  const result = run(cwd, [], { RELEASE_NOTES_FILE: 'notes.md' })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /release check passed for 0\.1\.5 -> 0\.1\.6/)
  assert.doesNotMatch(result.stderr, /missing|do not mention/)
})

test('fails when release notes omit a merged pull request, and #100 does not cover #10', (t) => {
  const cwd = repoWithPullRequests(t)
  release(cwd, '0.1.6')
  writeFileSync(path.join(cwd, 'notes.md'), '* only #5 and #60\n')
  const result = run(cwd, [], { RELEASE_NOTES_FILE: 'notes.md' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /do not mention #6/)
  assert.doesNotMatch(result.stderr, /do not mention #5/)
  assert.doesNotMatch(result.stderr, /do not mention #60/)

  const longer = repoWithPullRequests(t)
  commitFile(longer, 'feature.txt', 'ten\n', 'feat: ten (#10)')
  release(longer, '0.1.6')
  writeFileSync(path.join(longer, 'notes.md'), '* #100\n* #5\n* #6\n')
  const longerResult = run(longer, [], { RELEASE_NOTES_FILE: 'notes.md' })
  assert.match(longerResult.stderr, /do not mention #10/)
  assert.doesNotMatch(longerResult.stderr, /do not mention #100/)
  assert.doesNotMatch(longerResult.stderr, /do not mention #5/)
})

test('patch and beta bumps need no label; minor and major do', (t) => {
  const minor = repoWithPullRequests(t)
  release(minor, '0.2.0')
  writeNotes(minor, [5, 6])
  assert.match(run(minor, [], { RELEASE_NOTES_FILE: 'notes.md' }).stderr, /Minor bump 0\.1\.5 -> 0\.2\.0 requires the release:minor label/)
  assert.match(run(minor, [], { RELEASE_NOTES_FILE: 'notes.md', PR_LABELS: 'release:major' }).stderr, /requires the release:minor label/)
  assert.equal(run(minor, [], { RELEASE_NOTES_FILE: 'notes.md', PR_LABELS: 'other,release:minor' }).status, 0)

  const major = repoWithPullRequests(t)
  release(major, '1.0.0')
  writeNotes(major, [5, 6])
  assert.match(run(major, [], { RELEASE_NOTES_FILE: 'notes.md', PR_LABELS: 'release:minor' }).stderr, /Major bump 0\.1\.5 -> 1\.0\.0 requires the release:major label/)
  assert.equal(run(major, [], { RELEASE_NOTES_FILE: 'notes.md', PR_LABELS: 'release:major' }).status, 0)

  const beta = repoWithPullRequests(t)
  release(beta, '0.1.6-beta.0')
  writeNotes(beta, [5, 6])
  const betaResult = run(beta, [], { RELEASE_NOTES_FILE: 'notes.md' })
  assert.equal(betaResult.status, 0, betaResult.stderr)

  const minorBeta = repoWithPullRequests(t)
  release(minorBeta, '0.2.0-beta.0')
  writeNotes(minorBeta, [5, 6])
  assert.match(run(minorBeta, [], { RELEASE_NOTES_FILE: 'notes.md' }).stderr, /Minor bump 0\.1\.5 -> 0\.2\.0-beta\.0 requires the release:minor label/)
})

test('rejects lockfile, Cargo, and README drift, and a downgrade', (t) => {
  const lock = repoWithPullRequests(t)
  release(lock, '0.1.6', { lock: '0.1.5' })
  writeNotes(lock, [5, 6])
  assert.match(run(lock, [], { RELEASE_NOTES_FILE: 'notes.md' }).stderr, /package-lock\.json version is "0\.1\.5", expected 0\.1\.6/)

  const cargo = repoWithPullRequests(t)
  release(cargo, '0.1.6', { cargo: '0.1.5', cargoLock: '0.1.5' })
  writeNotes(cargo, [5, 6])
  const cargoResult = run(cargo, [], { RELEASE_NOTES_FILE: 'notes.md' })
  assert.match(cargoResult.stderr, /Cargo\.toml version is 0\.1\.5, expected 0\.1\.6/)
  assert.match(cargoResult.stderr, /Cargo\.lock version is "0\.1\.5", expected 0\.1\.6/)

  const readme = repoWithPullRequests(t)
  release(readme, '0.1.6')
  writeFileSync(path.join(readme, 'README.md'), '> **v0.1.5** · still the old banner\nStarting with `0.2.13`.\n')
  git(readme, ['add', 'README.md'])
  git(readme, ['commit', '--amend', '--no-edit'])
  writeNotes(readme, [5, 6])
  assert.match(run(readme, [], { RELEASE_NOTES_FILE: 'notes.md' }).stderr, /README\.md pins \*\*v0\.1\.5\*\*, expected \*\*v0\.1\.6\*\*/)

  const down = repoWithPullRequests(t)
  release(down, '0.1.4')
  writeNotes(down, [5, 6])
  assert.match(run(down, [], { RELEASE_NOTES_FILE: 'notes.md' }).stderr, /not a patch, minor, major, or beta increment/)
})

test('--tag and --publish require the tag, versions, and release notes', (t) => {
  const cwd = repoWithPullRequests(t)
  release(cwd, '0.1.6')
  writeNotes(cwd, [5, 6])
  const notes = { RELEASE_NOTES_FILE: 'notes.md' }

  const missingTag = run(cwd, ['--tag'], notes)
  assert.equal(missingTag.status, 1)
  assert.match(missingTag.stderr, /Cannot determine the release tag/)

  const mismatch = run(cwd, ['--publish'], { ...notes, GITHUB_REF_NAME: 'v0.1.5' })
  assert.equal(mismatch.status, 1)
  assert.match(mismatch.stderr, /tag v0\.1\.5 does not match npm version 0\.1\.6/)

  writeFileSync(path.join(cwd, 'notes.md'), '* #5 only\n')
  const omitted = run(cwd, ['--tag'], { RELEASE_NOTES_FILE: 'notes.md', GITHUB_REF_NAME: 'v0.1.6' })
  assert.equal(omitted.status, 1)
  assert.match(omitted.stderr, /do not mention #6/)

  writeNotes(cwd, [5, 6])
  const ok = run(cwd, ['--tag'], { RELEASE_NOTES_FILE: 'notes.md', GITHUB_REF_NAME: 'v0.1.6' })
  assert.equal(ok.status, 0, ok.stderr)
  assert.match(ok.stdout, /release tag check passed/)

  const beta = repoWithPullRequests(t)
  release(beta, '0.1.6-beta.0')
  writeNotes(beta, [5, 6])
  const betaResult = run(beta, ['--tag'], { RELEASE_NOTES_FILE: 'notes.md', GITHUB_REF_NAME: 'v0.1.6-beta.0' })
  assert.equal(betaResult.status, 0, betaResult.stderr)

  git(beta, ['tag', 'v0.1.6-beta.0'])
  release(beta, '0.1.6-beta.1')
  writeNotes(beta, [])
  const increment = run(beta, ['--tag'], { RELEASE_NOTES_FILE: 'notes.md', GITHUB_REF_NAME: 'v0.1.6-beta.1' })
  assert.equal(increment.status, 0, increment.stderr)
  git(beta, ['tag', 'v0.1.6-beta.1'])
  release(beta, '0.1.6')
  writeNotes(beta, [])
  const graduation = run(beta, ['--tag'], { RELEASE_NOTES_FILE: 'notes.md', GITHUB_REF_NAME: 'v0.1.6' })
  assert.equal(graduation.status, 0, graduation.stderr)
})

function run(cwd, args = [], extra = {}) {
  const env = { ...process.env }
  for (const key of [
    'GITHUB_EVENT_NAME', 'GITHUB_REF', 'GITHUB_REF_NAME', 'GITHUB_BASE_REF',
    'GITHUB_REPOSITORY', 'BASE_REF', 'PR_LABELS', 'RELEASE_LABELS', 'RELEASE_NOTES_FILE',
    'GH_TOKEN', 'GITHUB_TOKEN',
  ]) delete env[key]
  env.RELEASE_CHECK_OFFLINE = '1'
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd,
    encoding: 'utf8',
    env: { ...env, ...extra },
  })
  return { status: result.status ?? 1, stdout: result.stdout ?? '', stderr: result.stderr ?? '' }
}

function repoWithPullRequests(t) {
  const cwd = mkdtempSync(path.join(tmpdir(), 'martty-release-check-'))
  t.after(() => rmSync(cwd, { recursive: true, force: true }))
  git(cwd, ['init', '-b', 'main'])
  writeTree(cwd, '0.1.5')
  commitAll(cwd, 'release: v0.1.5')
  git(cwd, ['tag', 'v0.1.5'])
  commitFile(cwd, 'feature.txt', 'one\n', 'feat: one (#5)')
  commitFile(cwd, 'feature.txt', 'note\n', 'chore: internal note')
  commitFile(cwd, 'feature.txt', 'two\n', 'feat: two (#6)')
  return cwd
}

function release(cwd, version, extras = {}) {
  // Keep main at the previous version so the pull-request check compares
  // the bump against the base branch, the way CI checks origin/main.
  if (git(cwd, ['branch', '--show-current']).trim() === 'main') git(cwd, ['checkout', '-b', 'release'])
  writeTree(cwd, version, extras)
  commitAll(cwd, `release: v${version}`)
}

function writeNotes(cwd, numbers) {
  writeFileSync(path.join(cwd, 'notes.md'), `${numbers.map((number) => `* change in /pull/${number}`).join('\n')}\n`)
}

function writeTree(cwd, version, extras = {}) {
  mkdirSync(path.join(cwd, 'npm'), { recursive: true })
  const lockVersion = extras.lock ?? version
  const nested = extras.nested ?? lockVersion
  const cargo = extras.cargo ?? version
  const cargoLock = extras.cargoLock ?? version
  writeFileSync(path.join(cwd, 'npm', 'package.json'), `${JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version,
  }, null, 2)}\n`)
  writeFileSync(path.join(cwd, 'npm', 'package-lock.json'), `${JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version: lockVersion,
    lockfileVersion: 3,
    packages: { '': { name: '@openma/deepseek-harness-tui', version: nested } },
  }, null, 2)}\n`)
  writeFileSync(path.join(cwd, 'Cargo.toml'), `[package]\nname = "deepseek-harness-tui"\nversion = "${cargo}"\n`)
  writeFileSync(path.join(cwd, 'Cargo.lock'), `[[package]]\nname = "deepseek-harness-tui"\nversion = "${cargoLock}"\n`)
}

function commitFile(cwd, file, contents, message) {
  writeFileSync(path.join(cwd, file), contents)
  commitAll(cwd, message)
}

function commitAll(cwd, message) {
  git(cwd, ['add', '-A'])
  git(cwd, ['commit', '-m', message])
}

function git(cwd, args) {
  const result = spawnSync('git', [
    '-c', 'user.email=release-check@example.com',
    '-c', 'user.name=release-check',
    '-c', 'commit.gpgsign=false',
    ...args,
  ], { cwd, encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
  return result.stdout ?? ''
}

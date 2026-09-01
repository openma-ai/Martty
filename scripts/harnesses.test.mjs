import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const moduleUrl = new URL('../npm/lib/harnesses.js', import.meta.url)

test('named harnesses persist without overwriting existing Martty settings', async () => {
  const module = await import(moduleUrl).catch(() => ({}))
  assert.equal(typeof module.upsertHarness, 'function')
  assert.equal(typeof module.activateHarness, 'function')
  assert.equal(typeof module.selectedHarness, 'function')

  const root = mkdtempSync(path.join(tmpdir(), 'martty-harnesses-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({ language: 'zh', theme: 'ayu' }))
    module.upsertHarness(settingsPath, {
      id: 'local',
      label: 'Local ACP',
      command: '/opt/local/bin/local-acp',
      args: ['--stdio'],
    })
    module.activateHarness(settingsPath, 'local')

    assert.deepEqual(module.selectedHarness(settingsPath), {
      id: 'local',
      label: 'Local ACP',
      command: '/opt/local/bin/local-acp',
      args: ['--stdio'],
    })
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')), {
      language: 'zh',
      theme: 'ayu',
      harnesses: [{
        id: 'local',
        label: 'Local ACP',
        command: '/opt/local/bin/local-acp',
        args: ['--stdio'],
      }],
      activeHarness: 'local',
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('PATH discovery lists executable ACP entrypoints and ignores unrelated commands', async () => {
  const module = await import(moduleUrl)
  assert.equal(typeof module.discoverPathHarnesses, 'function')

  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-path-'))
  try {
    const acp = path.join(root, 'dsh-acp')
    const unrelated = path.join(root, 'helper')
    writeFileSync(acp, '#!/bin/sh\n')
    writeFileSync(unrelated, '#!/bin/sh\n')
    chmodSync(acp, 0o755)
    chmodSync(unrelated, 0o755)

    assert.deepEqual(module.discoverPathHarnesses(root), [{
      id: 'path-dsh-acp',
      label: 'dsh-acp',
      command: acp,
      args: [],
      source: 'path',
    }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness add stores a named command through the CLI surface', async () => {
  const module = await import(moduleUrl)
  assert.equal(typeof module.runHarnessCommand, 'function')

  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-add-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const result = module.runHarnessCommand([
      'add', 'codex', '--label', 'Codex ACP', '--command', 'codex-acp', '--arg', '--stdio',
    ], { settingsPath })
    assert.deepEqual(result, { code: 0, stdout: 'saved harness codex\n', stderr: '' })
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')).harnesses, [{
      id: 'codex',
      label: 'Codex ACP',
      command: 'codex-acp',
      args: ['--stdio'],
    }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness use can persist and activate a discovered local entrypoint', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-use-'))
  const bin = path.join(root, 'bin')
  const settingsPath = path.join(root, 'settings.json')
  try {
    mkdirSync(bin)
    const acp = path.join(bin, 'dsh-acp')
    writeFileSync(acp, '#!/bin/sh\n')
    chmodSync(acp, 0o755)

    const result = module.runHarnessCommand(['use', 'path-dsh-acp'], {
      settingsPath,
      pathValue: bin,
    })
    assert.deepEqual(result, {
      code: 0,
      stdout: 'active harness path-dsh-acp; next standalone launch starts a new session\n',
      stderr: '',
    })
    assert.deepEqual(module.selectedHarness(settingsPath), {
      id: 'path-dsh-acp',
      label: 'dsh-acp',
      command: acp,
      args: [],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness list marks the active entry and reports discovery sources', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-list-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    module.upsertHarness(settingsPath, {
      id: 'local',
      label: 'Local ACP',
      command: 'local-acp',
      args: ['--stdio'],
    })
    module.activateHarness(settingsPath, 'local')

    const result = module.runHarnessCommand(['list'], {
      settingsPath,
      pathValue: '',
      defaults: [{
        id: 'builtin-dsh',
        label: 'Bundled DeepSeek Harness',
        command: '/pkg/dsh-acp',
        args: ['--bundle', '/pkg/creator'],
        source: 'builtin',
      }],
    })
    assert.equal(result.code, 0)
    assert.equal(result.stderr, '')
    assert.match(result.stdout, /^\* local\tconfigured\tLocal ACP\tlocal-acp --stdio$/m)
    assert.match(
      result.stdout,
      /^  builtin-dsh\tbuiltin\tBundled DeepSeek Harness\t\/pkg\/dsh-acp --bundle \/pkg\/creator$/m,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('the packaged martty launcher routes harness subcommands without starting the TUI', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-bin-'))
  const bin = path.resolve(import.meta.dirname, '../npm/bin/martty.js')
  const env = { ...process.env, MARTTY_HOME: root, DSH_TUI_AGENT: '' }
  try {
    const add = spawnSync(process.execPath, [
      bin, 'harness', 'add', 'local', '--command', 'local-acp', '--arg', '--stdio',
    ], { env, encoding: 'utf8', timeout: 5_000 })
    assert.equal(add.status, 0, add.stderr)
    assert.equal(add.stdout, 'saved harness local\n')

    const use = spawnSync(process.execPath, [bin, 'harness', 'use', 'local'], {
      env,
      encoding: 'utf8',
      timeout: 5_000,
    })
    assert.equal(use.status, 0, use.stderr)

    const list = spawnSync(process.execPath, [bin, 'harness', 'list'], {
      env,
      encoding: 'utf8',
      timeout: 5_000,
    })
    assert.equal(list.status, 0, list.stderr)
    assert.match(list.stdout, /^\* local\tconfigured\tlocal\tlocal-acp --stdio$/m)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness help documents discovery, configuration, and selection commands', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['help'], { settingsPath: '/unused/settings.json' })
  assert.equal(result.code, 0)
  assert.equal(result.stderr, '')
  assert.match(result.stdout, /martty harness list/)
  assert.match(result.stdout, /martty harness add <id> --command <cmd>/)
  assert.match(result.stdout, /martty harness use <id>/)
})

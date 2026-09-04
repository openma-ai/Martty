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
  assert.equal(typeof module.setDefaultHarness, 'function')
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
    module.setDefaultHarness(settingsPath, 'local')

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
      defaultHarness: 'local',
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('writing a legacy active Harness migrates it to defaultHarness', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-migrate-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{ id: 'legacy', label: 'Legacy', command: 'legacy-acp', args: [] }],
      activeHarness: 'legacy',
    }))
    module.upsertHarness(settingsPath, {
      id: 'new', label: 'New', command: 'new-acp', args: [],
    })
    const settings = JSON.parse(readFileSync(settingsPath, 'utf8'))
    assert.equal(settings.defaultHarness, 'legacy')
    assert.equal(Object.hasOwn(settings, 'activeHarness'), false)
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
    assert.equal(result.code, 0)
    assert.equal(result.stderr, '')
    assert.match(result.stdout, /^Saved Codex ACP$/m)
    assert.match(result.stdout, /^  Next     martty harness use codex$/m)
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

test('harness add without a Harness shows guided setup instead of validator errors', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['add'], {
    settingsPath: '/unused/settings.json',
  })

  assert.equal(result.code, 0)
  assert.equal(result.stderr, '')
  assert.match(result.stdout, /^Add a Harness$/m)
  assert.match(result.stdout, /^Find an installed ACP Harness$/m)
  assert.match(result.stdout, /martty harness find/)
  assert.match(result.stdout, /^Custom ACP command$/m)
  assert.match(result.stdout, /martty harness add <id> --command <cmd>/)
  assert.deepEqual(
    module.runHarnessCommand(['add', '--help'], {
      settingsPath: '/unused/settings.json',
    }),
    result,
  )
  assert.deepEqual(
    module.runHarnessCommand(['add', 'codex', '--help'], {
      settingsPath: '/unused/settings.json',
    }),
    result,
  )
})

test('harness add uses the registry npx fallback when a known Harness is missing locally', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-codex-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const result = module.runHarnessCommand(['add', 'codex'], { settingsPath, pathValue: '' })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^Saved Codex Harness$/m)
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')).harnesses, [{
      id: 'codex',
      label: 'Codex Harness',
      command: 'npx',
      args: ['@agentclientprotocol/codex-acp'],
    }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('registry commands resolve to a named local Harness before PATH fallback entries', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-registry-local-'))
  const settingsPath = path.join(root, 'settings.json')
  const bin = path.join(root, 'bin')
  try {
    mkdirSync(bin)
    const command = path.join(bin, 'demo-acp')
    writeFileSync(command, '#!/bin/sh\n')
    chmodSync(command, 0o755)
    const result = module.runHarnessCommand(['find', 'demo'], {
      settingsPath,
      pathValue: bin,
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        commands: [{ command: 'demo-acp', args: ['--stdio'] }],
        install: { command: 'npx', args: ['demo-harness-acp'] },
      }],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^ACP Harness candidates \(1\)$/m)
    assert.match(result.stdout, /^  ○ Demo Harness$/m)
    assert.match(result.stdout, /^    Status   Found locally$/m)
    assert.ok(result.stdout.includes(`    Command  ${command}`))
    assert.match(result.stdout, /^    Use      martty harness use demo$/m)
    assert.doesNotMatch(result.stdout, /path-demo-acp/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('registry find prints the recommended npx install command when no local binary exists', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-registry-missing-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const result = module.runHarnessCommand(['find', 'demo'], {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        commands: [{ command: 'demo-acp', args: [] }],
        install: { command: 'npx', args: ['demo-harness-acp'] },
      }],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^ACP Harness candidates \(1\)$/m)
    assert.match(result.stdout, /^    Status   Not installed$/m)
    assert.match(result.stdout, /^    Install  npx demo-harness-acp$/m)
    assert.match(result.stdout, /^    After    martty harness find demo$/m)
    assert.match(result.stdout, /martty harness add demo --command <cmd>/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('non-npx registry install commands are guidance, not ACP launch commands', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-registry-non-npx-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const result = module.runHarnessCommand(['find', 'brew-demo'], {
      settingsPath,
      pathValue: '',
      registry: [{
        id: 'brew-demo',
        label: 'Brew Demo Harness',
        commands: [{ command: 'brew-demo-acp', args: [] }],
        install: { command: 'brew', args: ['install', 'brew-demo-acp'] },
      }],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^    Install  brew install brew-demo-acp$/m)
    assert.match(result.stdout, /^    After    martty harness find brew-demo$/m)
    assert.match(result.stdout, /^    Manual   martty harness add brew-demo --command <cmd>$/m)
    assert.doesNotMatch(result.stdout, /^    Config/m)
    assert.throws(
      () => module.runHarnessCommand(['add', 'brew-demo'], {
        settingsPath,
        pathValue: '',
        registry: [{
          id: 'brew-demo',
          label: 'Brew Demo Harness',
          commands: [{ command: 'brew-demo-acp', args: [] }],
          install: { command: 'brew', args: ['install', 'brew-demo-acp'] },
        }],
      }),
      (error) => {
        assert.equal(error.exitCode, 2)
        assert.match(error.message, /brew install brew-demo-acp/)
        assert.match(error.message, /martty harness add brew-demo --command <cmd>/)
        return true
      },
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness find searches registry labels and package arguments by all query terms', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['find', 'agent', 'client'], {
    settingsPath: '/unused/settings.json',
    pathValue: '',
    registry: [{
      id: 'demo',
      label: 'Demo Harness',
      commands: [{ command: 'demo-acp', args: [] }],
      install: { command: 'npx', args: ['@agentclientprotocol/demo-acp'] },
    }],
  })
  assert.equal(result.code, 0)
  assert.match(result.stdout, /^ACP Harness candidates \(1\)$/m)
  assert.match(result.stdout, /^  ○ Demo Harness$/m)
})

test('registry Harness add saves the resolved local command without asking for a path', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-registry-add-'))
  const settingsPath = path.join(root, 'settings.json')
  const bin = path.join(root, 'bin')
  try {
    mkdirSync(bin)
    const command = path.join(bin, 'demo-acp')
    writeFileSync(command, '#!/bin/sh\n')
    chmodSync(command, 0o755)
    const result = module.runHarnessCommand(['add', 'demo'], {
      settingsPath,
      pathValue: bin,
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        commands: [{ command: 'demo-acp', args: ['--stdio'] }],
        install: { command: 'npx', args: ['demo-harness-acp'] },
      }],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^Saved Demo Harness$/m)
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')).harnesses, [{
      id: 'demo',
      label: 'Demo Harness',
      command,
      args: ['--stdio'],
    }])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness find lists executable ACP candidates and provides the use command', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const bin = path.join(root, 'bin')
    mkdirSync(bin)
    const acp = path.join(bin, 'local-acp')
    writeFileSync(acp, '#!/bin/sh\n')
    chmodSync(acp, 0o755)
    const result = module.runHarnessCommand(['find'], {
      settingsPath,
      pathValue: bin,
      registry: [],
      defaults: [],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^ACP Harness candidates \(1\)$/m)
    assert.match(result.stdout, /^  ○ local-acp$/m)
    assert.ok(result.stdout.includes(`    Command  ${acp}`))
    assert.match(result.stdout, /martty harness use path-local-acp/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('custom harness add errors show a copyable command and use exit code 2', async () => {
  const module = await import(moduleUrl)

  assert.throws(
    () => module.runHarnessCommand(['add', 'local'], {
      settingsPath: '/unused/settings.json',
    }),
    (error) => {
      assert.equal(error.exitCode, 2)
      assert.match(error.message, /Missing --command for custom Harness "local"\./)
      assert.match(error.message, /martty harness add local --command <cmd>/)
      assert.doesNotMatch(error.message, /non-empty string/)
      return true
    },
  )
})

test('harness use can persist and set a discovered local entrypoint as default', async () => {
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
      stdout: 'default harness path-dsh-acp; next standalone launch starts a new session\n',
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

test('harness list marks the default entry and reports discovery sources', async () => {
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
    module.setDefaultHarness(settingsPath, 'local')

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
    assert.match(result.stdout, /^Harnesses \(2\)$/m)
    assert.match(result.stdout, /^  ● Local ACP$/m)
    assert.match(result.stdout, /^    ID       local$/m)
    assert.match(result.stdout, /^    Source   Settings$/m)
    assert.match(result.stdout, /^    Command  local-acp$/m)
    assert.match(result.stdout, /^    Args     --stdio$/m)
    assert.match(result.stdout, /^  ○ Bundled DeepSeek Harness$/m)
    assert.match(result.stdout, /^    ID       builtin-dsh$/m)
    assert.match(result.stdout, /^    Source   Bundled$/m)
    assert.match(result.stdout, /^    Command  \/pkg\/dsh-acp$/m)
    assert.match(result.stdout, /^    Args     --bundle \/pkg\/creator$/m)
    assert.match(result.stdout, /^  Default  local$/m)
    assert.match(result.stdout, /^  Set      martty harness use <id>$/m)
    assert.match(result.stdout, /^  In TUI   \/harness$/m)
    assert.doesNotMatch(result.stdout, /\t/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('a forced Harness takes precedence over a saved recipe with the same id', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-forced-list-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    module.upsertHarness(settingsPath, {
      id: 'codex', label: 'Saved Codex', command: 'saved-codex', args: [],
    })
    const entries = module.discoverHarnesses(settingsPath, {
      pathValue: '',
      defaults: [{
        id: 'codex', label: 'Forced Codex', command: 'forced-codex', args: [], source: 'forced',
      }],
    })
    assert.deepEqual(entries, [{
      id: 'codex', label: 'Forced Codex', command: 'forced-codex', args: [], source: 'forced',
    }])
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
    assert.match(add.stdout, /^Saved local$/m)
    assert.match(add.stdout, /^  Next     martty harness use local$/m)

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
    assert.match(list.stdout, /^Harnesses \(/m)
    assert.match(list.stdout, /^  ● local$/m)

    for (const alias of ['-h', '--help']) {
      const help = spawnSync(process.execPath, [bin, 'harness', alias], {
        env,
        encoding: 'utf8',
        timeout: 5_000,
      })
      assert.equal(help.status, 0, help.stderr)
      assert.match(help.stdout, /^Martty Harnesses$/m)
    }

    const addHelp = spawnSync(process.execPath, [bin, 'harness', 'add'], {
      env,
      encoding: 'utf8',
      timeout: 5_000,
    })
    assert.equal(addHelp.status, 0, addHelp.stderr)
    assert.match(addHelp.stdout, /^Add a Harness$/m)

    const incomplete = spawnSync(process.execPath, [bin, 'harness', 'add', 'custom'], {
      env,
      encoding: 'utf8',
      timeout: 5_000,
    })
    assert.equal(incomplete.status, 2)
    assert.match(incomplete.stderr, /Missing --command/)
    assert.match(incomplete.stderr, /martty harness add custom --command <cmd>/)
    assert.doesNotMatch(incomplete.stderr, /non-empty string/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('harness help aliases render structured commands, examples, and session guidance', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['help'], { settingsPath: '/unused/settings.json' })
  assert.equal(result.code, 0)
  assert.equal(result.stderr, '')
  assert.match(result.stdout, /^Martty Harnesses$/m)
  assert.match(result.stdout, /^Usage$/m)
  assert.match(result.stdout, /^Commands$/m)
  assert.match(result.stdout, /^Examples$/m)
  assert.match(result.stdout, /martty harness add local --label "Local ACP"/)
  assert.match(result.stdout, /starts a\s+new ACP session/)
  for (const alias of ['-h', '--help']) {
    assert.deepEqual(
      module.runHarnessCommand([alias], { settingsPath: '/unused/settings.json' }),
      result,
    )
  }
})

test('harness list fits long commands to the terminal width', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['list'], {
    settingsPath: '/unused/settings.json',
    pathValue: '',
    columns: 52,
    cwd: '/a/very/long/package/location',
    defaults: [{
      id: 'builtin-dsh',
      label: 'Bundled DeepSeek Harness',
      command: '/a/very/long/package/location/deepseek-harness-acp/dist/bin.js',
      args: ['--bundle', '/another/very/long/creator/location'],
      source: 'builtin',
    }],
  })
  assert.match(result.stdout, /…/)
  assert.match(result.stdout, /\.\/deepseek-harness-acp\/dist\/bin\.js/)
  for (const line of result.stdout.trimEnd().split('\n')) {
    assert.ok(line.length <= 52, `${line.length}: ${line}`)
  }
})

test('empty harness discovery gives an actionable next step', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['list'], {
    settingsPath: '/unused/settings.json',
    pathValue: '',
    columns: 80,
  })
  assert.match(result.stdout, /^No Harnesses found\.$/m)
  assert.match(result.stdout, /martty harness add <id> --command <cmd>/)
})

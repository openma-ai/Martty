import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import fs, { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { once } from 'node:events'
import test from 'node:test'

const moduleUrl = new URL('../npm/lib/harnesses.js', import.meta.url)
const registryModuleUrl = new URL('../npm/lib/harness-registry.js', import.meta.url)

test('CLI saved add is offline and preserves its recipe without reinstallation', async t => {
  const api = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-saved-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  api.upsertHarness(settingsPath, { id: 'saved', label: 'Saved', command: '/existing/acp', args: ['--stdio'] })
  const before = readFileSync(settingsPath, 'utf8')
  const result = await api.runHarnessCommandAsync(['add', 'saved'], {
    settingsPath, fetchRegistry() { assert.fail('saved recipe must not fetch Registry') },
  })
  assert.equal(result.code, 0)
  assert.equal(readFileSync(settingsPath, 'utf8'), before)
})

test('CLI find uses its bundled snapshot immediately and refresh failure keeps that catalog', async t => {
  const api = await import(moduleUrl)
  const { readAcpRegistrySnapshot } = await import(registryModuleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-offline-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  const id = readAcpRegistrySnapshot({ settingsPath }).find(entry => entry.distributions.some(d => d.type === 'npx')).id
  let requests = 0
  const options = { settingsPath, pathValue: '', defaults: [], fetchRegistry() { requests++; throw new Error('offline fixture') } }
  const local = await api.runHarnessCommandAsync(['find', id], options)
  assert.equal(requests, 0)
  assert.match(local.stdout, new RegExp(id))
  const refreshed = await api.runHarnessCommandAsync(['find', id, '--refresh'], options)
  assert.equal(requests, 1)
  assert.match(refreshed.stdout, new RegExp(id))
  assert.match(refreshed.stderr, /offline fixture/)
})

test('CLI rejects malformed options before Registry requests or settings writes', async t => {
  const api = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-args-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  api.upsertHarness(settingsPath, { id: 'saved', command: '/existing/acp' })
  const before = readFileSync(settingsPath, 'utf8')
  for (const argv of [['add', 'saved', '--bogus', 'x'], ['add', 'new', '--label'], ['add', 'saved', '--command', ''],
    ['list', '--bogus'], ['use', 'saved', 'extra'], ['find', '--bogus']]) {
    await assert.rejects(api.runHarnessCommandAsync(argv, {
      settingsPath, fetchRegistry() { assert.fail('validation must precede network') },
    }), error => error.exitCode === 2, argv.join(' '))
    assert.equal(readFileSync(settingsPath, 'utf8'), before)
  }
})

test('CLI removal previews exact paths and requires confirmation before cleaning private files', async t => {
  const api = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-remove-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, '.martty', 'settings.json')
  const resource = path.join(root, '.martty', 'bin', 'agent', '1.0', 'darwin-arm64')
  mkdirSync(resource, { recursive: true })
  const command = path.join(resource, 'agent')
  writeFileSync(command, 'fixture')
  api.upsertHarness(settingsPath, { id: 'agent', command }); api.setDefaultHarness(settingsPath, 'agent')
  const options = { settingsPath, pathValue: '', defaults: [] }
  const preview = await api.runHarnessCommandAsync(['remove', 'agent', '--cleanup'], options)
  assert.equal(preview.code, 2)
  assert.ok(preview.stdout.includes(resource))
  assert.ok(preview.stdout.includes(settingsPath))
  assert.ok(existsSync(command))
  const cancelled = await api.runHarnessCommandAsync(['remove', 'agent', '--cleanup'], {
    ...options, confirmRemoval: async () => false,
  })
  assert.equal(cancelled.code, 0); assert.ok(existsSync(command))
  const done = await api.runHarnessCommandAsync(['remove', 'agent', '--cleanup', '--yes'], options)
  assert.equal(done.code, 0)
  assert.ok(!existsSync(resource))
  assert.deepEqual(JSON.parse(readFileSync(settingsPath)), { harnesses: [] })
})

test('CLI binary add validates before downloading and preserves label and argument overrides', async t => {
  const api = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-binary-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  let downloads = 0
  const options = { settingsPath, pathValue: '', registry: [{
    id: 'fixture', label: 'Original', version: '1', distributions: [{
      type: 'binary', target: 'darwin-aarch64', command: './fixture', args: ['default'], env: {},
      archive: 'https://example.test/fixture.tar.gz',
    }],
  }], downloadFile: async (_url, dest) => { downloads++; writeFileSync(dest, 'fixture') },
  extractArchive(_archive, dest) { writeFileSync(path.join(dest, 'fixture'), 'fixture'); chmodSync(path.join(dest, 'fixture'), 0o755) } }
  await assert.rejects(api.runHarnessCommandAsync(['add', 'fixture', '--bogus', 'x'], options), /Unknown add option/)
  assert.equal(downloads, 0)
  await api.runHarnessCommandAsync(['add', 'fixture', '--label', 'Custom', '--arg', '--stdio'], options)
  const entry = JSON.parse(readFileSync(settingsPath)).harnesses[0]
  assert.equal(entry.label, 'Custom'); assert.deepEqual(entry.args, ['--stdio'])
  assert.equal(entry.command, path.join(root, 'bin', 'fixture', '1', 'darwin-aarch64', 'fixture'))
  await api.runHarnessCommandAsync(['add', 'fixture'], options)
  assert.equal(downloads, 1)
})

test('CLI wrapper runs add/list/use/remove against isolated settings and rejects missing startup values', t => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-wrapper-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const settingsPath = path.join(root, 'settings.json')
  // Do not let a developer's bundled native binary mask early CLI validation.
  mkdirSync(path.join(root, 'bin'))
  fs.copyFileSync(new URL('../npm/bin/martty.js', import.meta.url), path.join(root, 'bin', 'martty.mjs'))
  symlinkSync(new URL('../npm/lib', import.meta.url).pathname, path.join(root, 'lib'), 'junction')
  const cli = (...args) => spawnSync(process.execPath, [path.join(root, 'bin', 'martty.mjs'), ...args], {
    env: { ...process.env, MARTTY_HOME: root, MARTTY_BIN: '', DSH_TUI_BIN: '', DSH_TUI_AGENT: '', PATH: '' }, encoding: 'utf8', timeout: 5000,
  })
  let result = cli('harness', 'add', 'fixture', '--command', '/no-launch/fixture')
  assert.equal(result.status, 0, result.stderr)
  assert.equal(JSON.parse(readFileSync(settingsPath)).defaultHarness, undefined)
  result = cli('harness', 'list'); assert.match(result.stdout, /fixture/)
  result = cli('harness', 'use', 'fixture'); assert.equal(result.status, 0, result.stderr)
  assert.equal(JSON.parse(readFileSync(settingsPath)).defaultHarness, 'fixture')
  result = cli('harness', 'remove', 'fixture'); assert.equal(result.status, 2, result.stderr)
  assert.equal(JSON.parse(readFileSync(settingsPath)).harnesses.length, 1)
  result = cli('--agent'); assert.equal(result.status, 2, result.stderr)
  assert.match(result.stderr, /--agent needs a value/)
  result = cli('harness', 'remove', 'fixture', '--yes'); assert.equal(result.status, 0, result.stderr)
  assert.deepEqual(JSON.parse(readFileSync(settingsPath)), { harnesses: [] })
})

test('CLI Ctrl-C aborts a real binary transfer, clears staging and never saves a recipe', { skip: process.platform === 'win32' }, async t => {
  const { ACP_REGISTRY_URL, registryPlatformKey } = await import(registryModuleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-cancel-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const server = createServer((_req, response) => { response.writeHead(200); response.write('unfinished archive') })
  server.listen(0, '127.0.0.1'); await once(server, 'listening')
  t.after(() => { server.closeAllConnections(); server.close() })
  mkdirSync(path.join(root, 'cache'))
  writeFileSync(path.join(root, 'cache', 'acp-registry.json'), JSON.stringify({ source: ACP_REGISTRY_URL, catalog: { agents: [{
    id: 'fixture', name: 'Fixture', version: '1', distribution: { binary: {
      [registryPlatformKey()]: { cmd: './fixture', archive: `http://127.0.0.1:${server.address().port}/fixture.tar.gz` },
    } },
  }] } }))
  const requested = once(server, 'request')
  const child = spawn(process.execPath, [new URL('../npm/bin/martty.js', import.meta.url).pathname, 'harness', 'add', 'fixture'], {
    env: { ...process.env, MARTTY_HOME: root }, stdio: ['ignore', 'pipe', 'pipe'],
  })
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000)
  t.after(() => { clearTimeout(timer); if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL') })
  let stderr = ''; child.stderr.on('data', data => { stderr += data }); child.stdout.resume()
  const exited = once(child, 'exit')
  await Promise.race([requested, exited.then(() => { throw new Error(`CLI exited before downloading: ${stderr}`) })])
  child.kill('SIGINT')
  const [code] = await exited
  assert.equal(code, 130, stderr)
  assert.match(stderr, /cancelled/i)
  assert.ok(!existsSync(path.join(root, 'settings.json')))
  assert.ok(!fs.readdirSync(root, { recursive: true }).some(file => file.includes('.install-')))
})

test('CLI installs a real archive from cached Registry, reports progress, and reuses it offline', { skip: process.platform === 'win32' }, async t => {
  const { ACP_REGISTRY_URL, registryPlatformKey } = await import(registryModuleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-cli-install-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'source'), home = path.join(root, '.martty')
  mkdirSync(source); mkdirSync(path.join(home, 'cache'), { recursive: true })
  writeFileSync(path.join(source, 'fixture'), '#!/bin/sh\nexit 0\n'); chmodSync(path.join(source, 'fixture'), 0o755)
  const archivePath = path.join(root, 'fixture.tar.gz')
  const tar = spawnSync('tar', ['-czf', archivePath, '-C', source, 'fixture'], { encoding: 'utf8' })
  assert.equal(tar.status, 0, tar.stderr)
  const archive = readFileSync(archivePath)
  let requests = 0
  const server = createServer((_req, response) => { requests++; response.writeHead(200, { 'Content-Length': archive.length }); response.end(archive) })
  server.listen(0, '127.0.0.1'); await once(server, 'listening')
  t.after(() => { server.closeAllConnections(); server.close() })
  writeFileSync(path.join(home, 'cache', 'acp-registry.json'), JSON.stringify({ source: ACP_REGISTRY_URL, catalog: { agents: [{
    id: 'fixture', name: 'Fixture', version: '1', distribution: { binary: {
      [registryPlatformKey()]: { cmd: './fixture', archive: `http://127.0.0.1:${server.address().port}/fixture.tar.gz`, sha256: createHash('sha256').update(archive).digest('hex') },
    } },
  }] } }))
  const cli = (...args) => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [new URL('../npm/bin/martty.js', import.meta.url).pathname, 'harness', ...args], {
      env: { ...process.env, MARTTY_HOME: home }, stdio: ['ignore', 'pipe', 'pipe'], timeout: 5000,
    })
    let stdout = '', stderr = ''
    child.stdout.on('data', data => { stdout += data }); child.stderr.on('data', data => { stderr += data })
    child.on('error', reject); child.on('close', code => resolve({ code, stdout, stderr }))
  })
  const installed = await cli('add', 'fixture', '--label', 'Local Fixture', '--arg', '--stdio')
  assert.equal(installed.code, 0, installed.stderr)
  assert.match(installed.stderr, /Harness install: extract/)
  assert.match(installed.stderr, /Harness install: complete/)
  const settingsPath = path.join(home, 'settings.json'), before = readFileSync(settingsPath, 'utf8')
  const settings = JSON.parse(before), entry = settings.harnesses[0]
  assert.equal(settings.defaultHarness, undefined)
  assert.equal(entry.label, 'Local Fixture'); assert.deepEqual(entry.args, ['--stdio'])
  assert.equal(readFileSync(entry.command, 'utf8'), '#!/bin/sh\nexit 0\n')
  server.closeAllConnections(); await new Promise(resolve => server.close(resolve))
  const repeated = await cli('add', 'fixture')
  assert.equal(repeated.code, 0, repeated.stderr)
  assert.equal(repeated.stderr, '')
  assert.equal(readFileSync(settingsPath, 'utf8'), before)
  assert.equal(requests, 1)
})

test('official ACP Registry distributions normalize for the current platform', async () => {
  const registry = await import(registryModuleUrl).catch(() => ({}))
  assert.equal(typeof registry.normalizeAcpRegistry, 'function')
  assert.equal(registry.registryPlatformKey('win32', 'x64'), 'windows-x86_64')
  assert.equal(registry.registryPlatformKey('win32', 'arm64'), 'windows-aarch64')

  const records = registry.normalizeAcpRegistry({
    version: '1.0.0',
    agents: [
      {
        id: 'codex-acp',
        name: 'Codex',
        version: '1.9.0',
        description: 'ACP adapter for Codex',
        distribution: {
          npx: { package: '@agentclientprotocol/codex-acp@1.9.0' },
        },
      },
      {
        id: 'python-agent',
        name: 'Python Agent',
        version: '2.1.0',
        distribution: {
          uvx: { package: 'python-agent@2.1.0', args: ['--acp'] },
        },
      },
      {
        id: 'amp-acp',
        name: 'Amp',
        version: '0.9.0',
        distribution: {
          binary: {
            'darwin-aarch64': {
              archive: 'https://example.test/amp-acp.tar.gz',
              cmd: './amp-acp',
              args: ['serve'],
              sha256: 'a'.repeat(64),
            },
          },
        },
      },
    ],
  }, { platform: 'darwin', arch: 'arm64' })

  assert.deepEqual(records, [
    {
      id: 'codex-acp',
      label: 'Codex',
      version: '1.9.0',
      description: 'ACP adapter for Codex',
      distributions: [{
        type: 'npx',
        command: 'npx',
        args: ['@agentclientprotocol/codex-acp@1.9.0'],
        env: {},
      }],
    },
    {
      id: 'python-agent',
      label: 'Python Agent',
      version: '2.1.0',
      description: '',
      distributions: [{
        type: 'uvx',
        command: 'uvx',
        args: ['python-agent@2.1.0', '--acp'],
        env: {},
      }],
    },
    {
      id: 'amp-acp',
      label: 'Amp',
      version: '0.9.0',
      description: '',
      distributions: [{
        type: 'binary',
        target: 'darwin-aarch64',
        command: './amp-acp',
        args: ['serve'],
        env: {},
        archive: 'https://example.test/amp-acp.tar.gz',
        sha256: 'a'.repeat(64),
      }],
    },
  ])
  assert.doesNotMatch(records[0].distributions[0].args.join(' '), /--yes|--prefer-offline/)
})

test('ACP Registry loader fetches the official catalog and filters locally', async () => {
  const registry = await import(registryModuleUrl).catch(() => ({}))
  assert.equal(typeof registry.fetchAcpRegistry, 'function')
  const requested = []
  const records = await registry.fetchAcpRegistry({
    platform: 'linux',
    arch: 'x64',
    fetchImpl: async (url) => {
      requested.push(url)
      return {
        ok: true,
        async json() {
          return {
            version: '1.0.0',
            agents: [{
              id: 'demo-agent',
              name: 'Demo Agent',
              version: '1.0.0',
              distribution: { npx: { package: 'demo-agent@1.0.0' } },
            }],
          }
        },
      }
    },
  })
  assert.deepEqual(requested, [registry.ACP_REGISTRY_URL])
  assert.equal(records[0].id, 'demo-agent')
})

test('official binary distributions install under the Martty home bin directory', async () => {
  const registry = await import(registryModuleUrl).catch(() => ({}))
  assert.equal(typeof registry.installRegistryBinary, 'function')
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-binary-'))
  const settingsPath = path.join(root, 'settings.json')
  const archive = Buffer.from('fake archive bytes')
  const sha256 = createHash('sha256').update(archive).digest('hex')
  try {
    const installed = await registry.installRegistryBinary({
      id: 'amp-acp',
      label: 'Amp',
      version: '0.9.0',
      distribution: {
        type: 'binary',
        target: 'darwin-aarch64',
        command: './amp-acp',
        args: ['serve'],
        env: {},
        archive: 'https://example.test/amp-acp.tar.gz',
        sha256,
      },
    }, {
      settingsPath,
      downloadFile: async (_url, destination) => writeFileSync(destination, archive),
      extractArchive(_archivePath, destination) {
        const executable = path.join(destination, 'amp-acp')
        writeFileSync(executable, '#!/bin/sh\n')
        chmodSync(executable, 0o755)
      },
    })

    assert.equal(installed.command, path.join(
      root, 'bin', 'amp-acp', '0.9.0', 'darwin-aarch64', 'amp-acp',
    ))
    assert.deepEqual(installed.args, ['serve'])
    assert.equal(readFileSync(installed.command, 'utf8'), '#!/bin/sh\n')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('async harness add installs and persists an ACP Registry binary', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-async-binary-'))
  const settingsPath = path.join(root, 'settings.json')
  const archive = Buffer.from('async fake archive')
  const sha256 = createHash('sha256').update(archive).digest('hex')
  try {
    const result = await module.runHarnessCommandAsync(['add', 'amp-acp'], {
      settingsPath,
      registry: [{
        id: 'amp-acp',
        label: 'Amp',
        version: '0.9.0',
        description: '',
        distributions: [{
          type: 'binary',
          target: 'darwin-aarch64',
          command: './amp-acp',
          args: [],
          env: {},
          archive: 'https://example.test/amp-acp.tar.gz',
          sha256,
        }],
      }],
      downloadFile: async (_url, destination) => writeFileSync(destination, archive),
      extractArchive(_archivePath, destination) {
        const executable = path.join(destination, 'amp-acp')
        writeFileSync(executable, '#!/bin/sh\n')
        chmodSync(executable, 0o755)
      },
    })
    assert.match(result.stdout, /^Saved Amp$/m)
    const saved = JSON.parse(readFileSync(settingsPath, 'utf8')).harnesses[0]
    assert.equal(saved.command, path.join(
      root, 'bin', 'amp-acp', '0.9.0', 'darwin-aarch64', 'amp-acp',
    ))
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

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

test('legacy npx flags are removed when a saved Harness is read', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-npx-flags-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'codex-acp',
        label: 'Codex',
        command: 'npx',
        args: ['--yes', '--prefer-offline', '@agentclientprotocol/codex-acp@1.9.0'],
      }],
      defaultHarness: 'codex-acp',
    }))
    assert.deepEqual(module.selectedHarness(settingsPath), {
      id: 'codex-acp',
      label: 'Codex',
      command: 'npx',
      args: ['@agentclientprotocol/codex-acp@1.9.0'],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('legacy npx flags are removed from resolved Unix and Windows runner paths', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-resolved-npx-flags-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    for (const command of ['/opt/node/bin/npx', 'C:\\Program Files\\nodejs\\npx.cmd']) {
      const saved = module.upsertHarness(settingsPath, {
        id: 'legacy', command, args: ['--yes', '--prefer-offline', 'agent-package@1.0.0'],
      })
      assert.deepEqual(saved.args, ['agent-package@1.0.0'])
    }
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

test('Windows Registry package discovery resolves npx.cmd and uvx.exe through PATHEXT', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-windows-path-'))
  try {
    for (const command of ['npx.cmd', 'uvx.exe']) {
      const executable = path.join(root, command)
      writeFileSync(executable, '@echo off\r\n')
      chmodSync(executable, 0o755)
    }
    const result = module.runHarnessCommand(['find'], {
      settingsPath: path.join(root, 'settings.json'),
      platform: 'win32',
      pathExt: '.cmd;.exe',
      pathValue: root,
      defaults: [],
      registry: [
        {
          id: 'node-agent',
          label: 'Node Agent',
          version: '1.0.0',
          distributions: [{
            type: 'npx', command: 'npx', args: ['node-agent@1.0.0'], env: {},
          }],
        },
        {
          id: 'python-agent',
          label: 'Python Agent',
          version: '1.0.0',
          distributions: [{
            type: 'uvx', command: 'uvx', args: ['python-agent@1.0.0'], env: {},
          }],
        },
      ],
    })

    assert.match(result.stdout, /^  ○ Node Agent$/m)
    assert.match(result.stdout, /^    Status   Available via npx$/m)
    assert.match(result.stdout, /^  ○ Python Agent$/m)
    assert.match(result.stdout, /^    Status   Available via uvx$/m)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Registry package discovery reuses the executable declared by installed npm metadata', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-installed-npm-'))
  const settingsPath = path.join(root, 'settings.json')
  const bin = path.join(root, 'bin')
  const packageRoot = path.join(root, 'lib', 'node_modules', '@vendor', 'agent-package')
  try {
    mkdirSync(bin)
    mkdirSync(path.join(packageRoot, 'dist'), { recursive: true })
    const entrypoint = path.join(packageRoot, 'dist', 'server.js')
    writeFileSync(entrypoint, '#!/usr/bin/env node\n')
    chmodSync(entrypoint, 0o755)
    writeFileSync(path.join(packageRoot, 'package.json'), JSON.stringify({
      name: '@vendor/agent-package', version: '1.2.3', bin: { 'unrelated-server-name': 'dist/server.js' },
    }))
    const command = path.join(bin, 'unrelated-server-name')
    symlinkSync(entrypoint, command)
    const options = {
      settingsPath, pathValue: bin,
      registry: [{
        id: 'registry-agent', label: 'Registry Agent', version: '1.2.3',
        distributions: [{
          type: 'npx', command: 'npx', args: ['@vendor/agent-package@1.2.3', '--stdio'],
          env: { ACP_MODE: '1' },
        }],
      }],
    }
    const [candidate] = module.discoverHarnessCandidates(settingsPath, options)
    assert.equal(candidate.status, 'Found locally')
    assert.equal(candidate.resolvedCommand, command)
    assert.deepEqual(candidate.args, ['--stdio'])
    const saved = module.addHarness(settingsPath, 'registry-agent', [], options)
    assert.equal(saved.command, command)
    assert.deepEqual(saved.args, ['--stdio'])
    assert.deepEqual(saved.env, { ACP_MODE: '1' })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Registry package discovery verifies PATH links belong to the requested npm package', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-npm-identity-'))
  const bin = path.join(root, 'bin')
  const packageRoot = path.join(root, 'lib', 'node_modules', 'agent-package')
  try {
    mkdirSync(bin)
    mkdirSync(packageRoot, { recursive: true })
    writeFileSync(path.join(packageRoot, 'package.json'), JSON.stringify({
      name: 'agent-package', version: '1.0.0', bin: { 'agent-server': 'server.js' },
    }))
    writeFileSync(path.join(packageRoot, 'server.js'), '#!/usr/bin/env node\n')
    chmodSync(path.join(packageRoot, 'server.js'), 0o755)
    const unrelated = path.join(root, 'unrelated.js')
    writeFileSync(unrelated, '#!/usr/bin/env node\n')
    chmodSync(unrelated, 0o755)
    symlinkSync(unrelated, path.join(bin, 'agent-server'))
    const [candidate] = module.discoverHarnessCandidates(path.join(root, 'settings.json'), {
      pathValue: bin,
      registry: [{
        id: 'registry-agent', label: 'Registry Agent', version: '1.0.0',
        distributions: [{ type: 'npx', command: 'npx', args: ['agent-package@1.0.0'], env: {} }],
      }],
    })
    assert.equal(candidate.status, 'Needs npx')
    assert.equal(candidate.resolvedCommand, undefined)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Registry package discovery reuses installed uv tools through Python entrypoint metadata', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-installed-uv-'))
  const settingsPath = path.join(root, 'settings.json')
  const bin = path.join(root, 'bin')
  const toolRoot = path.join(root, 'tools', 'python-agent')
  const metadata = path.join(toolRoot, 'lib', 'python3.12', 'site-packages', 'python_agent-1.0.0.dist-info')
  try {
    mkdirSync(bin)
    mkdirSync(path.join(toolRoot, 'bin'), { recursive: true })
    mkdirSync(metadata, { recursive: true })
    writeFileSync(path.join(toolRoot, 'pyvenv.cfg'), 'home = /usr/bin\n')
    writeFileSync(path.join(metadata, 'METADATA'), 'Metadata-Version: 2.3\r\nName: Python_Agent\r\nVersion: 1.0.0\r\n')
    writeFileSync(path.join(metadata, 'entry_points.txt'), '[console_scripts]\r\nstrange-server = python_agent.main:cli\r\n')
    const entrypoint = path.join(toolRoot, 'bin', 'strange-server')
    writeFileSync(entrypoint, '#!/usr/bin/env python\nfrom python_agent.main import cli\ncli()\n')
    chmodSync(entrypoint, 0o755)
    const command = path.join(bin, 'strange-server')
    symlinkSync(entrypoint, command)
    const options = {
      settingsPath, pathValue: bin,
      registry: [{
        id: 'registry-agent', label: 'Registry Agent', version: '1.0.0',
        distributions: [{
          type: 'uvx', command: 'uvx', args: ['python-agent@1.0.0', '--stdio'], env: { ACP_MODE: '1' },
        }],
      }],
    }
    const [candidate] = module.discoverHarnessCandidates(settingsPath, options)
    assert.equal(candidate.status, 'Found locally')
    assert.equal(candidate.resolvedCommand, command)
    const saved = module.addHarness(settingsPath, 'registry-agent', [], options)
    assert.equal(saved.command, command)
    assert.deepEqual(saved.args, ['--stdio'])
    assert.deepEqual(saved.env, { ACP_MODE: '1' })

    // Installed dependencies may also have console scripts. A same-named
    // executable is not enough without matching distribution metadata.
    writeFileSync(path.join(metadata, 'METADATA'), 'Name: unrelated-agent\nVersion: 1.0.0\n')
    assert.equal(module.discoverRegistryHarnesses(options).length, 0)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

function tracedPackageFixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-discovery-scan-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const bin = path.join(root, 'bin')
  const tool = path.join(root, 'tools', 'python-agent')
  const metadata = path.join(tool, 'lib', 'python3.12', 'site-packages', 'python_agent-1.dist-info')
  const npmPackage = path.join(root, 'lib', 'node_modules', 'target-agent')
  for (const directory of [bin, path.join(tool, 'bin'), metadata, npmPackage]) mkdirSync(directory, { recursive: true })
  writeFileSync(path.join(tool, 'pyvenv.cfg'), 'home = /usr/bin\n')
  writeFileSync(path.join(metadata, 'METADATA'), 'Name: python-agent\nVersion: 1\n')
  writeFileSync(path.join(metadata, 'entry_points.txt'), '[console_scripts]\nlocal-python-server = python_agent.main:cli\n')
  writeFileSync(path.join(npmPackage, 'package.json'), JSON.stringify({ name: 'target-agent', version: '1', bin: 'server.js' }))
  for (const [target, name] of [[path.join(tool, 'bin', 'local-python-server'), 'local-python-server'], [path.join(npmPackage, 'server.js'), 'local-node-server']]) {
    writeFileSync(target, '#!/bin/sh\n')
    chmodSync(target, 0o755)
    symlinkSync(target, path.join(bin, name))
  }
  const reads = []
  const original = fs.readFileSync
  const read = t.mock.method(fs, 'readFileSync', (...args) => {
    reads.push(String(args[0]))
    return original(...args)
  })
  syncBuiltinESMExports()
  t.after(() => { read.mock.restore(); syncBuiltinESMExports() })
  return {
    reads, metadata: fs.realpathSync(path.join(metadata, 'METADATA')), npmMetadata: path.join(npmPackage, 'package.json'),
    settingsPath: path.join(root, 'settings.json'),
    options: { pathValue: bin, persist: false, registry: [
      { id: 'target', label: 'Target', distributions: [{ type: 'npx', command: 'npx', args: ['target-agent@1'], env: {} }] },
      { id: 'python', label: 'Python', distributions: [{ type: 'uvx', command: 'uvx', args: ['python-agent@1', '--stdio'], env: {} }] },
    ] },
  }
}

test('one candidate discovery scans Registry package metadata only once, even without runners', async (t) => {
  const module = await import(moduleUrl)
  const fixture = tracedPackageFixture(t)
  const entries = module.discoverHarnessCandidates(fixture.settingsPath, fixture.options)
  assert.equal(entries.find(({ id }) => id === 'python').status, 'Found locally')
  assert.equal(entries.find(({ id }) => id === 'target').status, 'Found locally')
  assert.equal(fixture.reads.filter((file) => file === fixture.metadata).length, 1)
  assert.equal(fixture.reads.filter((file) => file === fixture.npmMetadata).length, 1)
})

for (const method of ['addHarness', 'addHarnessAsync']) {
  test(`${method} inspects only the requested Registry record`, async (t) => {
    const module = await import(moduleUrl)
    const fixture = tracedPackageFixture(t)
    const saved = await module[method](fixture.settingsPath, 'target', [], fixture.options)
    assert.match(saved.command, /local-node-server$/)
    assert.equal(fixture.reads.includes(fixture.metadata), false, 'an unrelated uv recipe cannot trigger its environment scan')
    assert.equal(existsSync(fixture.settingsPath), false)
  })
}

test('async Registry add reuses its resolved package candidate', async (t) => {
  const module = await import(moduleUrl)
  const fixture = tracedPackageFixture(t)
  await module.addHarnessAsync(fixture.settingsPath, 'target', [], fixture.options)
  assert.equal(fixture.reads.filter((file) => file === fixture.npmMetadata).length, 1)
})

test('targeted Registry lookup keeps raw catalog, legacy recipe, and invalid-id behavior', async (t) => {
  const module = await import(moduleUrl)
  const fixture = tracedPackageFixture(t)
  const catalogs = [
    { version: '1.0.0', agents: [{ id: 'target', name: 'Target', version: '1', distribution: { npx: { package: 'target-agent@1' } } }] },
    [null, { id: 'ignored', distributions: [] }, { id: 'target', commands: [{ command: 'local-node-server', args: [] }] }],
  ]
  for (const registry of catalogs) {
    const options = { ...fixture.options, registry }
    for (const method of ['addHarness', 'addHarnessAsync']) {
      const saved = await module[method](fixture.settingsPath, 'target', [], options)
      assert.equal(saved.id, 'target')
      assert.match(saved.command, /local-node-server$/)
      await assert.rejects(async () => module[method](fixture.settingsPath, 'invalid id', [], options), /Invalid Harness id/)
      await assert.rejects(async () => module[method](fixture.settingsPath, 'missing', [], options), /Missing --command/)
    }
  }
  assert.equal(existsSync(fixture.settingsPath), false)
})

test('Harness discovery keeps different packages on the same runner and provides valid commands', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-distinct-packages-'))
  try {
    const npx = path.join(root, 'npx')
    writeFileSync(npx, '#!/bin/sh\n')
    chmodSync(npx, 0o755)
    const registry = ['first-agent', 'second-agent'].map((id) => ({
      id, label: id, version: '1.0.0',
      distributions: [{ type: 'npx', command: 'npx', args: [`${id}@1.0.0`], env: {} }],
    }))
    const entries = module.discoverHarnesses(path.join(root, 'settings.json'), { pathValue: root, registry })
    assert.deepEqual(entries.map(({ id }) => id), ['first-agent', 'second-agent'])
    assert.deepEqual(entries.map(({ command }) => command), [npx, npx])
    assert.deepEqual(entries.map(({ status }) => status), ['Available via npx', 'Available via npx'])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Harness discovery deduplicates complete launch recipes, not just commands', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-distinct-recipes-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    for (const harness of [
      { id: 'first', command: 'agent-server', args: ['one'], env: { MODE: 'read' } },
      { id: 'second', command: 'agent-server', args: ['two'], env: { MODE: 'read' } },
      { id: 'third', command: 'agent-server', args: ['two'], env: { MODE: 'write' } },
      { id: 'duplicate', command: 'agent-server', args: ['one'], env: { MODE: 'read' } },
    ]) module.upsertHarness(settingsPath, harness)
    assert.deepEqual(
      module.discoverHarnesses(settingsPath, { pathValue: '' }).map(({ id }) => id),
      ['first', 'second', 'third'],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Registry discovery falls back to a managed binary when its package runner is unavailable', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-binary-fallback-'))
  try {
    const result = module.runHarnessCommand(['find'], {
      settingsPath: path.join(root, 'settings.json'),
      platform: 'win32',
      arch: 'x64',
      pathValue: '',
      defaults: [],
      registry: [{
        id: 'dual-agent',
        label: 'Dual Agent',
        version: '1.0.0',
        distributions: [
          {
            type: 'npx', command: 'npx', args: ['dual-agent@1.0.0'], env: {},
          },
          {
            type: 'binary',
            target: 'windows-x86_64',
            command: './dual-agent.exe',
            args: ['acp'],
            env: {},
            archive: 'https://example.test/dual-agent.zip',
          },
        ],
      }],
    })

    assert.match(result.stdout, /^    Status   Available to install$/m)
    assert.match(result.stdout, /^    Distribution binary$/m)
    assert.match(result.stdout, /^    Install  martty harness add dual-agent$/m)
    assert.doesNotMatch(result.stdout, /Needs npx/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Registry discovery names the runtime that provides a missing package runner', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['find'], {
    settingsPath: '/unused/settings.json',
    pathValue: '',
    defaults: [],
    registry: [
      {
        id: 'node-agent',
        label: 'Node Agent',
        version: '1.0.0',
        distributions: [{
          type: 'npx', command: 'npx', args: ['node-agent@1.0.0'], env: {},
        }],
      },
      {
        id: 'python-agent',
        label: 'Python Agent',
        version: '1.0.0',
        distributions: [{
          type: 'uvx', command: 'uvx', args: ['python-agent@1.0.0'], env: {},
        }],
      },
    ],
  })

  assert.match(result.stdout, /^    Setup    Install Node\.js\/npm \(provides npx\), then run martty harness add node-agent$/m)
  assert.match(result.stdout, /^    Setup    Install uv \(provides uvx\), then run martty harness add python-agent$/m)
})

test('Registry add refuses to save a package recipe when its runner is unavailable', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-missing-runner-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    await assert.rejects(
      module.runHarnessCommandAsync(['add', 'node-agent'], {
        settingsPath,
        pathValue: '',
        registry: [{
          id: 'node-agent',
          label: 'Node Agent',
          version: '1.0.0',
          distributions: [{
            type: 'npx', command: 'npx', args: ['node-agent@1.0.0'], env: {},
          }],
        }],
      }),
      /Install Node\.js\/npm to provide npx/,
    )
    assert.equal(existsSync(settingsPath), false)
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
  assert.match(result.stdout, /^Find an ACP Harness$/m)
  assert.match(result.stdout, /martty harness find/)
  assert.doesNotMatch(result.stdout, /martty harness find codex/)
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

test('harness help does not require knowing a Registry id before discovery', async () => {
  const module = await import(moduleUrl)
  const result = module.runHarnessCommand(['help'], {
    settingsPath: '/unused/settings.json',
  })

  assert.match(result.stdout, /martty harness find/)
  assert.doesNotMatch(result.stdout, /martty harness find codex/)
  assert.doesNotMatch(result.stdout, /martty harness add codex-acp/)
})

test('harness add uses the official ACP Registry npx distribution', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-codex-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    const npx = path.join(root, 'npx')
    writeFileSync(npx, '#!/bin/sh\n')
    chmodSync(npx, 0o755)
    const result = await module.runHarnessCommandAsync(['add', 'codex-acp'], {
      settingsPath,
      pathValue: root,
      platform: 'darwin',
      arch: 'arm64',
      registry: [{
        id: 'codex-acp',
        label: 'Codex',
        version: '1.9.0',
        description: 'ACP adapter for Codex',
        distributions: [{
          type: 'npx',
          command: 'npx',
          args: ['@agentclientprotocol/codex-acp@1.9.0'],
          env: {},
        }],
      }],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^Saved Codex$/m)
    assert.deepEqual(JSON.parse(readFileSync(settingsPath, 'utf8')).harnesses, [{
      id: 'codex-acp',
      label: 'Codex',
      command: npx,
      args: ['@agentclientprotocol/codex-acp@1.9.0'],
      env: {},
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
    assert.match(result.stdout, /^    Add      martty harness add demo$/m)
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
    assert.match(result.stdout, /^    Verify   command -v brew-demo-acp$/m)
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

test('harness find tells users to add an unconfigured Registry package before use', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-npx-'))
  try {
    const npx = path.join(root, 'npx')
    writeFileSync(npx, '#!/bin/sh\n')
    chmodSync(npx, 0o755)
    const result = module.runHarnessCommand(['find'], {
      settingsPath: path.join(root, 'settings.json'),
      pathValue: root,
      registry: [{
        id: 'demo',
        label: 'Demo Harness',
        version: '1.0.0',
        description: 'Demo ACP adapter',
        distributions: [{
          type: 'npx',
          command: 'npx',
          args: ['demo-acp@1.0.0'],
          env: {},
        }],
      }],
      defaults: [],
    })

    assert.match(result.stdout, /^    Add      martty harness add demo$/m)
    assert.doesNotMatch(result.stdout, /^    Use      martty harness use demo$/m)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
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

test('harness find keeps saved Harnesses visible instead of showing an empty result', async () => {
  const module = await import(moduleUrl)
  const root = mkdtempSync(path.join(tmpdir(), 'martty-harness-find-configured-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    module.runHarnessCommand([
      'add', 'local', '--label', 'Local ACP', '--command', 'local-acp', '--arg', '--stdio',
    ], { settingsPath })
    const result = module.runHarnessCommand(['find'], {
      settingsPath,
      pathValue: '',
      registry: [],
      defaults: [],
    })
    assert.equal(result.code, 0)
    assert.match(result.stdout, /^ACP Harness candidates \(1\)$/m)
    assert.match(result.stdout, /^  ○ Local ACP$/m)
    assert.match(result.stdout, /^    Status   Configured$/m)
    assert.match(result.stdout, /^    Use      martty harness use local$/m)
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
  // Allow cold module loading on a busy CI filesystem; network responses are
  // separately controlled by the fixture below.
  const timeout = 15_000
  try {
    const add = spawnSync(process.execPath, [
      bin, 'harness', 'add', 'local', '--command', 'local-acp', '--arg', '--stdio',
    ], { env, encoding: 'utf8', timeout })
    assert.equal(add.status, 0, add.stderr)
    assert.match(add.stdout, /^Saved local$/m)
    assert.match(add.stdout, /^  Next     martty harness use local$/m)

    const use = spawnSync(process.execPath, [bin, 'harness', 'use', 'local'], {
      env,
      encoding: 'utf8',
      timeout,
    })
    assert.equal(use.status, 0, use.stderr)

    const list = spawnSync(process.execPath, [bin, 'harness', 'list'], {
      env,
      encoding: 'utf8',
      timeout,
    })
    assert.equal(list.status, 0, list.stderr)
    assert.match(list.stdout, /^Harnesses \(/m)
    assert.match(list.stdout, /^  ● local$/m)

    for (const alias of ['-h', '--help']) {
      const help = spawnSync(process.execPath, [bin, 'harness', alias], {
        env,
        encoding: 'utf8',
        timeout,
      })
      assert.equal(help.status, 0, help.stderr)
      assert.match(help.stdout, /^Martty Harnesses$/m)
    }

    const addHelp = spawnSync(process.execPath, [bin, 'harness', 'add'], {
      env,
      encoding: 'utf8',
      timeout,
    })
    assert.equal(addHelp.status, 0, addHelp.stderr)
    assert.match(addHelp.stdout, /^Add a Harness$/m)

    // The launcher's add path consults the Registry before deciding whether an
    // id is custom. Stub that external HTTP boundary, not the CLI validation.
    const registryFixture = `data:text/javascript,${encodeURIComponent(`
      globalThis.fetch = async (url) => {
        if (url !== 'https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json') {
          throw new Error('Unexpected test request: ' + url)
        }
        return new Response(JSON.stringify({ version: '1.0.0', agents: [] }))
      }
    `)}`
    const incomplete = spawnSync(process.execPath, [
      '--import', registryFixture, bin, 'harness', 'add', 'custom',
    ], {
      env,
      encoding: 'utf8',
      timeout,
    })
    assert.equal(incomplete.status, 2, incomplete.stderr || incomplete.error?.message)
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

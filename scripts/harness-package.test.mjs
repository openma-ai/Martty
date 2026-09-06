import test from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { prepareHarnessPackage } from '../npm/lib/harness-package.js'

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-preparation-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  return root
}

function entry(type = 'npx', spec = '@example/acp@1.2.3') {
  return {
    id: 'example', label: 'Example', command: type, runner: type,
    args: [spec, '--stdio'], env: { ACP_FIXTURE: 'enabled' },
    distribution: { type, command: type, args: [spec, '--stdio'], env: { ACP_FIXTURE: 'enabled' } },
  }
}

function nodeFixture(source, onSpawn) {
  return (command, args, options) => {
    const child = spawn(process.execPath, ['-e', source], options)
    onSpawn?.(child, command, args, options)
    return child
  }
}

test('npx preparation runs only a Node no-op with the same package spec and no TTY', async () => {
  const candidate = entry()
  const original = structuredClone(candidate)
  const progress = []
  let invocation
  const result = await prepareHarnessPackage(candidate, {
    onProgress: (event) => progress.push(event),
    spawnImpl: nodeFixture('console.log("added 1 package")', (_child, command, args, options) => {
      invocation = { command, args, options }
    }),
  })
  assert.equal(result, candidate)
  assert.deepEqual(candidate, original)
  assert.equal(invocation.command, 'npx')
  assert.deepEqual(invocation.args, ['--package', '@example/acp@1.2.3', '--', process.execPath, '-e', ''])
  assert.deepEqual(invocation.options.stdio, ['ignore', 'pipe', 'pipe'])
  assert.equal(invocation.options.windowsHide, true)
  assert.notEqual(invocation.options.shell, true)
  assert.equal(invocation.options.env.ACP_FIXTURE, 'enabled')
  assert.deepEqual([...new Set(progress.map(({ phase }) => phase))], ['preparing', 'download', 'complete'])
})

test('uvx preparation preserves package@version and excludes agent arguments', async () => {
  const candidate = entry('uvx', 'python-agent@2.1.0')
  candidate.runner = '/tools/uvx'
  let invocation
  await prepareHarnessPackage(candidate, {
    spawnImpl: nodeFixture('', (_child, command, args) => { invocation = { command, args } }),
  })
  assert.deepEqual(invocation, {
    command: '/tools/uvx', args: ['--from', 'python-agent@2.1.0', '--', 'python', '-c', ''],
  })
})

test('missing or non-package recipes fail without spawning an agent', async () => {
  for (const candidate of [entry('binary'), entry('npx', ''), entry('uvx', '--help'), {}]) {
    await assert.rejects(prepareHarnessPackage(candidate, {
      spawnImpl() { assert.fail('must not spawn an invalid recipe') },
    }), /package recipe/)
  }
})

test('a pre-aborted preparation never starts a child', async () => {
  const controller = new AbortController()
  controller.abort()
  await assert.rejects(prepareHarnessPackage(entry(), {
    signal: controller.signal,
    spawnImpl() { assert.fail('must not spawn after cancellation') },
  }), /cancelled/)
})

test('package progress and failure tails are bounded and strip split terminal controls', async () => {
  const progress = []
  const source = `
    process.stderr.write('\\x1b]52;c;PRIVATE');
    setTimeout(() => {
      process.stderr.write('CONTROL\\x07\\x1b[31mDownloaded\\x1b[0m\\u202eevil\\r');
      process.stdout.write('x'.repeat(40000) + '\\n');
      process.stderr.write('dependency unavailable\\x00\\x1b[2J\\n');
      process.exitCode = 7;
    }, 20);
  `
  await assert.rejects(prepareHarnessPackage(entry(), {
    onProgress: (event) => progress.push(event), spawnImpl: nodeFixture(source),
  }), (error) => {
    assert.match(error.message, /exited with code 7/)
    assert.match(error.message, /dependency unavailable/)
    assert.ok(error.message.length < 9000)
    assert.doesNotMatch(error.message, /PRIVATE|CONTROL|[\x00-\x08\x1b\x7f-\x9f\u202e]/)
    return true
  })
  assert.ok(progress.some(({ detail }) => detail === 'Downloadedevil'))
  assert.ok(progress.every(({ detail = '' }) => detail.length <= 240))
  assert.ok(progress.every(({ detail = '' }) => !/[\x00-\x1f\x7f-\x9f\u202e]/.test(detail)))
  assert.equal(progress.some(({ phase }) => phase === 'complete'), false)
})

test('a silent stalled package process times out and is reaped before rejection', async () => {
  let child
  await assert.rejects(prepareHarnessPackage(entry(), {
    timeoutMs: 80,
    spawnImpl: nodeFixture('setInterval(() => {}, 1000)', (value) => { child = value }),
  }), /timed out/)
  assert.notEqual(child.signalCode, null)
  assert.throws(() => process.kill(child.pid, 0), { code: 'ESRCH' })
})

test('cancellation terminates the installer process tree and never reports complete', {
  skip: process.platform === 'win32' && 'POSIX process-group fixture; native Windows case is below',
}, async () => {
  const controller = new AbortController()
  const progress = []
  let child, descendant
  const source = `
    const {spawn} = require('node:child_process');
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {stdio: 'ignore'});
    console.log('descendant ' + child.pid);
    setInterval(() => {}, 1000);
  `
  await assert.rejects(prepareHarnessPackage(entry(), {
    signal: controller.signal,
    spawnImpl: nodeFixture(source, (value) => { child = value }),
    onProgress(event) {
      progress.push(event)
      if (event.detail?.startsWith('descendant ')) {
        descendant = Number(event.detail.slice('descendant '.length))
        controller.abort()
      }
    },
  }), /cancelled/)
  assert.ok(descendant > 0)
  assert.throws(() => process.kill(child.pid, 0), { code: 'ESRCH' })
  assert.throws(() => process.kill(descendant, 0), { code: 'ESRCH' })
  assert.equal(progress.some(({ phase }) => phase === 'complete'), false)
})

test('a progress consumer error also terminates the running package process', async () => {
  let child
  await assert.rejects(prepareHarnessPackage(entry(), {
    spawnImpl: nodeFixture('console.log("starting"); setInterval(() => {}, 1000)', (value) => { child = value }),
    onProgress({ phase }) { if (phase === 'download') throw new Error('panel closed unexpectedly') },
  }), /panel closed unexpectedly/)
  assert.throws(() => process.kill(child.pid, 0), { code: 'ESRCH' })
})

test('a runner exiting before its installer descendant cannot leave a background process', {
  skip: process.platform === 'win32' && 'POSIX process-group ownership fixture',
}, async (t) => {
  let descendant
  t.after(() => { if (descendant) { try { process.kill(descendant, 'SIGKILL') } catch {} } })
  await prepareHarnessPackage(entry(), {
    spawnImpl: nodeFixture(`
      const child = require('node:child_process').spawn(process.execPath,
        ['-e', 'setInterval(() => {}, 1000)'], {stdio: 'ignore'});
      process.stdout.write('descendant ' + child.pid + '\\n', () => process.exit(0));
    `),
    onProgress({ detail }) {
      if (detail?.startsWith('descendant ')) descendant = Number(detail.slice('descendant '.length))
    },
  })
  assert.ok(descendant > 0)
  assert.throws(() => process.kill(descendant, 0), { code: 'ESRCH' })
})

test('a missing package runner reports a spawn error without hanging', async (t) => {
  const candidate = entry()
  candidate.runner = path.join(fixture(t), 'missing-npx')
  await assert.rejects(prepareHarnessPackage(candidate), /ENOENT|not found/)
})

test('real npx warms an isolated local package cache without invoking the agent', async (t) => {
  const root = fixture(t)
  const packageRoot = path.join(root, 'fixture-package')
  mkdirSync(packageRoot)
  const marker = path.join(root, 'agent-ran')
  writeFileSync(path.join(packageRoot, 'package.json'), JSON.stringify({
    name: 'martty-warm-fixture', version: '1.0.0', bin: { 'martty-warm-fixture': 'agent.js' },
  }))
  writeFileSync(path.join(packageRoot, 'agent.js'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ran')\n`)
  const candidate = entry('npx', packageRoot)
  candidate.runner = path.join(path.dirname(process.execPath), process.platform === 'win32' ? 'npx.cmd' : 'npx')
  if (!existsSync(candidate.runner)) return t.skip('npx is not installed alongside this Node')
  const cache = path.join(root, 'npm-cache')
  const env = { ...process.env, npm_config_cache: cache, npm_config_offline: 'true', npm_config_audit: 'false', npm_config_fund: 'false',
    npm_config_update_notifier: 'false' }
  await prepareHarnessPackage(candidate, {
    cwd: root, timeoutMs: 30000,
    env,
    onProgress: ({ phase, detail }) => t.diagnostic(`${phase}: ${detail}`),
  })
  assert.equal(existsSync(marker), false)
  const hash = createHash('sha512').update(packageRoot).digest('hex').slice(0, 16)
  const installed = path.join(cache, '_npx', hash, 'node_modules', 'martty-warm-fixture', 'package.json')
  assert.equal(JSON.parse(readFileSync(installed, 'utf8')).version, '1.0.0')
  for (let launch = 0; launch < 2; launch++) {
    const run = spawnSync(candidate.runner, candidate.args, { cwd: root, env, encoding: 'utf8', timeout: 30000 })
    assert.equal(run.status, 0, run.stderr)
    assert.doesNotMatch(run.stderr, /will be installed|added \d+ packages/)
  }
  assert.equal(readFileSync(marker, 'utf8'), 'ran')
})

// A tiny uncompressed wheel avoids external build backends and network installs.
function wheelArchive(files) {
  const locals = [], central = []
  let offset = 0
  for (const [name, text] of Object.entries(files)) {
    const filename = Buffer.from(name), content = Buffer.from(text)
    let checksum = 0xffffffff
    for (const byte of content) {
      checksum ^= byte
      for (let bit = 0; bit < 8; bit++) checksum = (checksum >>> 1) ^ (0xedb88320 & -(checksum & 1))
    }
    checksum = (checksum ^ 0xffffffff) >>> 0
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50); local.writeUInt16LE(20, 4)
    local.writeUInt32LE(checksum, 14); local.writeUInt32LE(content.length, 18)
    local.writeUInt32LE(content.length, 22); local.writeUInt16LE(filename.length, 26)
    const directory = Buffer.alloc(46)
    directory.writeUInt32LE(0x02014b50); directory.writeUInt16LE(20, 4); directory.writeUInt16LE(20, 6)
    directory.writeUInt32LE(checksum, 16); directory.writeUInt32LE(content.length, 20)
    directory.writeUInt32LE(content.length, 24); directory.writeUInt16LE(filename.length, 28)
    directory.writeUInt32LE(offset, 42)
    locals.push(local, filename, content); central.push(directory, filename)
    offset += local.length + filename.length + content.length
  }
  const directory = Buffer.concat(central), end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(Object.keys(files).length, 8)
  end.writeUInt16LE(Object.keys(files).length, 10); end.writeUInt32LE(directory.length, 12)
  end.writeUInt32LE(offset, 16)
  return Buffer.concat([...locals, directory, end])
}

test('real uvx warms the selected package environment without invoking its console script', async (t) => {
  const command = process.env.MARTTY_TEST_UVX
    ?? (existsSync('/opt/homebrew/bin/uvx') ? '/opt/homebrew/bin/uvx' : 'uvx')
  if (spawnSync(command, ['--version'], { timeout: 3000 }).status !== 0) return t.skip('uvx is not installed')
  const root = fixture(t)
  const marker = path.join(root, 'agent-ran')
  const info = 'warm_fixture-1.0.0.dist-info'
  const files = {
    'warm_fixture/__init__.py': `def main():\n    open(${JSON.stringify(marker)}, 'w').write('ran')\n`,
    [`${info}/METADATA`]: 'Metadata-Version: 2.1\nName: warm-fixture\nVersion: 1.0.0\n',
    [`${info}/WHEEL`]: 'Wheel-Version: 1.0\nGenerator: Martty test\nRoot-Is-Purelib: true\nTag: py3-none-any\n',
    [`${info}/entry_points.txt`]: '[console_scripts]\nwarm-fixture = warm_fixture:main\n',
  }
  files[`${info}/RECORD`] = [...Object.keys(files), `${info}/RECORD`].map((name) => `${name},,\n`).join('')
  writeFileSync(path.join(root, 'warm_fixture-1.0.0-py3-none-any.whl'), wheelArchive(files))
  const candidate = entry('uvx', 'warm-fixture@1.0.0')
  candidate.runner = command
  const cache = path.join(root, 'uv-cache')
  const env = { ...process.env, UV_OFFLINE: '1', UV_NO_INDEX: '1', UV_FIND_LINKS: root, UV_CACHE_DIR: cache,
    UV_TOOL_DIR: path.join(root, 'tools'), UV_PYTHON_DOWNLOADS: 'never' }
  await prepareHarnessPackage(candidate, {
    cwd: root, timeoutMs: 15000,
    env,
  })
  assert.equal(existsSync(marker), false)
  assert.ok(readdirSync(cache, { recursive: true }).some((name) =>
    name.includes('environments') && name.endsWith(path.join(info, 'METADATA'))))
  for (let launch = 0; launch < 2; launch++) {
    const run = spawnSync(command, candidate.args, { cwd: root, env, encoding: 'utf8', timeout: 15000 })
    assert.equal(run.status, 0, run.stderr)
    assert.doesNotMatch(run.stderr, /Installed \d+ package/)
  }
  assert.equal(readFileSync(marker, 'utf8'), 'ran')
})

test('Windows package runners use cmd shim escaping without starting the agent', {
  skip: process.platform !== 'win32' && 'requires native Windows cmd.exe',
}, async (t) => {
  const root = fixture(t)
  const runner = path.join(root, 'npx.cmd')
  const output = path.join(root, 'args.json')
  const script = path.join(root, 'capture.cjs')
  writeFileSync(script, `require('node:fs').writeFileSync(${JSON.stringify(output)}, JSON.stringify(process.argv.slice(2)))`)
  writeFileSync(runner, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`)
  const candidate = entry()
  candidate.runner = runner
  await prepareHarnessPackage(candidate)
  assert.deepEqual(JSON.parse(readFileSync(output, 'utf8')), ['--package', '@example/acp@1.2.3', '--', process.execPath, '-e', ''])
})

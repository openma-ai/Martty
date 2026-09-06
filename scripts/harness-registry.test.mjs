import test from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, truncateSync, writeFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fetchAcpRegistry, installRegistryBinary, managedBinaryPath } from '../npm/lib/harness-registry.js'
import { readAcpRegistrySnapshot } from '../npm/lib/harness-registry.js'

const catalog = JSON.stringify({ agents: [{
  id: 'example', name: 'Example', version: '1.0.0',
  distribution: { npx: { package: 'example-acp' } },
}] })

test('successful Registry refresh persists a snapshot for a new process', async (t) => {
  const { options } = fixture(t)
  await fetchAcpRegistry({ ...options, fetchImpl: async () => new Response(catalog) })
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { readAcpRegistrySnapshot } from ${JSON.stringify(new URL('../npm/lib/harness-registry.js', import.meta.url).href)};
    console.log(JSON.stringify(readAcpRegistrySnapshot(${JSON.stringify(options.settingsPath ? { settingsPath: options.settingsPath } : {})})))
  `], { encoding: 'utf8' })
  assert.equal(child.status, 0, child.stderr)
  assert.deepEqual(JSON.parse(child.stdout).map(({ id }) => id), ['example'])
})

test('failed or malformed Registry refresh preserves the last valid disk snapshot', async (t) => {
  const { options } = fixture(t)
  await fetchAcpRegistry({ ...options, fetchImpl: async () => new Response(catalog) })
  await assert.rejects(fetchAcpRegistry({ ...options, fetchImpl: async () => { throw new Error('offline') } }), /offline/)
  await assert.rejects(fetchAcpRegistry({ ...options, fetchImpl: async () => new Response('{}') }), /invalid.*catalog/i)
  assert.deepEqual(readAcpRegistrySnapshot(options).map(({ id }) => id), ['example'])
})

test('corrupt disk Registry cache falls back to the bundled official catalog', (t) => {
  const { options, root } = fixture(t)
  mkdirSync(path.join(root, 'cache'))
  writeFileSync(path.join(root, 'cache', 'acp-registry.json'), '{broken')
  assert.ok(readAcpRegistrySnapshot(options).some(({ id }) => id === 'antigravity-acp'))
})

function delayedResponse(text, delay = 100) {
  let timer
  return new Response(new ReadableStream({
    start(controller) {
      controller.enqueue(Buffer.from(text.slice(0, 1)))
      timer = setTimeout(() => {
        controller.enqueue(Buffer.from(text.slice(1)))
        controller.close()
      }, delay)
    },
    cancel() { clearTimeout(timer) },
  }))
}

function stalledResponse(text) {
  return new Response(new ReadableStream({
    start(controller) { controller.enqueue(Buffer.from(text.slice(0, 1))) },
  }))
}

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-registry-transfer-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const entry = {
    id: 'example', label: 'Example', version: '1.0.0',
    distribution: {
      type: 'binary', target: 'linux-x86_64', command: './example-acp',
      archive: 'https://example.test/example.tar.gz', args: [], env: {},
    },
  }
  const options = {
    settingsPath: path.join(root, 'settings.json'),
    extractArchive(_archive, destination) {
      writeFileSync(path.join(destination, 'example-acp'), '#!/bin/sh\n')
    },
  }
  return { root, entry, options }
}

function assertNotInstalled(entry, options) {
  const command = managedBinaryPath(entry, options)
  assert.equal(existsSync(path.dirname(command)), false)
  const parent = path.dirname(path.dirname(command))
  if (existsSync(parent)) assert.deepEqual(readdirSync(parent), [])
  assert.equal(existsSync(options.settingsPath), false)
}

async function localArchive(t, entry, body = 'archive bytes') {
  const server = createServer(typeof body === 'function' ? body : (_request, response) => {
    response.writeHead(200, { 'content-length': Buffer.byteLength(body) })
    response.end(body)
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  t.after(() => new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve())
    server.closeAllConnections()
  }))
  const archivePath = new URL(entry.distribution.archive).pathname
  entry.distribution.archive = `http://127.0.0.1:${server.address().port}${archivePath}`
}

test('Registry timeout includes a stalled JSON response body', async () => {
  await assert.rejects(fetchAcpRegistry({
    timeoutMs: 20,
    fetchImpl: async () => stalledResponse(catalog),
  }), /ACP Registry request timed out/)
})

test('Registry fetch can be cancelled while reading JSON', async () => {
  const controller = new AbortController()
  const task = fetchAcpRegistry({
    signal: controller.signal,
    fetchImpl: async () => delayedResponse(catalog),
  })
  controller.abort()
  await assert.rejects(task, /cancelled/)
})

test('Registry timeout also covers response headers', async () => {
  await assert.rejects(fetchAcpRegistry({
    timeoutMs: 20,
    fetchImpl: () => new Promise(() => {}),
  }), /ACP Registry request timed out/)
})

test('binary installation delegates download to a fixed staging path before verifying the archive', async (t) => {
  const { root, entry, options } = fixture(t)
  const archive = Buffer.from('download adapter archive')
  entry.distribution.sha256 = createHash('sha256').update(archive).digest('hex')
  const controller = new AbortController()
  let downloadPath
  const installed = await installRegistryBinary(entry, {
    ...options,
    signal: controller.signal,
    connectTimeoutMs: 1234,
    idleTimeoutMs: 5678,
    fetchImpl() { throw new Error('binary installation must not use the Registry JSON fetch transport') },
    async downloadFile(url, destination, downloadOptions) {
      assert.equal(url, entry.distribution.archive)
      assert.equal(downloadOptions.signal, controller.signal)
      assert.equal(downloadOptions.connectTimeoutMs, 1234)
      assert.equal(downloadOptions.idleTimeoutMs, 5678)
      assert.equal(downloadOptions.maxBytes, 512 * 1024 * 1024)
      downloadPath = destination
      assert.equal(path.basename(destination), 'download.tar.gz')
      assert.match(path.basename(path.dirname(destination)), /^\.install-/)
      assert.equal(path.relative(root, destination).startsWith('..'), false)
      writeFileSync(destination, archive)
    },
    extractArchive(archivePath, destination) {
      assert.equal(archivePath, downloadPath)
      assert.deepEqual(readFileSync(archivePath), archive)
      options.extractArchive(archivePath, destination)
    },
  })
  assert.equal(existsSync(installed.command), true)
  assert.equal(existsSync(downloadPath), false)
})

test('binary transfer reports streamed bytes and extraction stages', async (t) => {
  const { entry, options } = fixture(t)
  const archive = Buffer.from('several archive chunks')
  entry.distribution.sha256 = createHash('sha256').update(archive).digest('hex')
  const progress = []
  let response
  await localArchive(t, entry, (_request, res) => {
    response = res
    res.writeHead(200, { 'content-length': archive.length })
    res.write(archive.subarray(0, 7))
  })
  const installed = await installRegistryBinary(entry, {
    ...options,
    onProgress(event) {
      progress.push(event)
      if (event.phase === 'download' && event.receivedBytes === 7) response.end(archive.subarray(7))
    },
    extractArchive(archivePath, destination) {
      assert.deepEqual(readFileSync(archivePath), archive)
      options.extractArchive(archivePath, destination)
    },
  })
  assert.equal(existsSync(installed.command), true)
  assert.deepEqual([...new Set(progress.map((event) => event.phase))], [
    'download', 'extract', 'verify', 'complete',
  ])
  assert.ok(progress.some((event) => event.phase === 'download' && event.receivedBytes === 7))
  assert.ok(progress.some((event) => event.phase === 'download'
    && event.receivedBytes === archive.length && event.totalBytes === archive.length))
})

test('binary timeout includes a stalled archive response body and leaves no install', async (t) => {
  const { entry, options } = fixture(t)
  await localArchive(t, entry, (_request, response) => {
    response.writeHead(200)
    response.write('x')
  })
  await assert.rejects(installRegistryBinary(entry, {
    ...options, connectTimeoutMs: 5000, idleTimeoutMs: 200,
  }), /binary download timed out/)
  assertNotInstalled(entry, options)
})

test('binary cancellation stops transfer and removes only its staging directory', async (t) => {
  const { root, entry, options } = fixture(t)
  const preserved = path.join(root, 'other-install.txt')
  writeFileSync(preserved, 'keep me')
  const controller = new AbortController()
  await localArchive(t, entry, (_request, response) => {
    response.writeHead(200)
    response.write('x')
  })
  await assert.rejects(installRegistryBinary(entry, {
    ...options, signal: controller.signal,
    onProgress(event) {
      if (event.phase === 'download' && event.receivedBytes > 0) controller.abort()
    },
  }), /cancelled/)
  assertNotInstalled(entry, options)
  assert.equal(readFileSync(preserved, 'utf8'), 'keep me')
})

test('binary extraction cancellation never publishes a partially extracted install', async (t) => {
  const { entry, options } = fixture(t)
  const controller = new AbortController()
  await localArchive(t, entry)
  await assert.rejects(installRegistryBinary(entry, {
    ...options, signal: controller.signal,
    async extractArchive(_archive, destination, _url, extraction) {
      writeFileSync(path.join(destination, 'example-acp'), 'partial')
      controller.abort()
      assert.equal(extraction?.signal.aborted, true)
    },
  }), /cancelled/)
  assertNotInstalled(entry, options)
})

test('binary extraction can finish before its ten-minute default deadline', async (t) => {
  const { entry, options } = fixture(t)
  const result = await installRegistryBinary(entry, {
    ...options,
    async downloadFile(_url, destination) {
      writeFileSync(destination, 'archive bytes')
      // Advance only the extraction clock; keep transfer and native I/O real-time.
      t.mock.timers.enable({ apis: ['setTimeout'] })
    },
    async extractArchive(_archive, destination, _url, { signal }) {
      writeFileSync(path.join(destination, 'example-acp'), 'partial')
      t.mock.timers.tick(10 * 60_000 - 1)
      assert.equal(signal.aborted, false, 'extraction may continue until ten minutes')
      writeFileSync(path.join(destination, 'example-acp'), '#!/bin/sh\ncomplete\n')
    },
  })
  assert.match(readFileSync(result.command, 'utf8'), /complete/)
})

test('binary extraction times out at ten minutes by default and cleans staging', async (t) => {
  const { entry, options } = fixture(t)
  await assert.rejects(installRegistryBinary(entry, {
    ...options,
    async downloadFile(_url, destination) {
      writeFileSync(destination, 'archive bytes')
      t.mock.timers.enable({ apis: ['setTimeout'] })
    },
    async extractArchive(_archive, destination, _url, { signal }) {
      writeFileSync(path.join(destination, 'example-acp'), 'partial')
      t.mock.timers.tick(10 * 60_000 - 1)
      assert.equal(signal.aborted, false)
      t.mock.timers.tick(1)
      // The real install path must reject instead of publishing this payload.
    },
  }), /binary extraction timed out/)
  assertNotInstalled(entry, options)
})

test('binary extraction honors an explicitly supplied deadline and leaves no install', async (t) => {
  const { entry, options } = fixture(t)
  await localArchive(t, entry)
  await assert.rejects(installRegistryBinary(entry, {
    ...options, extractTimeoutMs: 20,
    extractArchive: () => new Promise(() => {}),
  }), /binary extraction timed out/)
  assertNotInstalled(entry, options)
})

test('an extractor rejecting during cancellation does not leave an unhandled rejection', async (t) => {
  const { entry, options } = fixture(t)
  const controller = new AbortController()
  await localArchive(t, entry)
  await assert.rejects(installRegistryBinary(entry, {
    ...options, signal: controller.signal,
    async extractArchive() {
      controller.abort()
      throw new Error('extractor noticed cancellation')
    },
  }), /cancelled/)
  // Let Node surface any rejected extractor promise abandoned by cancellation.
  await new Promise((resolve) => setImmediate(resolve))
  assertNotInstalled(entry, options)
})

test('binary timeout also covers response headers without retaining staging files', async (t) => {
  const { entry, options } = fixture(t)
  await localArchive(t, entry, () => {})
  await assert.rejects(installRegistryBinary(entry, {
    ...options, timeoutMs: 20,
  }), /binary download timed out/)
  assertNotInstalled(entry, options)
})

test('binary checksum failures never extract or publish an install', async (t) => {
  const { entry, options } = fixture(t)
  entry.distribution.sha256 = '0'.repeat(64)
  let extracted = false
  await localArchive(t, entry)
  await assert.rejects(installRegistryBinary(entry, {
    ...options,
    extractArchive() { extracted = true },
  }), /binary checksum mismatch/)
  assert.equal(extracted, false)
  assertNotInstalled(entry, options)
})

test('oversized binary response headers fail before extraction and leave no install', async (t) => {
  const { entry, options } = fixture(t)
  await localArchive(t, entry, (_request, response) => {
    response.writeHead(200, { 'content-length': 512 * 1024 * 1024 + 1 })
    response.flushHeaders()
  })
  let extracted = false
  await assert.rejects(installRegistryBinary(entry, {
    ...options,
    extractArchive() { extracted = true },
  }), /larger than/)
  assert.equal(extracted, false)
  assertNotInstalled(entry, options)
})

test('binary installation rechecks the downloaded file size before hashing or extraction', async (t) => {
  const { entry, options } = fixture(t)
  entry.distribution.sha256 = '0'.repeat(64)
  let extracted = false
  await assert.rejects(installRegistryBinary(entry, {
    ...options,
    fetchImpl() { throw new Error('binary installation must use the download adapter') },
    async downloadFile(_url, destination) {
      writeFileSync(destination, '')
      // A sparse fixture exercises the installer's second size guard without
      // transferring or allocating an actual 512 MiB archive.
      truncateSync(destination, 512 * 1024 * 1024 + 1)
    },
    extractArchive() { extracted = true },
  }), /larger than/)
  assert.equal(extracted, false)
  assertNotInstalled(entry, options)
})

test('binary commands cannot escape the managed installation directory', async (t) => {
  const { root, entry, options } = fixture(t)
  const preserved = path.join(root, 'outside-command')
  writeFileSync(preserved, 'keep me')
  entry.distribution.command = '../outside-command'
  await assert.rejects(installRegistryBinary(entry, {
    ...options,
    downloadFile() { throw new Error('an unsafe binary recipe must fail before downloading') },
  }), /must stay inside/)
  assert.equal(readFileSync(preserved, 'utf8'), 'keep me')
  assert.deepEqual(readdirSync(root), ['outside-command'])
})

test('native extractor installs a local tar fixture with the streamed downloader', async (t) => {
  const { root, entry, options } = fixture(t)
  const input = path.join(root, 'source')
  mkdirSync(input)
  writeFileSync(path.join(input, 'example-acp'), '#!/bin/sh\n')
  const archive = path.join(root, 'fixture.tar')
  const tar = process.platform === 'win32' ? 'tar.exe' : 'tar'
  const created = spawnSync(tar, ['-cf', archive, '-C', input, 'example-acp'], { encoding: 'utf8' })
  assert.equal(created.status, 0, created.error?.message ?? created.stderr)
  entry.distribution.archive = 'https://example.test/example.tar'
  await localArchive(t, entry, readFileSync(archive))
  const installed = await installRegistryBinary(entry, {
    settingsPath: options.settingsPath,
  })
  assert.equal(readFileSync(installed.command, 'utf8'), '#!/bin/sh\n')
})

test('cancelled native extraction waits for child exit before removing its staging files', {
  skip: process.platform === 'win32' ? 'fixture uses a POSIX executable shim' : false,
}, async (t) => {
  const { root, entry, options } = fixture(t)
  const toolsDir = path.join(root, 'tools')
  const childInfo = path.join(root, 'extracting.json')
  mkdirSync(toolsDir)
  const tarPath = path.join(toolsDir, 'tar')
  writeFileSync(tarPath, `#!${process.execPath}\n`
    + `const fs = require('node:fs');\n`
    + `if (process.argv[2] === '-tf') { process.stdout.write('example-acp\\n'); }\n`
    + `else { fs.writeFileSync(${JSON.stringify(childInfo)}, JSON.stringify({pid:process.pid,destination:process.argv[5]})); setInterval(() => {}, 1000); }\n`)
  chmodSync(tarPath, 0o755)
  const originalPath = process.env.PATH
  process.env.PATH = `${toolsDir}${path.delimiter}${originalPath ?? ''}`
  t.after(() => { process.env.PATH = originalPath })
  const controller = new AbortController()
  await localArchive(t, entry)
  const transfer = installRegistryBinary(entry, {
    settingsPath: options.settingsPath,
    signal: controller.signal,
  })
  const rejected = assert.rejects(transfer, /cancelled/)
  // Observe a running extraction process, not merely the preceding inspect step.
  const deadline = Date.now() + 20_000
  while (!existsSync(childInfo) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 20))
  }
  controller.abort()
  await rejected
  assert.equal(existsSync(childInfo), true)
  const child = JSON.parse(readFileSync(childInfo, 'utf8'))
  assert.throws(() => process.kill(child.pid, 0), { code: 'ESRCH' })
  assert.equal(existsSync(child.destination), false)
  assertNotInstalled(entry, options)
})

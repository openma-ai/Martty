import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { execFile } from 'node:child_process'
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

const api = await import('../npm/lib/download.js').catch(() => ({}))

async function fixture(t, handler) {
  assert.equal(typeof api.downloadFile, 'function', 'a shared SDK download adapter is available')
  const root = mkdtempSync(path.join(tmpdir(), 'martty-sdk-download-'))
  const server = createServer(handler)
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(async () => {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
    rmSync(root, { recursive: true, force: true })
  })
  const url = `http://127.0.0.1:${server.address().port}/archive`
  return { root, url, destination: path.join(root, 'download.tar.gz') }
}

test('SDK follows redirects and fixes the local filename, including unknown content length', async (t) => {
  const { url, root, destination } = await fixture(t, (req, res) => {
    if (req.url === '/archive') { res.writeHead(302, { location: '/payload' }); res.end(); return }
    res.writeHead(200, { 'content-disposition': 'attachment; filename="../../not-the-target"' })
    res.write('hello ')
    res.end('sdk')
  })
  const progress = []
  await api.downloadFile(url, destination, { onProgress: (event) => progress.push(event) })
  assert.equal(readFileSync(destination, 'utf8'), 'hello sdk')
  assert.deepEqual(readdirSync(root), ['download.tar.gz'])
  assert.ok(progress.some((event) => event.receivedBytes === 9))
})

test('SDK slow transfers continue past the timeout as long as bytes keep arriving', { timeout: 10_000 }, async (t) => {
  const { url, destination } = await fixture(t, (_req, res) => {
    res.writeHead(200)
    res.write('x')
    let sent = 1
    const timer = setInterval(() => { if (++sent === 8) res.end('x'); else res.write('x') }, 180)
    res.on('close', () => clearInterval(timer))
  })
  await api.downloadFile(url, destination, { connectTimeoutMs: 1000, idleTimeoutMs: 700 })
  assert.equal(readFileSync(destination, 'utf8'), 'xxxxxxxx')
})

test('SDK idle and connection failures are distinguished and remove partial files', { timeout: 10_000 }, async (t) => {
  const { url, root, destination } = await fixture(t, (req, res) => {
    if (req.url === '/headers') return
    res.writeHead(200)
    res.write('x')
  })
  await assert.rejects(api.downloadFile(url, destination, { idleTimeoutMs: 200 }), /idle.*received 1 bytes/)
  assert.deepEqual(readdirSync(root), [])
  await assert.rejects(api.downloadFile(url.replace('/archive', '/headers'), destination, { connectTimeoutMs: 200 }), /connecting.*received 0 bytes/)
  assert.deepEqual(readdirSync(root), [])
})

test('SDK aborts during progress and before starting without treating stop as success', async (t) => {
  const { url, root, destination } = await fixture(t, (_req, res) => { res.writeHead(200); res.write('partial') })
  const controller = new AbortController()
  await assert.rejects(api.downloadFile(url, destination, {
    signal: controller.signal,
    onProgress(event) { if (event.receivedBytes > 0) controller.abort() },
  }), /cancelled/)
  assert.deepEqual(readdirSync(root), [])
  await assert.rejects(api.downloadFile(url, destination, { signal: controller.signal }), /cancelled/)
  assert.deepEqual(readdirSync(root), [])
})

test('SDK enforces both declared and streamed size limits', async (t) => {
  const { url, root, destination } = await fixture(t, (req, res) => {
    if (req.url === '/declared') res.writeHead(200, { 'content-length': '1000' })
    else res.writeHead(200)
    res.write('oversized')
  })
  for (const route of ['/declared', '/streamed']) {
    await assert.rejects(api.downloadFile(url.replace('/archive', route), destination, { maxBytes: 4 }), /larger than/)
    assert.deepEqual(readdirSync(root), [])
  }
})

test('SDK rejects truncated downloads and permits an explicit clean retry', async (t) => {
  let attempts = 0
  const { url, destination, root } = await fixture(t, (_req, res) => {
    attempts++
    if (attempts === 1) {
      res.writeHead(200, { 'content-length': '100' })
      res.write('partial')
      setImmediate(() => res.destroy())
    } else res.end('complete')
  })
  await assert.rejects(api.downloadFile(url, destination), /download/)
  assert.equal(attempts, 1, 'failures stay under the panel retry action')
  assert.deepEqual(readdirSync(root), [])
  await api.downloadFile(url, destination)
  assert.equal(readFileSync(destination, 'utf8'), 'complete')
})

test('SDK does not overwrite an existing destination or accept a non-HTTP URL', async (t) => {
  const { url, destination } = await fixture(t, (_req, res) => res.end('replacement'))
  writeFileSync(destination, 'keep')
  await assert.rejects(api.downloadFile(url, destination), /exists/)
  assert.equal(readFileSync(destination, 'utf8'), 'keep')
  await assert.rejects(api.downloadFile('file:///etc/passwd', destination), /HTTP/)
})

test('SDK rejects invalid redirects without crashing the download process', async (t) => {
  const { url, root, destination } = await fixture(t, (req, res) => {
    res.writeHead(302, { location: req.url === '/malformed' ? 'http://[broken' : 'ftp://127.0.0.1/not-http' })
    res.end()
  })
  // Isolate uncaught exceptions from the SDK's HTTP response callback. A
  // malformed redirect must reject the adapter, not terminate Martty's process.
  const moduleUrl = new URL('../npm/lib/download.js', import.meta.url).href
  for (const address of [url, url.replace('/archive', '/malformed')]) {
    const source = `
    import { downloadFile } from ${JSON.stringify(moduleUrl)};
    try {
      await downloadFile(${JSON.stringify(address)}, ${JSON.stringify(destination)}, {
        connectTimeoutMs: 1000, idleTimeoutMs: 1000,
      });
      process.exitCode = 2;
    } catch (error) {
      process.stdout.write('rejected: ' + error.message);
    }
  `
    const result = await new Promise((resolve) => {
      execFile(process.execPath, ['--input-type=module', '--eval', source], {
        timeout: 5000, maxBuffer: 128 * 1024,
      }, (error, stdout, stderr) => resolve({ error, stdout, stderr }))
    })
    assert.equal(result.error?.code, undefined, result.stderr.slice(-2000))
    assert.match(result.stdout, /rejected:.*(HTTP|redirect|protocol|URL)/i)
    assert.deepEqual(readdirSync(root), [])
  }
})

test('SDK resolves each relative redirect against its immediately preceding URL', async (t) => {
  const { url, destination } = await fixture(t, (req, res) => {
    if (req.url === '/archive') {
      res.writeHead(302, { location: '/release/metadata' })
      res.end()
    } else if (req.url === '/release/metadata') {
      res.writeHead(302, { location: 'payload' })
      res.end()
    } else if (req.url === '/release/payload') res.end('correct artifact')
    else { res.writeHead(404); res.end('wrong redirect base') }
  })
  await api.downloadFile(url, destination)
  assert.equal(readFileSync(destination, 'utf8'), 'correct artifact')
})

test('SDK rejects a redirect response without a destination instead of saving its body', async (t) => {
  const { url, destination, root } = await fixture(t, (_req, res) => {
    res.writeHead(302)
    res.end('not an archive')
  })
  await assert.rejects(api.downloadFile(url, destination), /HTTP 302/)
  assert.deepEqual(readdirSync(root), [])
})

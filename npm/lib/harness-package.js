import { StringDecoder } from 'node:string_decoder'
import spawn from 'cross-spawn'

const DEFAULT_TIMEOUT_MS = 300_000
const DETAIL_LIMIT = 240
const OUTPUT_LIMIT = 8192

// Keep parser state across chunks: an OSC clipboard/title payload or split CSI
// must never become visible text in a progress panel. Partial lines stay bounded.
function outputLines(onLine) {
  const decoder = new StringDecoder('utf8')
  let state = 'text', line = ''
  const flush = () => {
    const text = line.trim()
    line = ''
    if (text) onLine(text)
  }
  const consume = (text) => {
    for (const char of text) {
      const code = char.codePointAt(0)
      if (state === 'escape') {
        state = char === '[' ? 'csi' : char === ']' ? 'osc'
          : ['P', 'X', '^', '_'].includes(char) ? 'string'
            : code >= 0x20 && code <= 0x2f ? 'intermediate' : 'text'
      } else if (state === 'intermediate') {
        if (code >= 0x30 && code <= 0x7e) state = 'text'
      } else if (state === 'csi') {
        if (code >= 0x40 && code <= 0x7e) state = 'text'
        else if (code === 0x1b) state = 'escape'
      } else if (state === 'osc' || state === 'string') {
        if (code === 0x07 || code === 0x9c) state = 'text'
        else if (code === 0x1b) state = 'stringEscape'
      } else if (state === 'stringEscape') {
        state = char === '\\' ? 'text' : code === 0x1b ? 'stringEscape' : 'string'
      } else if (code === 0x1b) state = 'escape'
      else if (code === 0x9b) state = 'csi'
      else if (code === 0x9d) state = 'osc'
      else if ([0x90, 0x98, 0x9e, 0x9f].includes(code)) state = 'string'
      else if (char === '\n' || char === '\r') flush()
      else if (char === '\t') { if (line.length < OUTPUT_LIMIT) line += ' ' }
      else if (code >= 0x20 && !(code >= 0x7f && code <= 0x9f)
        && !/[\u061c\u200e\u200f\u2028-\u202e\u2066-\u2069]/u.test(char)
        && line.length < OUTPUT_LIMIT) line += char
    }
  }
  return {
    write(chunk) { consume(decoder.write(chunk)) },
    end() { consume(decoder.end()); flush() },
  }
}

function safeText(value, limit = DETAIL_LIMIT) {
  const lines = []
  const output = outputLines((line) => lines.push(line))
  output.write(Buffer.from(String(value ?? '').slice(0, OUTPUT_LIMIT)))
  output.end()
  return lines.join(' ').slice(0, limit)
}

function cancellationError() {
  const error = new Error('Harness package preparation cancelled')
  error.name = 'AbortError'
  return error
}

function signalProcessTree(child, signal) {
  if (!child.pid) return
  try { process.kill(-child.pid, signal) }
  catch (error) {
    if (error.code !== 'ESRCH') {
      try { child.kill(signal) } catch { /* The process may already have exited. */ }
    }
  }
}

async function stopProcessTree(child) {
  if (!child.pid) return
  if (process.platform === 'win32') {
    // Killing cmd.exe alone leaves npm/uv and their children running. taskkill
    // accepts the numeric PID directly; no shell or user-controlled arguments.
    await new Promise((resolve) => {
      let killer
      try {
        killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], {
          stdio: 'ignore', windowsHide: true,
        })
      } catch { resolve(); return }
      const timer = setTimeout(() => { killer.kill(); resolve() }, 2000)
      const done = () => { clearTimeout(timer); resolve() }
      killer.once('error', done)
      killer.once('close', done)
    })
    try { child.kill() } catch { /* Already reaped by taskkill. */ }
    return
  }
  signalProcessTree(child, 'SIGTERM')
  // Always finish killing the owned group, even when its leader exits first and
  // descendants have closed their pipes. Do not return while installers survive.
  await new Promise((resolve) => setTimeout(resolve, 200))
  signalProcessTree(child, 'SIGKILL')
}

/**
 * Populate the package runner's cache without invoking the ACP entry point.
 * npm 7+ hashes --package specs exactly as normal npx invocations; stdin is not
 * a TTY, so first install needs no --yes flag. uvx --from builds the same package
 * environment before running Python (including package@version / @latest).
 * This never changes the saved recipe, selects a Harness, or inherits the TTY.
 */
export async function prepareHarnessPackage(entry, options = {}) {
  const distribution = entry?.distribution
  const spec = distribution?.args?.[0]
  if (!['npx', 'uvx'].includes(distribution?.type) || typeof spec !== 'string'
    || !spec.trim() || spec.startsWith('-') || /[\x00-\x1f\x7f]/.test(spec)) {
    throw new Error('Harness package recipe must specify an npx or uvx package')
  }
  if (options.signal?.aborted) throw cancellationError()
  const runner = entry.runner ?? distribution.command ?? distribution.type
  const args = distribution.type === 'npx'
    ? ['--package', spec, '--', process.execPath, '-e', '']
    : ['--from', spec, '--', 'python', '-c', '']
  const progress = (phase, detail) => options.onProgress?.({ phase, detail: safeText(detail) })
  progress('preparing', `Preparing ${entry.label ?? entry.id ?? spec}`)
  if (options.signal?.aborted) throw cancellationError()

  await new Promise((resolve, reject) => {
    let child, timer, failure, stopping, closed = false, settled = false
    const output = { stdout: '', stderr: '' }
    const cleanup = () => {
      clearTimeout(timer)
      options.signal?.removeEventListener('abort', abort)
    }
    const finish = () => {
      if (!closed || stopping || settled) return
      settled = true
      cleanup()
      if (failure) reject(failure)
      else resolve()
    }
    const stop = (error) => {
      if (settled || failure) return
      failure = error
      clearTimeout(timer)
      stopping = stopProcessTree(child).finally(() => { stopping = undefined; finish() })
    }
    const abort = () => stop(cancellationError())
    const collect = (stream, line) => {
      output[stream] = `${output[stream]}${line}\n`.slice(-OUTPUT_LIMIT / 2)
      if (failure) return
      try { progress('download', line) }
      catch (error) { stop(new Error(safeText(error?.message ?? error, OUTPUT_LIMIT))) }
    }
    const stdout = outputLines((line) => collect('stdout', line))
    const stderr = outputLines((line) => collect('stderr', line))
    try {
      child = (options.spawnImpl ?? spawn)(runner, args, {
        cwd: options.cwd,
        env: { ...process.env, ...distribution.env, ...entry.env, ...options.env },
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
        detached: process.platform !== 'win32',
      })
    } catch (error) {
      reject(new Error(`Could not start package runner: ${safeText(error?.message ?? error)}`))
      return
    }
    child.stdout?.on('data', (chunk) => stdout.write(chunk))
    child.stderr?.on('data', (chunk) => stderr.write(chunk))
    child.stdout?.on('error', (error) => stop(new Error(safeText(error.message))))
    child.stderr?.on('error', (error) => stop(new Error(safeText(error.message))))
    child.once('error', (error) => {
      stop(new Error(`Could not start package runner: ${safeText(error.message)}`))
    })
    child.once('close', (code, signal) => {
      stdout.end(); stderr.end()
      closed = true
      if (!failure && code !== 0) {
        const tail = `${output.stdout}${output.stderr}`.trim()
        stop(new Error(`Harness package preparation ${signal ? `stopped by ${safeText(signal)}` : `exited with code ${code}`}${tail ? `\n${tail}` : ''}`))
      }
      if (!stopping && child.pid && process.platform !== 'win32') {
        // An installer may spawn a worker with ignored stdio then exit first.
        // A closed leader does not imply that its owned process group is empty.
        try {
          process.kill(-child.pid, 0)
          stopping = stopProcessTree(child).finally(() => { stopping = undefined; finish() })
        } catch { /* The owned process group has already exited. */ }
      }
      finish()
    })
    const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS
    timer = setTimeout(() => stop(new Error('Harness package preparation timed out')), timeoutMs)
    options.signal?.addEventListener('abort', abort, { once: true })
    // Cover cancellation from a synchronous spawn hook before the listener.
    if (options.signal?.aborted) abort()
  })
  if (options.signal?.aborted) throw cancellationError()
  progress('complete', 'Package ready')
  return entry
}

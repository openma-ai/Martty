import { DownloaderHelper } from 'node-downloader-helper'
import { lstatSync, statSync } from 'node:fs'
import { rm } from 'node:fs/promises'
import path from 'node:path'

function httpUrl(value, base) {
  const parsed = new URL(value, base)
  if (!['http:', 'https:'].includes(parsed.protocol)) {
    throw new Error('download requires an HTTP or HTTPS URL')
  }
  return parsed.href
}

// Compatibility boundary for the pinned SDK 2.1.11: its response callback can
// throw outside start()'s promise, and resolves relative redirects against the
// initial URL. Wrap that callback, not the transport or global HTTP modules.
// Re-run the redirect regressions when upgrading the SDK's private hook.
class ArchiveDownloader extends DownloaderHelper {
  __downloadRequest(resolve, reject) {
    const requestUrl = this.requestURL
    const request = super.__downloadRequest(resolve, reject)
    const [onResponse] = request.listeners('response')
    request.removeListener('response', onResponse)
    request.once('response', (response) => {
      try {
        if (response.statusCode >= 300 && response.statusCode < 400) {
          if (![301, 302, 303, 307, 308].includes(response.statusCode) || !response.headers.location) {
            throw new Error(`download returned HTTP ${response.statusCode} without a usable redirect`)
          }
          response.headers.location = httpUrl(response.headers.location, requestUrl)
        }
        onResponse.call(request, response)
      } catch (error) {
        response.destroy()
        request.destroy()
        this.emit('error', error)
        reject(error)
      }
    })
    return request
  }
}

/** Download into a caller-owned staging directory, without a total-time limit. */
export async function downloadFile(url, destination, options = {}) {
  const label = options.label ?? 'binary download'
  const cancelled = () => Object.assign(new Error(`${label} cancelled`), { name: 'AbortError' })
  if (options.signal?.aborted) throw cancelled()
  url = httpUrl(url)
  const filePath = path.resolve(destination)
  // This adapter owns only a new staging file, never a pre-existing user file.
  try {
    lstatSync(filePath)
    throw new Error(`download destination already exists: ${filePath}`)
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
  const connectTimeoutMs = options.connectTimeoutMs ?? 30_000
  const idleTimeoutMs = options.idleTimeoutMs ?? 60_000
  const maxBytes = options.maxBytes ?? 512 * 1024 * 1024
  const request = new AbortController()
  const downloader = new ArchiveDownloader(url, path.dirname(filePath), {
    fileName: path.basename(filePath),
    override: true,
    // The panel owns retry. SDK retry/resume can otherwise outlive cancellation.
    retry: false,
    resumeOnIncomplete: false,
    resumeIfFileExists: false,
    forceResume: false,
    removeOnStop: false,
    removeOnFail: false,
    httpRequestOptions: { signal: request.signal },
    httpsRequestOptions: { signal: request.signal },
  })

  return new Promise((resolve, reject) => {
    let state = 'running'
    let phase = 'connecting'
    let receivedBytes = 0
    let totalBytes
    let timer
    const cleanListeners = () => {
      clearTimeout(timer)
      options.signal?.removeEventListener('abort', onAbort)
    }
    const fail = (error) => {
      if (state !== 'running') return
      state = 'stopping'
      cleanListeners()
      request.abort(error)
      // SDK emits "download" before finishing stream setup. Let that setup
      // finish, then await closed handles before removing a partial file (Windows).
      void Promise.resolve().then(async () => {
        try {
          await downloader.stop()
          await rm(filePath, { force: true })
        } catch (cleanupError) {
          reject(new Error(`${error.message}; download cleanup failed: ${cleanupError.message}`, { cause: error }))
          return
        }
        reject(error)
      })
    }
    const onAbort = () => fail(cancelled())
    const resetTimer = () => {
      clearTimeout(timer)
      if (state !== 'running') return
      const duration = phase === 'connecting' ? connectTimeoutMs : idleTimeoutMs
      timer = setTimeout(() => fail(new Error(
        `${label} timed out (${phase} for ${duration}ms; received ${receivedBytes} bytes)`,
      )), duration)
    }
    const progress = (detail) => {
      if (state !== 'running') return
      try {
        options.onProgress?.({ phase: 'download', receivedBytes,
          ...(totalBytes === undefined ? {} : { totalBytes }),
          ...(detail === undefined ? {} : { detail }) })
      } catch (error) { fail(error) }
    }
    const tooLarge = () => fail(new Error(`${label} is larger than ${maxBytes} bytes`))
    downloader.on('download', (info) => {
      if (state !== 'running') return
      if (info.totalSize > maxBytes) { tooLarge(); return }
      totalBytes = info.totalSize > 0 ? info.totalSize : undefined
      phase = 'idle'
      resetTimer()
      progress('Connected; receiving archive data…')
    })
    downloader.on('progress', (info) => {
      if (state !== 'running') return
      if (info.downloaded > receivedBytes) {
        receivedBytes = info.downloaded
        resetTimer()
      }
      if (receivedBytes > maxBytes) { tooLarge(); return }
      progress()
    })
    downloader.on('end', (info) => {
      if (state !== 'running') return
      try {
        if (info.incomplete || path.resolve(info.filePath) !== filePath) {
          throw new Error(`${label} is incomplete or has an unexpected destination`)
        }
        const size = statSync(filePath).size
        if (size > maxBytes) { tooLarge(); return }
        if (totalBytes !== undefined && size !== totalBytes) {
          throw new Error(`${label} is incomplete: expected ${totalBytes} bytes, received ${size}`)
        }
        state = 'complete'
        cleanListeners()
        resolve({ filePath, receivedBytes: size })
      } catch (error) { fail(error) }
    })
    // Keep an error listener through stop/cleanup; native abort can emit late.
    downloader.on('error', (error) => fail(new Error(`${label} failed: ${error.message}`, { cause: error })))
    downloader.on('stop', () => fail(new Error(`${label} stopped before completion`)))
    options.signal?.addEventListener('abort', onAbort, { once: true })
    resetTimer()
    progress('Connecting to download server…')
    if (state !== 'running') return
    // start() resolves true on STOP as well as completion. Only "end" is success.
    downloader.start().catch((error) => fail(new Error(`${label} failed: ${error.message}`, { cause: error })))
  })
}

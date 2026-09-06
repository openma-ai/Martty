/** Run filesystem probes off the Client event loop; never launch an agent. */
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads'
import { discoverHarnessCandidates } from './harnesses.js'

export async function scanHarnessCandidates(settingsPath, options = {}, query = '') {
  const { signal } = options
  signal?.throwIfAborted()
  // Pass only discovery data, not transport callbacks or unrelated plugin options.
  const scoped = Object.fromEntries([
    'registry', 'defaults', 'pathValue', 'pathExt', 'platform', 'arch', 'installRoot',
  ].filter((key) => options[key] !== undefined).map((key) => [key, options[key]]))
  scoped.settingsPath = settingsPath
  const worker = new Worker(new URL(import.meta.url), {
    workerData: { harnessDiscovery: true, settingsPath, options: scoped, query },
    // No CLI/test-runner flags apply to this plain filesystem worker.
    execArgv: [],
  })
  return new Promise((resolve, reject) => {
    let settled = false
    const finish = (error, entries) => {
      if (settled) return
      settled = true
      signal?.removeEventListener('abort', cancel)
      void worker.terminate()
      if (error) reject(error)
      else resolve(entries)
    }
    const cancel = () => finish(signal.reason)
    signal?.addEventListener('abort', cancel, { once: true })
    worker.once('message', (entries) => finish(undefined, entries))
    worker.once('error', (error) => finish(error))
    worker.once('exit', (code) => {
      if (!settled) finish(new Error(`Harness discovery worker exited before returning results (${code})`))
    })
    if (signal?.aborted) cancel()
  })
}

if (!isMainThread && workerData?.harnessDiscovery === true) {
  parentPort.postMessage(discoverHarnessCandidates(workerData.settingsPath, workerData.options, workerData.query))
}

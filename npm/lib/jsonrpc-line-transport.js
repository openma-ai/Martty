/**
 * Newline-delimited JSON-RPC 2.0 over caller-owned streams.
 *
 * Local copy of the wire the native TUI speaks. Published `dsh` does not put
 * `@deepseek-ai/dsh-sdk-protocol` on the profile module fallback, and listing
 * that package as an npm dependency makes `dsh plugin add` warn about its
 * unmet peers (profile `autoInstallPeers: false`).
 */

import { StringDecoder } from 'node:string_decoder'

export class JsonRpcLineTransport {
  constructor(input, output) {
    this.input = input
    this.output = output
    this.buffer = ''
    this.decoder = new StringDecoder('utf8')
    this.started = false
    this.requestHandler = undefined
    this.notificationHandler = undefined
    this.pending = new Map()
  }

  start() {
    if (this.started) return
    this.started = true
    this.input.on('data', this.onData)
    this.input.on('error', this.onInputError)
    this.input.on('end', this.onInputEnd)
  }

  close() {
    this.input.off('data', this.onData)
    this.input.off('error', this.onInputError)
    this.input.off('end', this.onInputEnd)
    this.failPending(new Error('JSON-RPC transport closed'))
  }

  onRequest(handler) {
    this.requestHandler = handler
  }

  onNotification(handler) {
    this.notificationHandler = handler
  }

  notify(method, params) {
    this.write(
      params === undefined
        ? { jsonrpc: '2.0', method }
        : { jsonrpc: '2.0', method, params },
    )
  }

  flush() {
    return new Promise((resolve, reject) => {
      this.output.write('', (error) => {
        if (error) reject(error)
        else resolve()
      })
    })
  }

  onData = (chunk) => {
    this.buffer += typeof chunk === 'string' ? chunk : this.decoder.write(chunk)
    this.drainLines()
  }

  drainLines() {
    for (;;) {
      const newline = this.buffer.indexOf('\n')
      if (newline < 0) break
      const line = this.buffer.slice(0, newline).trim()
      this.buffer = this.buffer.slice(newline + 1)
      if (!line) continue
      void this.handleLine(line)
    }
  }

  onInputError = (error) => {
    this.failPending(error)
  }

  onInputEnd = () => {
    this.buffer += this.decoder.end()
    this.drainLines()
    this.failPending(new Error('JSON-RPC input closed'))
  }

  async handleLine(line) {
    let message
    try {
      message = JSON.parse(line)
    } catch {
      return
    }
    if (!message || typeof message !== 'object') return
    const id = message.id
    const method = message.method
    if ((typeof id === 'string' || typeof id === 'number') && typeof method === 'string') {
      await this.handleIncomingRequest(id, method, objectParams(message.params))
      return
    }
    if (typeof id === 'string' || typeof id === 'number') {
      this.handleIncomingResponse(id, message)
      return
    }
    if (typeof method === 'string') {
      this.notificationHandler?.(method, objectParams(message.params))
    }
  }

  async handleIncomingRequest(id, method, params) {
    const handler = this.requestHandler
    if (!handler) {
      this.write({ jsonrpc: '2.0', id, error: { code: -32601, message: `method not found: ${method}` } })
      return
    }
    try {
      const result = await handler(method, params)
      this.write({ jsonrpc: '2.0', id, result })
    } catch (error) {
      this.write({
        jsonrpc: '2.0',
        id,
        error: { code: -32603, message: error instanceof Error ? error.message : String(error) },
      })
    }
  }

  handleIncomingResponse(id, frame) {
    const pending = this.pending.get(id)
    if (!pending) return
    this.pending.delete(id)
    if (frame.error && typeof frame.error === 'object') {
      pending.reject(new Error(typeof frame.error.message === 'string' ? frame.error.message : 'JSON-RPC error'))
      return
    }
    pending.resolve(frame.result)
  }

  write(message) {
    this.output.write(`${JSON.stringify(message)}\n`)
  }

  failPending(error) {
    const pending = [...this.pending.values()]
    this.pending.clear()
    for (const waiter of pending) waiter.reject(error)
  }
}

function objectParams(params) {
  return params && typeof params === 'object' && !Array.isArray(params) ? params : {}
}

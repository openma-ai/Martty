/**
 * A minimal ACP agent that only explains why the real one is unavailable.
 *
 * In the dsh plugin profile the TUI Client attaches to the Host's
 * `acpServer`, which the ACP plugin provides. When that plugin cannot run on
 * this Host (a dsh below the floor, a required Host capability missing, or a
 * mount failure), nothing provides `acpServer`: the runner would wait
 * forever and dsh would only report pending entries. Providing this agent
 * instead lets the TUI start and show the reason in the conversation, with
 * the actions that fix it, while the profile stays otherwise intact.
 */

export const NOTICE_SESSION_ID = 'martty-host-notice'

function textUpdate(sessionId, text) {
  return {
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId,
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } },
    },
  }
}

/**
 * @param {string} message - Markdown shown as the agent's message.
 * @param {{ version?: string }} [options]
 * @returns {{ connect(stream: { readable: ReadableStream, writable: WritableStream }): { closed: Promise<void>, dispose(): Promise<void> } }}
 */
export function createHostNoticeServer(message, { version = '0.0.0' } = {}) {
  return {
    connect(stream) {
      const writer = stream.writable.getWriter()
      const reader = stream.readable.getReader()
      let disposed = false
      const send = (payload) => (disposed ? Promise.resolve() : writer.write(payload).catch(() => {}))
      const respond = (id, result) => send({ jsonrpc: '2.0', id, result })
      const fail = (id, code, text) => send({ jsonrpc: '2.0', id, error: { code, message: text } })

      async function handle(request) {
        const { id, method, params } = request ?? {}
        if (id === undefined || id === null) return // notifications need no answer
        switch (method) {
          case 'initialize':
            return respond(id, {
              protocolVersion: typeof params?.protocolVersion === 'number' ? params.protocolVersion : 1,
              agentCapabilities: { loadSession: false },
              authMethods: [],
              agentInfo: { name: 'martty-host-notice', title: 'Martty (dsh host unavailable)', version },
            })
          case 'session/new': {
            await respond(id, { sessionId: NOTICE_SESSION_ID })
            return send(textUpdate(NOTICE_SESSION_ID, message))
          }
          case 'session/prompt':
            await send(textUpdate(params?.sessionId ?? NOTICE_SESSION_ID, message))
            return respond(id, { stopReason: 'end_turn' })
          case 'session/cancel':
            return respond(id, null)
          default:
            return fail(id, -32601, message.split('\n')[0])
        }
      }

      const closed = (async () => {
        try {
          for (;;) {
            const { value, done } = await reader.read()
            if (done) break
            await handle(value)
          }
        } catch {
          // The Client went away; nothing to clean up beyond the streams.
        }
      })()
      return {
        closed,
        async dispose() {
          if (disposed) return
          disposed = true
          await reader.cancel().catch(() => {})
          await writer.close().catch(() => {})
        },
      }
    },
  }
}

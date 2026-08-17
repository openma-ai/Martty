import { EventEmitter } from 'node:events'

export const state = {
  serverApplies: 0,
  connections: [],
  nodeStreams: [],
  bootOptions: [],
  clientDisposes: 0,
  clientSpawns: [],
}

export function reset() {
  state.serverApplies = 0
  state.connections.length = 0
  state.nodeStreams.length = 0
  state.bootOptions.length = 0
  state.clientDisposes = 0
  state.clientSpawns.length = 0
}

export function spawnClient(command, args, options) {
  const child = new EventEmitter()
  child.exitCode = null
  child.stdin = { id: 'agent-to-client' }
  child.stdout = { id: 'client-to-agent' }
  child.stdio = [child.stdin, child.stdout, null, null, null]
  child.kill = (signal) => {
    child.killedWith = signal
    child.exitCode = 0
    return true
  }
  state.clientSpawns.push({ command, args, options, child })
  return child
}

export const serverPlugin = {
  name: 'acp-server',
  inject: [],
  apply(ctx) {
    state.serverApplies += 1
    if (ctx.get('acpServer') !== undefined) return
    ctx.provide('acpServer', makeServer('fallback'))
  },
}

export const serverName = serverPlugin.name
export const serverInject = serverPlugin.inject
export function applyServer(ctx) {
  return serverPlugin.apply(ctx)
}

export function makeServer(owner) {
  return {
    owner,
    connect(stream) {
      const connection = {
        stream,
        disposed: false,
        dispose() {
          connection.disposed = true
        },
        then(resolve) {
          resolve()
        },
      }
      state.connections.push(connection)
      return connection
    },
  }
}

export function nodeAcpStream(input, output) {
  const stream = { input, output, protocol: 'acp' }
  state.nodeStreams.push(stream)
  return stream
}

export async function bootClient(options) {
  state.bootOptions.push(options)
  return {
    root: {
      fiber: {
        async dispose() {
          state.clientDisposes += 1
        },
      },
    },
  }
}

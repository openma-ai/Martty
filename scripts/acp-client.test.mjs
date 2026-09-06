import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { PassThrough } from 'node:stream'
import { createInterface } from 'node:readline'
import test from 'node:test'
import { apply, resolveAgent } from '../npm/lib/acp-client.js'
import { resolveStackedAgent } from '../npm/lib/agent.js'
import { parseClientArgv, painterArgs } from '../npm/lib/boot.js'
import { muxAcpAndCompositor } from '../npm/lib/mux.js'

test('resolveAgent defaults to dsh-acp and honors command/args', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  try {
    assert.deepEqual(resolveAgent(), { command: 'dsh-acp', args: [] })
    assert.deepEqual(resolveAgent({ agent: { command: 'dsh', args: ['--profile', 'acp'] } }), {
      command: 'dsh',
      args: ['--profile', 'acp'],
    })
    process.env.DSH_TUI_AGENT = 'codex --acp'
    assert.deepEqual(resolveAgent(), { command: 'codex', args: ['--acp'] })
  } finally {
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('standalone resolution uses the default harness from Martty settings', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-default-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'local',
        label: 'Local ACP',
        command: '/opt/local/bin/local-acp',
        args: ['--stdio'],
      }],
      defaultHarness: 'local',
    }))
    assert.deepEqual(resolveStackedAgent(import.meta.url, { settingsPath }), {
      command: '/opt/local/bin/local-acp',
      args: ['--stdio'],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('a forced product Harness wins the saved default Harness at startup', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-forced-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  const forcedHarness = {
    id: 'product-default',
    label: 'Product Default',
    command: 'product-acp',
    args: ['--stdio'],
  }
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'last-used',
        label: 'Last Used',
        command: 'last-used-acp',
        args: [],
      }],
      defaultHarness: 'last-used',
    }))

    assert.deepEqual(
      parseClientArgv([], { settingsPath, forcedHarness }).agent,
      { command: 'product-acp', args: ['--stdio'] },
    )
    assert.equal(
      JSON.parse(readFileSync(settingsPath, 'utf8')).defaultHarness,
      'last-used',
      'startup resolution must not overwrite the last user selection',
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('an empty forced Harness falls back to the saved default Harness', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-empty-forced-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'last-used',
        label: 'Last Used',
        command: 'last-used-acp',
        args: [],
      }],
      defaultHarness: 'last-used',
    }))

    assert.deepEqual(
      parseClientArgv([], { settingsPath, forcedHarness: null }).agent,
      { command: 'last-used-acp', args: [] },
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('parseClientArgv rejects missing startup values before resolving defaults', () => {
  for (const argv of [['--agent'], ['--agent', ''], ['--agent', '--theme'], ['--agent-arg'], ['--agent', 'cmd', '--agent-arg']]) {
    assert.throws(() => parseClientArgv(argv), error => error.exitCode === 2 && /needs a value/.test(error.message))
  }
})

test('environment Harness override preserves quoted executable paths and arguments without a shell', () => {
  const before = process.env.DSH_TUI_AGENT
  try {
    process.env.DSH_TUI_AGENT = '"/opt/Agent Tools/acp" --name "hello world" ""'
    const expected = { command: '/opt/Agent Tools/acp', args: ['--name', 'hello world', ''] }
    assert.deepEqual(resolveStackedAgent(), expected)
    assert.deepEqual(resolveAgent({}), expected)
    process.env.DSH_TUI_AGENT = '"C:\\Program Files\\Agent\\acp.exe" --stdio'
    assert.equal(resolveStackedAgent().command, 'C:\\Program Files\\Agent\\acp.exe')
  } finally {
    if (before === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = before
  }
})

test('parseClientArgv strips agent flags for the painter', () => {
  const parsed = parseClientArgv([
    '--theme',
    'dark',
    '--agent',
    'dsh',
    '--agent-arg',
    '--profile',
    '--agent-arg',
    'acp',
    '-w',
    '/tmp/ws',
  ])
  assert.deepEqual(parsed.agent, { command: 'dsh', args: ['--profile', 'acp'] })
  assert.deepEqual(parsed.rustArgs, ['--theme', 'dark', '-w', '/tmp/ws'])
  assert.deepEqual(painterArgs(parsed), [
    '--theme',
    'dark',
    '-w',
    '/tmp/ws',
    '--agent',
    'dsh',
    '--agent-arg',
    '--profile',
    '--agent-arg',
    'acp',
  ])
})

test('parseClientArgv uses the selected harness when no CLI override is present', () => {
  const previous = process.env.DSH_TUI_AGENT
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-argv-harness-'))
  const settingsPath = path.join(root, 'settings.json')
  try {
    writeFileSync(settingsPath, JSON.stringify({
      harnesses: [{
        id: 'other',
        label: 'Other ACP',
        command: 'other-acp',
        args: ['serve'],
      }],
      defaultHarness: 'other',
    }))
    assert.deepEqual(parseClientArgv(['--theme', 'dark'], { settingsPath }), {
      agent: { command: 'other-acp', args: ['serve'] },
      rustArgs: ['--theme', 'dark'],
    })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previous === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previous
  }
})

test('standalone argv resolution reads the default harness from MARTTY_HOME by default', () => {
  const previousAgent = process.env.DSH_TUI_AGENT
  const previousHome = process.env.MARTTY_HOME
  delete process.env.DSH_TUI_AGENT
  const root = mkdtempSync(path.join(tmpdir(), 'martty-default-harness-home-'))
  process.env.MARTTY_HOME = root
  try {
    writeFileSync(path.join(root, 'settings.json'), JSON.stringify({
      harnesses: [{
        id: 'home',
        label: 'Home ACP',
        command: 'home-acp',
        args: [],
      }],
      defaultHarness: 'home',
    }))
    assert.deepEqual(parseClientArgv([]).agent, { command: 'home-acp', args: [] })
  } finally {
    rmSync(root, { recursive: true, force: true })
    if (previousAgent === undefined) delete process.env.DSH_TUI_AGENT
    else process.env.DSH_TUI_AGENT = previousAgent
    if (previousHome === undefined) delete process.env.MARTTY_HOME
    else process.env.MARTTY_HOME = previousHome
  }
})

test('apply with a nonexistent agent command does not crash with an uncaught exception', async () => {
  const ctx = {}
  apply(ctx, { agent: { command: 'martty-definitely-missing-agent' } })
  assert.equal(ctx.acpClient.kind, 'spawn')
  // The child emits 'error' (ENOENT) asynchronously; without a listener Node
  // would kill the whole process before this assertion runs.
  await new Promise((resolve) => setTimeout(resolve, 200))
  assert.ok(ctx.acpClient.stdin.destroyed, 'stdin destroyed after failed spawn')
  assert.ok(ctx.acpClient.stdout.destroyed, 'stdout destroyed after failed spawn')
})

test('standalone ACP client replaces its child without replacing the transport', async () => {
  const ctx = {}
  apply(ctx, {
    agent: {
      command: process.execPath,
      args: ['-e', 'setInterval(() => {}, 10000)', 'first'],
    },
  })
  const service = ctx.acpClient
  const stableInput = service.stdin
  const stableOutput = service.stdout
  const first = service.child
  const switches = []
  try {
    service.onSwitch((next, previous) => switches.push({ next, previous }))
    await service.switchAgent({
      command: process.execPath,
      args: ['-e', 'setInterval(() => {}, 10000)', 'second'],
    })

    assert.equal(service.stdin, stableInput)
    assert.equal(service.stdout, stableOutput)
    assert.notEqual(service.child, first)
    assert.equal(switches.length, 1)
    assert.equal(switches[0].previous.child, first)
    assert.equal(switches[0].next.child, service.child)
    if (first.exitCode === null) await once(first, 'exit')
    assert.notEqual(first.signalCode, null)
  } finally {
    service.close?.()
    if (service.child?.exitCode === null) service.child.kill('SIGTERM')
    if (first.exitCode === null) first.kill('SIGTERM')
  }
})

function switchingClient(t) {
  const ctx = {}
  apply(ctx, { agent: { command: process.execPath, args: ['-e', 'setInterval(() => {}, 10000)'] } })
  const service = ctx.acpClient
  const tui = { input: new PassThrough(), output: new PassThrough() }
  const responses = createInterface({ input: tui.output })
  const mux = muxAcpAndCompositor({
    agent: service,
    tui,
    onAcp(direction, message) {
      if (direction === 'client') {
        service.observeClient?.(message)
        ctx.acpClientEvents.observeClient(message)
      } else {
        service.observeAgent?.(message)
        ctx.acpClientEvents.observeAgent(message)
      }
    },
  })
  service.onSwitch(() => mux.resetAgent())
  service.onFailure?.((error) => mux.failAgent(error))
  t.after(() => {
    service.close()
    responses.close()
    tui.input.destroy()
    tui.output.destroy()
  })
  return {
    service,
    async request(method, id) {
      const reply = once(responses, 'line')
      tui.input.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params: {} })}\n`)
      return JSON.parse((await reply)[0])
    },
  }
}

function acpFixture(body) {
  return {
    command: process.execPath,
    args: ['-e', `
      const lines = require('node:readline').createInterface({ input: process.stdin });
      let setupCount = 0;
      lines.on('line', line => {
        const request = JSON.parse(line);
        const reply = value => process.stdout.write(JSON.stringify({jsonrpc:'2.0', id:request.id, ...value}) + '\\n');
        ${body}
      });
    `],
  }
}

test('switch handoff is distinct from readiness after initialize and session/new', async (t) => {
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(`
    if (request.method === 'initialize') reply({result:{protocolVersion:1, agentInfo:{name:'example'}}});
    else if (request.method === 'session/new') reply({result:{sessionId:'new-session'}});
  `))
  assert.ok(switched?.ready instanceof Promise, 'OS spawn must return separate ACP readiness')
  let ready = false
  switched.ready.then(() => { ready = true })
  await client.request('initialize', 1)
  assert.equal(ready, false, 'initialize alone does not establish an ACP session')
  await client.request('session/new', 2)
  assert.deepEqual(await switched.ready, { sessionId: 'new-session', server: 'example' })
})

test('switch readiness rejects initialization failure', async (t) => {
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(`reply({error:{code:-32603,message:'broken setup'}});`))
  assert.ok(switched?.ready instanceof Promise)
  const failed = assert.rejects(switched.ready, /initialize.*broken setup/)
  await client.request('initialize', 1)
  await failed
})

test('switch readiness waits for sign-in before accepting a new session', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(`
    if (request.method === 'initialize') reply({result:{protocolVersion:1}});
    else if (request.method === 'session/new' && setupCount++ === 0) reply({error:{code:-32000,message:'sign in'}});
    else if (request.method === 'session/new') reply({result:{sessionId:'signed-in'}});
    else if (request.method === 'authenticate') reply({result:{}});
  `), { timeoutMs: 500 })
  assert.ok(switched?.ready instanceof Promise)
  let settled = false
  switched.ready.finally(() => { settled = true }).catch(() => {})
  await client.request('initialize', 1)
  await client.request('session/new', 2)
  t.mock.timers.tick(550)
  assert.equal(settled, false)
  await client.request('authenticate', 3)
  await client.request('session/new', 4)
  assert.deepEqual(await switched.ready, { sessionId: 'signed-in' })
})

test('browser authentication can outlast the setup timeout without killing the switched Harness', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(`
    if (request.method === 'initialize') reply({result:{protocolVersion:1}});
    else if (request.method === 'session/new' && setupCount++ === 0) reply({error:{code:-32000,message:'sign in'}});
    else if (request.method === 'session/new') reply({result:{sessionId:'signed-in'}});
    else if (request.method === 'authenticate') setTimeout(() => reply({result:{}}), 100);
  `), { timeoutMs: 500 })
  let settled = false
  switched.ready.finally(() => { settled = true }).catch(() => {})
  await client.request('initialize', 1)
  await client.request('session/new', 2)
  const authentication = client.request('authenticate', 3)
  t.mock.timers.tick(501)
  await Promise.resolve()
  assert.equal(settled, false, 'human sign-in must not consume the machine setup deadline')
  assert.deepEqual((await authentication).result, {})
  await client.request('session/new', 4)
  assert.deepEqual(await switched.ready, { sessionId: 'signed-in' })
})

test('switch setup has a twenty-minute default deadline', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(''))
  let settled = false
  switched.ready.finally(() => { settled = true }).catch(() => {})
  const failed = assert.rejects(switched.ready, /ACP setup timed out after 1200s/)
  failed.catch(() => {})
  const response = client.request('initialize', 1)
  t.mock.timers.tick(20 * 60_000 - 1)
  await Promise.resolve()
  assert.equal(settled, false, 'setup must remain alive until twenty minutes')
  t.mock.timers.tick(1)
  assert.match((await response).error.message, /timed out after 1200s/)
  await failed
  assert.equal(client.service.stdin.destroyed, false)
})

test('switch timeout releases the pending ACP request so the client can retry', async (t) => {
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(''), { timeoutMs: 80 })
  assert.ok(switched?.ready instanceof Promise)
  const failed = assert.rejects(switched.ready, /timed out/)
  const response = await client.request('initialize', 1)
  assert.match(response.error.message, /timed out/)
  await failed
  assert.equal(client.service.stdin.destroyed, false)
})

test('switch readiness rejects when its process exits before an ACP response', async (t) => {
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture('process.exit(7);'))
  assert.ok(switched?.ready instanceof Promise)
  const failed = assert.rejects(switched.ready, /exited.*7/)
  const response = await client.request('initialize', 1)
  assert.match(response.error.message, /exited.*7/)
  await failed
})

test('closing during a switch rejects readiness instead of leaving a pending promise', async (t) => {
  const client = switchingClient(t)
  const switched = await client.service.switchAgent(acpFixture(''))
  assert.ok(switched?.ready instanceof Promise)
  const failed = assert.rejects(switched.ready, /closed/)
  client.service.close()
  await failed
})

test('a child exiting during handoff leaves the previous process running', async (t) => {
  const client = switchingClient(t)
  const previous = client.service.child
  client.service.onSwitch(async (next) => { await once(next.child, 'exit') })
  await assert.rejects(client.service.switchAgent({
    command: process.execPath,
    args: ['-e', 'process.exit(9)'],
  }), /exited.*9/)
  assert.equal(client.service.child, previous)
  assert.equal(previous.killed, false)
})

function isolatedAcpClient(script) {
  return spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { once } from 'node:events';
    import { createInterface } from 'node:readline';
    import { apply } from ${JSON.stringify(new URL('../npm/lib/acp-client.js', import.meta.url).href)};
    ${script}
  `], { encoding: 'utf8', timeout: 10_000, maxBuffer: 4 * 1024 * 1024 })
}

test('agent stderr is drained privately and exposes only a bounded sanitized diagnostic tail', () => {
  const agent = `
    const { once } = require('node:events');
    (async () => {
      process.stderr.write('old diagnostic must expire\\n');
      if (!process.stderr.write('x'.repeat(1024 * 1024))) await once(process.stderr, 'drain');
      process.stderr.write('\\n\\x1b[');
      await new Promise(resolve => setTimeout(resolve, 10));
      process.stderr.write('31mWarning: local diagnostic\\x1b[0m\\n');
      process.stderr.end('\\x1b]0;do not set title\\x07last detail\\x00\\x08\\x7f\\x85\\r\\n');
      process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:1,result:{protocolVersion:1}}) + '\\n');
    })();
  `
  const result = isolatedAcpClient(`
    const ctx = {};
    apply(ctx, {agent:{command:process.execPath,args:['-e',${JSON.stringify(agent)}]}});
    const service = ctx.acpClient;
    let protocol = '';
    service.stdout.on('data', chunk => { protocol += chunk; });
    await once(service.child, 'close');
    const diagnostics = service.diagnostics?.() ?? '';
    service.close();
    process.stdout.write(JSON.stringify({protocol,diagnostics}));
  `)
  assert.equal(result.status, 0, result.error?.message ?? result.stderr)
  assert.equal(result.stderr.length, 0, 'agent stderr must never inherit the user terminal')
  const { protocol, diagnostics } = JSON.parse(result.stdout)
  assert.deepEqual(JSON.parse(protocol), { jsonrpc: '2.0', id: 1, result: { protocolVersion: 1 } })
  assert.ok(Buffer.byteLength(diagnostics, 'utf8') <= 8192, 'retain at most 8 KiB per child')
  assert.match(diagnostics, /Warning: local diagnostic\nlast detail/)
  assert.doesNotMatch(diagnostics, /old diagnostic|do not set title/)
  assert.doesNotMatch(diagnostics, /[\x00-\x09\x0b-\x1f\x7f-\x9f]/, 'diagnostics cannot execute terminal controls')
})

test('failed initial spawn reports its error without console or inherited stderr output', () => {
  const result = isolatedAcpClient(`
    const ctx = {};
    apply(ctx,{agent:{command:'martty-definitely-missing-agent-for-stderr-test'}});
    const failures = [];
    ctx.acpClient.onFailure(error => failures.push(error.message));
    await new Promise(resolve => ctx.acpClient.child.once('close', resolve));
    process.stdout.write(JSON.stringify({failures}));
    ctx.acpClient.close();
  `)
  assert.equal(result.status, 0, result.error?.message ?? result.stderr)
  assert.equal(result.stderr.length, 0, 'spawn failures must be surfaced through the client, not console')
  assert.match(JSON.parse(result.stdout).failures[0], /ENOENT/)
})

test('readiness exit failure includes the final sanitized stderr after a large write', () => {
  const agent = `
    const { once } = require('node:events');
    require('node:readline').createInterface({input:process.stdin}).once('line', async () => {
      if (!process.stderr.write('y'.repeat(1024 * 1024))) await once(process.stderr, 'drain');
      process.stderr.end('\\n\\x1b[31mMissing credential\\x1b[0m\\x07\\n', () => process.exit(23));
    });
  `
  const result = isolatedAcpClient(`
    const ctx = {};
    apply(ctx,{agent:{command:process.execPath,args:['-e','setInterval(() => {}, 10000)']}});
    const service = ctx.acpClient;
    const {ready} = await service.switchAgent({command:process.execPath,args:['-e',${JSON.stringify(agent)}]});
    const closed = once(service.child, 'close');
    const request = {jsonrpc:'2.0',id:1,method:'initialize',params:{}};
    service.observeClient(request);
    service.stdin.write(JSON.stringify(request)+'\\n');
    const error = await ready.then(() => null, error => error.message);
    await closed;
    service.close();
    process.stdout.write(JSON.stringify({error}));
  `)
  assert.equal(result.status, 0, result.error?.message ?? result.stderr)
  assert.equal(result.stderr.length, 0)
  const { error } = JSON.parse(result.stdout)
  assert.match(error, /exited.*23/)
  assert.match(error, /Agent stderr:\n[\s\S]*Missing credential/)
  assert.ok(Buffer.byteLength(error, 'utf8') < 8400)
  assert.doesNotMatch(error, /[\x00-\x09\x0b-\x1f\x7f-\x9f]/)
})

test('ACP setup rejection includes private stderr and a new child resets the diagnostic tail', () => {
  const agent = `
    require('node:readline').createInterface({input:process.stdin}).once('line', line => {
      const request = JSON.parse(line);
      process.stderr.write('\\x1b[31mLogin provider unavailable\\x1b[0m\\n', () => {
        process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,error:{code:-32603,message:'setup rejected',data:{code:'ENOENT'}}})+'\\n');
      });
    });
  `
  const result = isolatedAcpClient(`
    const ctx = {};
    apply(ctx,{agent:{command:process.execPath,args:['-e','setInterval(() => {}, 10000)']}});
    const service = ctx.acpClient;
    const {ready} = await service.switchAgent({command:process.execPath,args:['-e',${JSON.stringify(agent)}]});
    const lines = createInterface({input:service.stdout});
    const response = once(lines, 'line');
    // Both channels are independent. Observe the diagnostic data before
    // processing the real setup response received on stdout.
    const stderr = service.child.stderr ? once(service.child.stderr, 'data') : Promise.resolve();
    const request = {jsonrpc:'2.0',id:1,method:'initialize',params:{}};
    service.observeClient(request);
    service.stdin.write(JSON.stringify(request)+'\\n');
    await stderr;
    service.observeAgent(JSON.parse((await response)[0]));
    const failure = await ready.then(() => null, error => error);
    const error = failure.message;
    const structured = {method:failure.method, acpError:failure.acpError};
    await service.switchAgent({command:process.execPath,args:['-e','setInterval(() => {}, 10000)']});
    const diagnostics = service.diagnostics?.() ?? '';
    lines.close();
    service.close();
    process.stdout.write(JSON.stringify({error,diagnostics,structured}));
  `)
  assert.equal(result.status, 0, result.error?.message ?? result.stderr)
  assert.equal(result.stderr.length, 0)
  const { error, diagnostics, structured } = JSON.parse(result.stdout)
  assert.deepEqual(structured, { method: 'initialize', acpError: {
    code: -32603, message: 'setup rejected', data: { code: 'ENOENT' },
  } }, 'diagnostic wrapping must preserve the ACP error and request method')
  assert.match(error, /initialize: setup rejected[\s\S]*Login provider unavailable/)
  assert.doesNotMatch(error, /\x1b/)
  assert.equal(diagnostics, '', 'warnings from a previous agent must not contaminate the next one')
})

test('Windows command shims use the platform adapter at the OS spawn boundary', () => {
  // Simulate only the platform and OS call in an isolated process. The real
  // Windows-only test below checks the corresponding executable/argv behavior.
  const script = `
    import childProcess from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    import { EventEmitter } from 'node:events';
    import { PassThrough } from 'node:stream';
    Object.defineProperty(process, 'platform', {value:'win32'});
    const calls = [];
    childProcess.spawn = (command, args, options) => {
      calls.push({command,args,options});
      const child = new EventEmitter();
      Object.assign(child,{pid:42,exitCode:null,stdin:new PassThrough(),stdout:new PassThrough(),stderr:new PassThrough(),kill(){}});
      return child;
    };
    syncBuiltinESMExports();
    const { apply } = await import(${JSON.stringify(new URL('../npm/lib/acp-client.js', import.meta.url).href)});
    const ctx = {};
    apply(ctx,{agent:{command:'C:\\\\Program Files\\\\example-acp.cmd',args:['hello world','a&b']}});
    ctx.acpClient.close();
    process.stdout.write(JSON.stringify(calls[0]));
  `
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', script], { encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
  const call = JSON.parse(result.stdout)
  assert.match(call.command, /(?:^|[\\/])cmd\.exe$/i)
  assert.deepEqual(call.args.slice(0, 3), ['/d', '/s', '/c'])
  assert.equal(call.options.shell, undefined, 'no shell:true with unescaped user arguments')
  assert.equal(call.options.windowsVerbatimArguments, true)
})

test('Windows ACP command shims preserve spaces and metacharacters', { skip: process.platform !== 'win32' }, async (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty cmd shim '))
  const binRoot = path.join(root, 'node_modules', '.bin')
  mkdirSync(binRoot, { recursive: true })
  const scriptPath = path.join(root, 'echo arguments.cjs')
  const shimPath = path.join(binRoot, 'echo-acp.cmd')
  writeFileSync(scriptPath, 'process.stdout.write(JSON.stringify(process.argv.slice(2))+"\\n");')
  writeFileSync(shimPath, `@ECHO off\r\n"${process.execPath}" "${scriptPath}" %*\r\n`)
  const args = ['hello world', 'a&b', '(a|b)', 'semi;colon', 'quoted "value"', 'trailing\\']
  const ctx = {}
  apply(ctx, { agent: { command: shimPath, args } })
  t.after(() => {
    ctx.acpClient.close()
    rmSync(root, { recursive: true, force: true })
  })
  const lines = createInterface({ input: ctx.acpClient.stdout })
  t.after(() => lines.close())
  assert.deepEqual(JSON.parse((await once(lines, 'line'))[0]), args)
})

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { createInterface } from 'node:readline'
import test from 'node:test'
import { apply, resolveAgent } from '../npm/lib/acp-client.js'
import { resolveStackedAgent } from '../npm/lib/agent.js'
import { parseClientArgv, painterArgs } from '../npm/lib/boot.js'

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
  assert.equal(ctx.acpClient.stdin.destroyed, false, 'the client stays available for another Harness')
  assert.equal(ctx.acpClient.stdout.destroyed, false)
  ctx.acpClient.close()
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
  assert.equal(protocol, '', 'unsolicited response ids cannot enter the client connection')
  assert.ok(Buffer.byteLength(diagnostics, 'utf8') <= 8192, 'retain at most 8 KiB per child')
  assert.match(diagnostics, /Warning: local diagnostic\nlast detail/)
  assert.doesNotMatch(diagnostics, /old diagnostic|do not set title/)
  assert.doesNotMatch(diagnostics, /[\x00-\x09\x0b-\x1f\x7f-\x9f]/, 'diagnostics cannot execute terminal controls')
})

test('failed initial spawn answers the pending request without inherited stderr', () => {
  const result = isolatedAcpClient(`
    const ctx = {};
    apply(ctx,{agent:{command:'martty-definitely-missing-agent-for-stderr-test'}});
    const lines = createInterface({input:ctx.acpClient.stdout});
    const response = once(lines,'line');
    ctx.acpClient.stdin.write(JSON.stringify({jsonrpc:'2.0',id:1,method:'initialize',params:{}})+'\\n');
    process.stdout.write((await response)[0]);
    ctx.acpClient.close();
    lines.close();
  `)
  assert.equal(result.status, 0, result.error?.message ?? result.stderr)
  assert.equal(result.stderr.length, 0)
  assert.match(JSON.parse(result.stdout).error.message, /ENOENT/)
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
  writeFileSync(scriptPath, `require('node:readline').createInterface({input:process.stdin}).on('line', line => { const request = JSON.parse(line); process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,result:{argv:process.argv.slice(2)}})+'\\n'); });`)
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
  const reply = once(lines, 'line')
  ctx.acpClient.stdin.write(JSON.stringify({jsonrpc:'2.0',id:1,method:'initialize',params:{}})+'\n')
  assert.deepEqual(JSON.parse((await reply)[0]).result.argv, args)
})

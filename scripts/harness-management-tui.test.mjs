import assert from 'node:assert/strict'
import { existsSync, readFileSync, rmSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import test from 'node:test'
import { createHarnessDiscoveryScenario } from './harness-discovery-scenario.mjs'
import { discoverHarnessCandidates, upsertHarness } from '../npm/lib/harnesses.js'

test('CLI terminal removal defaults to no, handles Ctrl-C, and deletes only after yes', { skip: process.platform === 'win32' }, t => {
  const scenario = createHarnessDiscoveryScenario()
  t.after(() => rmSync(scenario.root, { recursive: true, force: true }))
  const script = String.raw`
import pexpect, sys, os, json
env = dict(os.environ, MARTTY_HOME=os.path.dirname(sys.argv[3]))
for answer, code in [('n', 0), ('interrupt', 130), ('y', 0)]:
    c = pexpect.spawn(sys.argv[1], [sys.argv[2], 'harness', 'remove', 'fixture-initial'], env=env, encoding='utf-8', timeout=10)
    try:
        c.expect('Remove this Harness')
        if answer == 'interrupt': c.sendcontrol('c')
        else: c.sendline(answer)
        c.expect(pexpect.EOF); c.close()
        assert c.exitstatus == code, (c.exitstatus, c.before)
        with open(sys.argv[3]) as f: settings = json.load(f)
        assert bool(settings['harnesses']) == (answer != 'y'), settings
    finally:
        if c.isalive(): c.close(force=True)
`
  const run = spawnSync('python3', ['-c', script, process.execPath, path.join(import.meta.dirname, '../npm/bin/martty.js'), scenario.settingsPath], {
    encoding: 'utf8', timeout: 35000,
  })
  assert.equal(run.status, 0, `${run.stdout}\n${run.stderr}`)
})

test('Real TUI keyboard flow removes a private installation, shows errors directly, retries and recovers', { skip: process.platform === 'win32' }, t => {
  const scenario = createHarnessDiscoveryScenario()
  t.after(() => rmSync(scenario.root, { recursive: true, force: true }))
  const candidates = discoverHarnessCandidates(scenario.settingsPath, { settingsPath: scenario.settingsPath, registry: scenario.registry, pathValue: scenario.pathValue })
  const managed = candidates.find(entry => entry.id === 'fixture-managed-binary')
  assert.equal(managed.status, 'Installed')
  upsertHarness(scenario.settingsPath, { ...managed, command: managed.resolvedCommand ?? managed.command })
  upsertHarness(scenario.settingsPath, { id: 'fixture-failure', label: 'Fixture Failure', command: process.execPath,
    args: [scenario.agentScript, 'acp', 'failure'] })
  const script = String.raw`
import pexpect, sys, time, json, os
c = pexpect.spawn(sys.argv[1], [sys.argv[2], '--tui', '--root', sys.argv[3]], encoding='utf-8', timeout=15, dimensions=(50,160))
c.logfile_read = sys.stdout
def requests(role, count):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        with open(os.path.join(sys.argv[3], 'fixture-events.jsonl')) as f:
            events = [json.loads(line) for line in f if line.endswith('\n')]
        if sum(e.get('role') == role and e.get('method') == 'session/new' for e in events) >= count:
            time.sleep(.2) # Let the response's native overlay frame settle, not a repeated older frame.
            return
        c.expect(pexpect.TIMEOUT, timeout=.02) # Drain PTY output so native painting cannot block the handshake.
    raise AssertionError('ACP request did not occur: ' + role)
try:
    c.expect('fixture-initial-model')
    c.send('/harness\r')
    c.expect('Default Harness')
    c.send('\x1b[3~') # Current row is protected.
    c.send('\x1b[B') # Select the saved private installation.
    c.expect('delete') # Incremental terminal paint may reuse cells in "remove".
    c.send('\x1b[3~')
    c.expect('Remove configuration only')
    c.send('\x1b[B\r')
    c.expect('Permanently delete private installation')
    c.send('\x1b'); time.sleep(.15)
    c.expect('Remove configuration only')
    c.send('\x1b'); time.sleep(.15)
    c.expect('Default Harness')
    c.send('\x1b[3~') # Returned to the same removable row, not the current Harness.
    c.expect('Remove configuration only')
    c.send('\x1b[B\r')
    c.expect('Permanently delete private installation')
    c.send('\r')
    c.expect('Harness removed')
    c.send('\x1b'); time.sleep(.15)
    c.send('/harness fixture-failure\r')
    c.expect('Default Harness saved')
    c.send('\r')
    c.expect('Fixture executable is missing')
    c.expect('fixture diagnostic: missing dependency')
    c.send('/new\r')
    requests('failure', 2)
    c.send('/harness fixture-initial --new\r')
    requests('initial', 2)
    c.expect('fixture-initial-model')
    c.send('/quit\r')
    c.expect(pexpect.EOF)
finally:
    if c.isalive(): c.terminate(force=True)
`
  const run = spawnSync('python3', ['-c', script, process.execPath, path.join(import.meta.dirname, 'harness-discovery-scenario.mjs'), scenario.root], {
    encoding: 'utf8', timeout: 90_000, maxBuffer: 8 * 1024 * 1024,
  })
  assert.equal(run.status, 0, `${run.error ?? ''}\n${run.stdout?.slice(-16000)}\n${run.stderr}`)
  assert.ok(!existsSync(managed.resolvedCommand ?? managed.command))
  const settings = JSON.parse(readFileSync(scenario.settingsPath))
  assert.ok(!settings.harnesses.some(entry => entry.id === managed.id))
  assert.equal(settings.defaultHarness, 'fixture-initial')
  const events = readFileSync(scenario.eventsPath, 'utf8').trim().split('\n').map(JSON.parse)
  assert.equal(events.filter(event => event.role === 'failure' && event.method === 'session/new').length, 2)
  assert.ok(!run.stdout.includes('Harness recovery'))
})

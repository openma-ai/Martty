import assert from 'node:assert/strict'
import { readFileSync, rmSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import test from 'node:test'
import { createHarnessDiscoveryScenario } from './harness-discovery-scenario.mjs'
import { upsertHarness } from '../npm/lib/harnesses.js'

test('empty tab → Harness default → new tab → chat → /new preserves every session', { skip: process.platform === 'win32', timeout: 60000 }, t => {
  const scenario = createHarnessDiscoveryScenario()
  t.after(() => rmSync(scenario.root, { recursive: true, force: true }))
  upsertHarness(scenario.settingsPath, { id:'fixture-beta', label:'Offline Beta', command:process.execPath,
    args:[scenario.agentScript,'acp','beta'] })
  const script = String.raw`
import pexpect, sys, os, json, time
c = pexpect.spawn(sys.argv[1], [sys.argv[2], '--tui', '--root', sys.argv[3]], encoding='utf-8', timeout=15, dimensions=(40,140))
c.logfile_read = sys.stdout

def events():
    try:
        with open(os.path.join(sys.argv[3], 'fixture-events.jsonl')) as f:
            return [json.loads(line) for line in f if line.endswith('\n')]
    except FileNotFoundError: return []

def drain(seconds=.3): c.expect(pexpect.TIMEOUT, timeout=seconds)
def wait_request(role, method, count):
    deadline = time.time()+10
    while time.time()<deadline:
        hits = [e for e in events() if e.get('role') == role and e.get('method') == method]
        if len(hits)>=count:
            drain()
            return hits[-1]
        drain(.03)
    raise AssertionError((role, method, count, events()))
def command(text):
    c.send(text+'\r')
    drain()
try:
    wait_request('initial','session/new',1)
    command('/harness fixture-beta')
    with open(os.path.join(sys.argv[3],'.martty','settings.json')) as f:
        assert json.load(f)['defaultHarness']=='fixture-beta'
    assert not any(e.get('role')=='beta' for e in events()), 'Saving must not start beta'
    c.send('\r') # Immediate open uses the native /new action.
    wait_request('beta','session/new',1)
    command('beta-first')
    assert wait_request('beta','session/prompt',1)['sessionId']=='fixture-beta-session'
    command('/new')
    wait_request('beta','session/new',2)
    command('beta-second')
    assert wait_request('beta','session/prompt',2)['sessionId']=='fixture-beta-session-2'
    command('/session prev')
    command('beta-first-again')
    assert wait_request('beta','session/prompt',3)['sessionId']=='fixture-beta-session'
    command('/session prev')
    command('initial-still-alive')
    assert wait_request('initial','session/prompt',1)['sessionId']=='fixture-initial-session'
    command('/harness fixture-initial')
    c.send('\x1b'); drain()
    assert len([e for e in events() if e.get('role')=='initial' and e.get('method')=='session/new'])==1
    command('/new')
    wait_request('initial','session/new',2)
    command('/new') # Even an empty current tab must produce another session.
    wait_request('initial','session/new',3)
    command('/harness fixture-beta --new')
    wait_request('beta','session/new',3)
    command('beta-third')
    assert wait_request('beta','session/prompt',4)['sessionId']=='fixture-beta-session-3'
    c.send('/quit\r')
    c.expect(pexpect.EOF); c.close()
    assert c.exitstatus == 0, (c.exitstatus, c.signalstatus)
finally:
    if c.isalive(): c.close(force=True)
`
  const run = spawnSync('python3', ['-c',script,process.execPath,path.join(import.meta.dirname,'harness-discovery-scenario.mjs'),scenario.root],
    { encoding:'utf8', timeout:55000, maxBuffer:8*1024*1024 })
  assert.equal(run.status,0,`${run.error ?? ''}\n${run.stdout?.slice(-20000)}\n${run.stderr}`)
  assert.doesNotMatch(run.stdout,/panicked|insertion index/)
  const events = readFileSync(scenario.eventsPath,'utf8').trim().split('\n').map(JSON.parse)
  assert.equal(events.filter(e=>e.method==='initialize').length,2, 'One initialization per Harness, not per tab')
  assert.equal(events.filter(e=>e.method==='session/new').length,6)
})

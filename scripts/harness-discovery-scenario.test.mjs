import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { get } from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { discoverHarnessCandidates } from '../npm/lib/harnesses.js'
import { ACP_REGISTRY_URL } from '../npm/lib/harness-registry.js'

const script = fileURLToPath(new URL('./harness-discovery-scenario.mjs', import.meta.url))
const scenarioApi = () => import('./harness-discovery-scenario.mjs')
const expected = {
  'fixture-local-binary': 'Found locally',
  'fixture-npm-global': 'Found locally',
  'fixture-managed-binary': 'Installed',
  'fixture-npx-download': 'Available via npx',
  'fixture-missing-uvx': 'Needs uvx',
  'fixture-binary-download': 'Available to install',
}

async function fixture(t) {
  const parentDirectory = mkdtempSync(path.join(tmpdir(), 'martty-scenario-test-'))
  t.after(() => rmSync(parentDirectory, { recursive: true, force: true }))
  const api = await scenarioApi()
  return { api, scenario: api.createHarnessDiscoveryScenario({ parentDirectory }) }
}

test('discovery scenario creates six real filesystem states without invoking a runner', async (t) => {
  const { scenario } = await fixture(t)
  const before = readFileSync(scenario.settingsPath, 'utf8')
  const rows = discoverHarnessCandidates(scenario.settingsPath, {
    settingsPath: scenario.settingsPath, registry: scenario.registry, pathValue: scenario.pathValue, defaults: [],
  })
  for (const [id, status] of Object.entries(expected)) {
    assert.equal(rows.find((row) => row.id === id)?.status, status, id)
  }
  for (const agent of scenario.registry.agents) {
    for (const binary of Object.values(agent.distribution.binary ?? {})) {
      assert.equal(new URL(binary.archive).hostname, '127.0.0.1', 'saved fixtures must not name an external download host')
    }
  }
  assert.match(rows.find((row) => row.id === 'fixture-npm-global').resolvedCommand, /global-fixture-acp/)
  const managed = rows.find((row) => row.id === 'fixture-managed-binary')
  assert.ok(managed.command.startsWith(path.join(scenario.marttyHome, 'bin') + path.sep))
  assert.equal(existsSync(scenario.eventsPath), false, 'discovery must not execute fixture or real agents')
  assert.equal(readFileSync(scenario.settingsPath, 'utf8'), before, 'probe must not register or switch Harnesses')
})

test('scenario subprocess environment isolates config and command discovery without changing the caller', async (t) => {
  const { scenario, api } = await fixture(t)
  const caller = { PATH: '/real/user/bin', MARTTY_HOME: '/real/settings', DSH_HOME: '/real/dsh', NODE_OPTIONS: '--require user-preload.cjs', TERM: 'xterm' }
  const before = structuredClone(caller)
  const environment = api.scenarioEnvironment(scenario, caller)
  assert.deepEqual(caller, before)
  assert.equal(environment.MARTTY_HOME, scenario.marttyHome)
  assert.equal(environment.PATH, scenario.pathValue)
  assert.equal(environment.NODE_OPTIONS, '')
  assert.equal(environment.DSH_TUI_AGENT, '')
  assert.equal(environment.TERM, 'xterm')
  assert.ok(environment.DSH_HOME.startsWith(scenario.root + path.sep))
})

test('scenario Registry is local and every real download URL is refused', async (t) => {
  const { scenario, api } = await fixture(t)
  const originalFetch = globalThis.fetch
  const fetch = api.createScenarioFetch(scenario)
  assert.deepEqual(await (await fetch(ACP_REGISTRY_URL)).json(), scenario.registry)
  assert.equal(globalThis.fetch, originalFetch, 'importing or probing must not replace caller fetch')
  const binary = scenario.registry.agents.find((row) => row.id === 'fixture-binary-download')
  const url = Object.values(binary.distribution.binary)[0].archive
  await assert.rejects(fetch(url), /fixture.*download.*disabled/i)
  await assert.rejects(fetch('https://example.com/real-network'), /blocked.*network/i)
})

test('fixture binary URLs refuse downloads through a real HTTP client that bypasses fetch', async (t) => {
  const { scenario, api } = await fixture(t)
  const persisted = readFileSync(path.join(scenario.root, 'scenario.json'), 'utf8')
  // Existing --root fixtures also get rewritten, without trusting their old URL.
  const oldBinary = scenario.registry.agents.find((agent) => agent.id === 'fixture-binary-download')
  Object.values(oldBinary.distribution.binary)[0].archive = 'https://martty-fixture.invalid/binary-download.tar.gz'
  const original = structuredClone(scenario)
  const guard = await api.startScenarioDownloadGuard(scenario)
  t.after(() => guard.close())
  const registry = await (await api.createScenarioFetch(guard.scenario)(ACP_REGISTRY_URL)).json()
  const archives = registry.agents.flatMap((agent) => Object.values(agent.distribution.binary ?? {}).map((binary) => binary.archive))
  assert.equal(archives.length, 3)
  for (const archive of archives) {
    const url = new URL(archive)
    assert.equal(url.hostname, '127.0.0.1', 'never send a fixture SDK download to an external host')
    assert.equal(url.protocol, 'http:')
    const response = await new Promise((resolve, reject) => {
      const request = get(url, (response) => {
        let body = ''
        response.setEncoding('utf8').on('data', (chunk) => { body += chunk })
        response.on('end', () => resolve({ status: response.statusCode, location: response.headers.location, body }))
      })
      request.setTimeout(2000, () => request.destroy(new Error('Fixture server did not reply')))
      request.on('error', reject)
    })
    assert.equal(response.status, 403)
    assert.equal(response.location, undefined, 'the guard never redirects a downloader elsewhere')
    assert.match(response.body, /offline fixture.*binary download is disabled/i)
  }
  assert.deepEqual(scenario, original, 'ephemeral port is not persisted into scenario config')
  assert.equal(readFileSync(path.join(scenario.root, 'scenario.json'), 'utf8'), persisted)
  assert.equal(api.inspectHarnessDiscoveryScenario(guard.scenario).ok, true)
  assert.equal(existsSync(scenario.eventsPath), false, 'no binary or package runner was executed')
  const { downloadFile } = await import('../npm/lib/download.js')
  const destination = path.join(scenario.root, 'must-not-download.tar.gz')
  await assert.rejects(downloadFile(archives[0], destination, { connectTimeoutMs: 2000 }), /403|offline fixture/i)
  assert.equal(existsSync(destination), false, 'the SDK leaves no downloaded executable or archive')
  await guard.close()
  await assert.rejects(new Promise((resolve, reject) => {
    get(archives[0], (response) => { response.resume(); resolve(response.statusCode) }).on('error', reject)
  }), { code: 'ECONNREFUSED' }, 'closing the fixture leaves no HTTP service listening')
})

test('--check verifies an existing isolated fixture and leaves the user settings untouched', async (t) => {
  const { scenario } = await fixture(t)
  const userSettings = path.join(path.dirname(scenario.root), 'settings.json')
  const original = '{"defaultHarness":"do-not-touch"}\n'
  writeFileSync(userSettings, original)
  const result = spawnSync(process.execPath, [script, '--check', '--root', scenario.root], {
    encoding: 'utf8', timeout: 10_000,
    env: { ...process.env, MARTTY_HOME: path.dirname(userSettings), DSH_TUI_AGENT: 'not-a-fixture-agent' },
  })
  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /6\/6/)
  assert.ok(result.stdout.includes(scenario.root))
  assert.match(result.stdout, /--tui/)
  for (const id of Object.keys(expected)) assert.ok(result.stdout.includes(id), id)
  assert.equal(readFileSync(userSettings, 'utf8'), original)
  assert.equal(existsSync(scenario.eventsPath), false)
})

test('fixture ACP gives deterministic local protocol responses and never calls inference', async (t) => {
  const { scenario, api } = await fixture(t)
  const requests = [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: {} },
    { jsonrpc: '2.0', id: 2, method: 'session/new', params: {} },
    { jsonrpc: '2.0', id: 3, method: 'session/prompt', params: { sessionId: 'fixture-initial-session', prompt: [{type:'text',text:'hello'}] } },
  ]
  const result = spawnSync(process.execPath, [scenario.agentScript, 'acp', 'initial'], {
    input: requests.map((request) => JSON.stringify(request)).join('\n') + '\n',
    encoding: 'utf8', timeout: 5000, env: api.scenarioEnvironment(scenario),
  })
  assert.equal(result.status, 0, result.stderr)
  const output = result.stdout.trim().split('\n').map((line) => JSON.parse(line))
  assert.equal(output.find((message) => message.id === 1).result.protocolVersion, 1)
  assert.equal(output.find((message) => message.id === 2).result.sessionId, 'fixture-initial-session')
  assert.equal(output.find((message) => message.id === 3).result.stopReason, 'end_turn')
  assert.match(JSON.stringify(output), /No model or network was called/)
})

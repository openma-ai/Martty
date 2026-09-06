#!/usr/bin/env node
/** Offline, disposable Harness discovery playground; never uses a real agent. */
import {
  chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync,
} from 'node:fs'
import { spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { discoverHarnessCandidates } from '../npm/lib/harnesses.js'
import { ACP_REGISTRY_URL, registryPlatformKey } from '../npm/lib/harness-registry.js'

const scriptPath = fileURLToPath(import.meta.url)
const projectRoot = path.dirname(path.dirname(scriptPath))
const KIND = 'martty-harness-discovery-fixture-v1'
const PACKAGE = '@martty-fixtures/not-downloaded'
const EXPECTED = Object.freeze({
  'fixture-local-binary': 'Found locally',
  'fixture-npm-global': 'Found locally',
  'fixture-managed-binary': 'Installed',
  'fixture-npx-download': 'Available via npx',
  'fixture-missing-uvx': 'Needs uvx',
  'fixture-binary-download': 'Available to install',
})

// This is the only program behind every executable in the fixture. Even a
// typed prompt produces a canned ACP response; no SDK or network is imported.
const AGENT_SOURCE = String.raw`
const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const record = value => {
  if (process.env.MARTTY_SCENARIO_EVENTS) fs.appendFileSync(process.env.MARTTY_SCENARIO_EVENTS, JSON.stringify(value)+'\n');
};
exports.run = async (mode, args) => {
  if (mode === 'npx') {
    record({kind:'fixture-runner',args});
    if (args[0] === '--package' && args[1] === '@martty-fixtures/not-downloaded') {
      for (const detail of ['Preparing offline fixture', 'Simulating package download', 'Fixture package ready']) {
        process.stderr.write(detail+'\n');
        await sleep(600);
      }
      const cache = path.join(process.env.MARTTY_HOME, 'fixture-package-cache');
      fs.mkdirSync(cache, {recursive:true});
      fs.writeFileSync(path.join(cache, 'ready'), 'No package was downloaded.\n');
      return;
    }
    if (args[0] !== '@martty-fixtures/not-downloaded') throw new Error('Fixture npx refuses every non-fixture package');
    args = [args[1] || 'npx-download'];
  }
  const role = args[0] || 'local-binary';
  if (role === 'failure') process.stderr.write('fixture diagnostic: missing dependency\n');
  const sessionId = 'fixture-'+role+'-session';
  const model = 'fixture-'+role+'-model';
  record({kind:'fixture-acp',role});
  const lines = readline.createInterface({input:process.stdin});
  lines.on('line', line => {
    let request;
    try { request = JSON.parse(line); } catch { return; }
    record({kind:'request',role,method:request.method});
    if (request.id === undefined) return;
    const send = value => process.stdout.write(JSON.stringify(value)+'\n');
    const reply = result => send({jsonrpc:'2.0',id:request.id,result});
    if (request.method === 'initialize') reply({protocolVersion:1,agentInfo:{name:'Offline Fixture ACP',version:'1'},
      agentCapabilities:{loadSession:false,promptCapabilities:{},_meta:{dsh:{cordis:{protocol:0}}}},authMethods:[]});
    else if (request.method === 'session/new' && role === 'failure') send({jsonrpc:'2.0',id:request.id,
      error:{code:-32603,message:'Fixture executable is missing',data:{code:'ENOENT'}}});
    else if (request.method === 'session/new') reply({sessionId,
      configOptions:[{id:'model',name:'Model',type:'select',category:'model',currentValue:model,options:[{value:model,name:model}]}]});
    else if (request.method === 'session/prompt') {
      send({jsonrpc:'2.0',method:'session/update',params:{sessionId:request.params?.sessionId || sessionId,
        update:{sessionUpdate:'agent_message_chunk',content:{type:'text',text:'Offline fixture only. No model or network was called.'}}}});
      reply({stopReason:'end_turn'});
    } else if (request.method === 'session/list') reply({sessions:[]});
    else reply({});
  });
};
if (require.main === module) exports.run(process.argv[2], process.argv.slice(3)).catch(error => {
  process.stderr.write(error.message+'\n'); process.exitCode = 1;
});
`

function jsonFile(file, value) {
  mkdirSync(path.dirname(file), { recursive: true })
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`)
}

function launcher(file, agentScript, runtime, mode = 'acp') {
  mkdirSync(path.dirname(file), { recursive: true })
  if (process.platform === 'win32') {
    // Conventional relative .cmd shim. The caller uses cross-spawn for argv.
    const relativeNode = path.relative(path.dirname(file), runtime)
    const relativeScript = path.relative(path.dirname(file), agentScript)
    writeFileSync(file, `@ECHO OFF\r\n"%~dp0${relativeNode}" "%~dp0${relativeScript}" ${mode} %*\r\n`)
  } else {
    writeFileSync(file, `#!/usr/bin/env node\nrequire(${JSON.stringify(agentScript)}).run(${JSON.stringify(mode)}, process.argv.slice(2)).catch(error => { process.stderr.write(error.message+'\\n'); process.exitCode = 1; });\n`)
    chmodSync(file, 0o755)
  }
}

/** Create only under a new mkdtemp directory; caller environment is untouched. */
export function createHarnessDiscoveryScenario({ parentDirectory = tmpdir() } = {}) {
  const target = registryPlatformKey()
  if (!target) throw new Error(`No fixture binary target for ${process.platform}/${process.arch}`)
  const root = mkdtempSync(path.join(parentDirectory, 'martty-harness-scenario-'))
  const marttyHome = path.join(root, '.martty')
  const settingsPath = path.join(marttyHome, 'settings.json')
  const bin = path.join(root, 'bin')
  const npmBin = path.join(root, 'npm-global', 'bin')
  const runtime = path.join(bin, process.platform === 'win32' ? 'node.exe' : 'node')
  const agentScript = path.join(root, 'fixture-acp.cjs')
  const suffix = process.platform === 'win32' ? '.cmd' : ''
  mkdirSync(bin, { recursive: true })
  mkdirSync(npmBin, { recursive: true })
  writeFileSync(agentScript, AGENT_SOURCE)
  if (process.platform === 'win32') copyFileSync(process.execPath, runtime)
  else symlinkSync(process.execPath, runtime)
  const localName = `fixture-local-acp${suffix}`
  const managedName = `fixture-managed-acp${suffix}`
  launcher(path.join(bin, localName), agentScript, runtime)
  launcher(path.join(bin, `npx${suffix}`), agentScript, runtime, 'npx')

  const packageRoot = path.join(root, 'npm-global', 'lib', 'node_modules', '@martty-fixtures', 'global-acp')
  const packageEntry = path.join(packageRoot, 'bin', `server${suffix || '.js'}`)
  launcher(packageEntry, agentScript, runtime)
  jsonFile(path.join(packageRoot, 'package.json'), {
    name: '@martty-fixtures/global-acp', version: '1.0.0', bin: { 'global-fixture-acp': `bin/${path.basename(packageEntry)}` },
  })
  try { symlinkSync(packageEntry, path.join(npmBin, `global-fixture-acp${suffix}`), 'file') }
  catch (error) {
    throw new Error(`Cannot create npm bin symlink for this fixture at ${root}. Windows may require Developer Mode. No user settings were changed. ${error.message}`)
  }
  launcher(path.join(marttyHome, 'bin', 'fixture-managed-binary', '1.0.0', target, managedName), agentScript, runtime)
  const binary = (cmd, role) => ({ [target]: {
    // Replaced by the fixture-client's bound loopback service before discovery.
    // Persisted fixtures must never contain an external download destination.
    cmd: `./${cmd}`, args: [role], archive: `http://127.0.0.1:0/${role}.tar.gz`,
  } })
  const registry = { version: '1.0.0', agents: [
    { id: 'fixture-local-binary', name: '01 Local binary · on isolated PATH', version: '1.0.0',
      distribution: { binary: binary(localName, 'local-binary') } },
    { id: 'fixture-npm-global', name: '02 npm global · package metadata + bin', version: '1.0.0',
      distribution: { npx: { package: '@martty-fixtures/global-acp', args: ['npm-global'] } } },
    { id: 'fixture-managed-binary', name: '03 Managed binary · already in .martty/bin', version: '1.0.0',
      distribution: { binary: binary(managedName, 'managed-binary') } },
    { id: 'fixture-npx-download', name: '04 npx package · simulated download', version: '1.0.0',
      distribution: { npx: { package: PACKAGE, args: ['npx-download'] } } },
    { id: 'fixture-missing-uvx', name: '05 uvx missing · installation guidance', version: '1.0.0',
      distribution: { uvx: { package: 'martty-fixture-missing-tool' } } },
    { id: 'fixture-binary-download', name: '06 Binary not installed · download disabled', version: '1.0.0',
      distribution: { binary: binary(`fixture-uninstalled-acp${suffix}`, 'binary-download') } },
  ] }
  jsonFile(settingsPath, { defaultHarness: 'fixture-initial', harnesses: [{
    id: 'fixture-initial', label: 'Offline Fixture ACP · initial session',
    command: process.execPath, args: [agentScript, 'acp', 'initial'],
  }] })
  const scenario = {
    kind: KIND, root, marttyHome, settingsPath, registry,
    pathValue: [bin, npmBin].join(path.delimiter), agentScript,
    eventsPath: path.join(root, 'fixture-events.jsonl'), expected: EXPECTED,
  }
  jsonFile(path.join(root, 'scenario.json'), scenario)
  return scenario
}

function loadScenario(root) {
  const resolved = path.resolve(root)
  const scenario = JSON.parse(readFileSync(path.join(resolved, 'scenario.json'), 'utf8'))
  if (scenario.kind !== KIND || scenario.root !== resolved) throw new Error('--root must name a generated Harness fixture directory')
  for (const field of ['marttyHome', 'settingsPath', 'agentScript', 'eventsPath']) {
    if (!scenario[field]?.startsWith(`${resolved}${path.sep}`)) throw new Error(`Fixture ${field} escapes --root`)
  }
  return scenario
}

export function inspectHarnessDiscoveryScenario(scenario) {
  const entries = discoverHarnessCandidates(scenario.settingsPath, {
    settingsPath: scenario.settingsPath, registry: scenario.registry, pathValue: scenario.pathValue, defaults: [],
  })
  const checks = Object.entries(EXPECTED).map(([id, expected]) => {
    const entry = entries.find((entry) => entry.id === id)
    return { id, expected, actual: entry?.status ?? 'Missing', ok: entry?.status === expected }
  })
  return { entries, checks, ok: checks.every(({ ok }) => ok) }
}

export function scenarioEnvironment(scenario, env = process.env) {
  return {
    ...env, MARTTY_HOME: scenario.marttyHome, PATH: scenario.pathValue,
    DSH_HOME: path.join(scenario.root, 'dsh-home'), XDG_CONFIG_HOME: path.join(scenario.root, 'config'),
    DSH_TUI_AGENT: '', NODE_OPTIONS: '', MARTTY_SCENARIO_EVENTS: scenario.eventsPath,
  }
}

export function createScenarioFetch(scenario) {
  const disabledArchives = new Set(scenario.registry.agents.flatMap((agent) =>
    Object.values(agent.distribution.binary ?? {}).map((binary) => binary.archive)))
  return async (url) => {
    const address = String(url)
    if (address === ACP_REGISTRY_URL) return new Response(JSON.stringify(scenario.registry))
    if (disabledArchives.has(address)) {
      throw new Error('Offline fixture: binary download is disabled. No network request was made; use the simulated npx entry to test progress.')
    }
    throw new Error(`Blocked network request in offline fixture: ${address}`)
  }
}

/** SDK downloaders bypass fetch, so every archive must target this local guard. */
export async function startScenarioDownloadGuard(scenario) {
  const server = createServer((_request, response) => {
    response.writeHead(403, 'Offline fixture download disabled', {
      'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store', Connection: 'close',
    })
    response.end('Offline fixture: binary download is disabled. No external network request was made; use the simulated npx entry to test progress.\n')
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => { server.off('error', reject); resolve() })
  })
  // This service must not keep the fixture client alive after the TUI exits.
  server.unref()
  const local = structuredClone(scenario)
  const origin = `http://127.0.0.1:${server.address().port}`
  for (const agent of local.registry.agents) {
    for (const [target, binary] of Object.entries(agent.distribution.binary ?? {})) {
      binary.archive = `${origin}/${encodeURIComponent(agent.id)}/${encodeURIComponent(target)}.tar.gz`
    }
  }
  let closing
  return {
    scenario: local,
    close: () => closing ??= new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve())
      server.closeAllConnections()
    }),
  }
}

function printScenario(scenario, result) {
  console.log(`Offline Harness discovery scenario\nRoot: ${scenario.root}\nSettings: ${scenario.settingsPath}`)
  console.log('All commands are local fixtures. No real agent, model, external network, or user settings are used.\n')
  for (const check of result.checks) console.log(`${check.ok ? 'PASS' : 'FAIL'}  ${check.id.padEnd(26)} ${check.actual}`)
  console.log(`\n${result.checks.filter(({ ok }) => ok).length}/6 expected states matched.`)
  console.log('In the TUI, run /harness add to browse these six scenarios. Entries 01–03 connect locally; 04 simulates a slow package preparation; 05 shows missing-uvx guidance; 06 tests a local HTTP 403 download failure.')
  console.log(`\nTry this directory:\nnode scripts/harness-discovery-scenario.mjs --tui --root ${JSON.stringify(scenario.root)}`)
  console.log('The temporary directory is kept for inspection. Re-run without --root to create a fresh scenario.')
}

function fixtureNativeBinary() {
  const name = process.platform === 'win32' ? 'martty.exe' : 'martty'
  const candidates = [path.join(projectRoot, 'target', 'debug', name), path.join(projectRoot, 'npm', 'vendor', `${process.platform}-${process.arch}`, name)]
  const binary = candidates.find((candidate) => existsSync(candidate))
  if (!binary) throw new Error('No local Martty binary found. Build the project first; this fixture never installs dependencies or builds automatically.')
  return binary
}

async function main(argv) {
  if (argv[0] === '--fixture-client' && argv.length === 2) {
    const guard = await startScenarioDownloadGuard(loadScenario(argv[1]))
    const { scenario } = guard
    try {
      globalThis.fetch = createScenarioFetch(scenario)
      const { bootClient } = await import('../npm/lib/boot.js')
      await bootClient({
        agent: { command: process.execPath, args: [scenario.agentScript, 'acp', 'initial'] },
        settingsPath: scenario.settingsPath, harnessDefaults: [], harnessPathValue: scenario.pathValue,
        artifactRoot: path.join(scenario.root, 'artifacts'), packagePlugins: [], extraArgs: ['--workspace', scenario.root],
      })
    } catch (error) {
      await guard.close()
      throw error
    }
    return
  }
  if (argv.includes('--help') || argv.includes('-h')) {
    console.log('Usage: node scripts/harness-discovery-scenario.mjs [--check | --tui] [--root <existing-fixture>]\nDefault: --check. Creates a disposable offline fixture unless --root is supplied. --tui uses the existing local Martty binary and fake ACP only.')
    return
  }
  let root, mode = 'check', modeSet = false
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--root' && argv[i + 1]) root = argv[++i]
    else if ((argv[i] === '--check' || argv[i] === '--tui') && !modeSet) { mode = argv[i].slice(2); modeSet = true }
    else throw new Error(`Unknown or conflicting option: ${argv[i]}; use --help`)
  }
  const scenario = root ? loadScenario(root) : createHarnessDiscoveryScenario()
  const result = inspectHarnessDiscoveryScenario(scenario)
  printScenario(scenario, result)
  if (mode === 'check') { if (!result.ok) process.exitCode = 1; return }
  if (!process.stdin.isTTY || !process.stdout.isTTY) throw new Error('--tui requires an interactive terminal; use --check for automated probing')
  const child = spawnSync(process.execPath, [scriptPath, '--fixture-client', scenario.root], {
    stdio: 'inherit', cwd: scenario.root,
    env: { ...scenarioEnvironment(scenario), MARTTY_BIN: fixtureNativeBinary() },
  })
  if (child.error) throw child.error
  process.exitCode = child.status ?? 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  main(process.argv.slice(2)).catch((error) => { console.error(error.message); process.exitCode = 1 })
}

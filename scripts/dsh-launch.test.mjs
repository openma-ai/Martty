import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { inspectDshProfile, planDshLaunch, resolveGlobalDsh, takeDshFlag } from '../npm/lib/dsh-launch.js'
import { TESTED_DSH_THROUGH } from '../npm/lib/dsh-compat.js'

function home(t, { bundles, martty, acp }) {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-dsh-launch-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const dir = path.join(root, 'profiles', 'p')
  mkdirSync(dir, { recursive: true })
  writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ dsh: { profile: { bundles } } }))
  for (const [name, manifest] of [['martty', martty], ['@openma/deepseek-harness-acp', acp]]) {
    if (manifest === undefined) continue
    const pkg = path.join(dir, 'node_modules', name)
    mkdirSync(pkg, { recursive: true })
    writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ name, ...manifest }))
  }
  return { DSH_HOME: root }
}

const dshAt = (version) => () => ({ status: 0, stdout: `dsh ${version}\n`, stderr: '' })

test('takeDshFlag splits both spellings and validates the profile name', () => {
  assert.deepEqual(takeDshFlag(['--dsh', 'martty', '--x']), { profile: 'martty', rest: ['--x'] })
  assert.deepEqual(takeDshFlag(['--dsh=martty']), { profile: 'martty', rest: [] })
  assert.deepEqual(takeDshFlag(['--x']), { profile: null, rest: ['--x'] })
  assert.throws(() => takeDshFlag(['--dsh']), /needs a dsh profile name/)
  assert.throws(() => takeDshFlag(['--dsh', '../x']), /not a dsh profile name/)
  assert.throws(() => takeDshFlag(['--dsh', 'a', '--agent', 'x']), /cannot be combined/)
})

test('a missing global dsh names the install command', () => {
  const spawnSync = () => ({ error: Object.assign(new Error('nope'), { code: 'ENOENT' }) })
  assert.throws(() => resolveGlobalDsh({ spawnSync }), /npm install -g @deepseek-ai\/dsh@latest/)
})

test('a Martty profile on a supported dsh hands the terminal to dsh --profile', (t) => {
  const env = home(t, {
    bundles: ['martty'],
    martty: { version: '9.0.0', engines: { dsh: '>=0.1.7-rc.2' } },
    acp: { version: '0.4.39', engines: { dsh: '>=0.1.7-rc.2' } },
  })
  const plan = planDshLaunch('p', ['--foo'], { env, spawnSync: dshAt('0.2.0-rc.2') })
  assert.deepEqual(plan, { mode: 'profile', command: 'dsh', args: ['--profile', 'p', '--foo'] })
})

test('a dsh newer than tested is attempted with a warning, never rejected', (t) => {
  const env = home(t, {
    bundles: ['martty'],
    martty: { version: '9.0.0', engines: { dsh: '>=0.1.7-rc.2' } },
    acp: { version: '0.4.39', peerDependencies: { '@deepseek-ai/dsh': '>=0.1.7-rc.2' } },
  })
  const plan = planDshLaunch('p', [], { env, spawnSync: dshAt('9.9.9') })
  assert.equal(plan.mode, 'profile')
  assert.match(plan.warning, new RegExp(`newer than .*${TESTED_DSH_THROUGH.replace(/\./g, '\\.')}.*continuing`))
})

test('an ACP-only profile becomes the ACP agent; untested versions add an info notice', (t) => {
  const env = home(t, { bundles: ['@openma/deepseek-harness-acp'], acp: { version: '0.4.39', engines: { dsh: '>=0.1.7-rc.2' } } })
  assert.deepEqual(planDshLaunch('p', [], { env, spawnSync: dshAt('0.2.1-rc.2') }), {
    mode: 'agent', agent: { command: 'dsh', args: ['--profile', 'p'] },
  })
  const newer = planDshLaunch('p', [], { env, spawnSync: dshAt('0.3.0') })
  assert.equal(newer.notice.level, 'info')
})

test('below a floor the launcher falls back to the bundled harness with a notice naming why', (t) => {
  const env = home(t, {
    bundles: ['martty'],
    martty: { version: '9.0.0', engines: { dsh: '>=0.1.7-rc.2' } },
    // dsh-acp <= 0.4.38 shipped an exact list; it refuses anything else at mount.
    acp: { version: '0.4.38', peerDependencies: { '@deepseek-ai/dsh': '0.1.7-rc.2 || 0.2.0-rc.1 || 0.2.0-rc.2' } },
  })
  const plan = planDshLaunch('p', [], { env, spawnSync: dshAt('0.2.1-rc.2') })
  assert.equal(plan.mode, 'bundled')
  assert.equal(plan.notice.level, 'warn')
  assert.match(plan.notice.text, /global dsh 0\.2\.1-rc\.2 is not supported/)
  assert.match(plan.notice.text, /@openma\/deepseek-harness-acp 0\.4\.38 needs dsh 0\.1\.7-rc\.2 \|\| /)
  assert.match(plan.notice.text, /bundled harness/)
  const old = planDshLaunch('p', [], { env, spawnSync: dshAt('0.1.0') })
  assert.equal(old.mode, 'bundled')
  assert.match(old.notice.text, /martty \(launcher\) .* needs dsh >=0\.1\.7-rc\.2/)
})

test('Martty <= 0.3.2 profiles are judged by their dsh.supported list', (t) => {
  const env = home(t, { bundles: ['martty'], martty: { version: '0.3.2', dsh: { supported: '0.2.0-rc.2' } } })
  const { requirements } = inspectDshProfile('p', '0.2.1-rc.2', { env })
  assert.deepEqual(requirements.find(({ name }) => name === 'martty'), {
    name: 'martty', version: '0.3.2', range: '0.2.0-rc.2', supported: false,
  })
})

test('missing or Martty-less profiles fail with the add command', (t) => {
  const env = home(t, { bundles: ['other'] })
  assert.throws(() => planDshLaunch('p', [], { env, spawnSync: dshAt('0.2.0-rc.2') }), /dsh plugin --profile p add martty@latest/)
  assert.throws(() => planDshLaunch('nope', [], { env, spawnSync: dshAt('0.2.0-rc.2') }), /does not exist/)
})

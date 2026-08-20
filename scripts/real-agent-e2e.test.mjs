import assert from 'node:assert/strict'
import { chmod, mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const script = path.join(import.meta.dirname, 'real-agent-e2e.py')

async function fixture(events, expected = {}) {
  const root = await mkdtemp(path.join(tmpdir(), 'dsh-tui-real-agent-score-'))
  const log = path.join(root, 'session.jsonl')
  const spec = path.join(root, 'spec.json')
  await writeFile(log, `${events.map((event) => JSON.stringify(event)).join('\n')}\n`)
  await writeFile(spec, `${JSON.stringify({
    prompt: 'create the requested TUI plugin',
    expected: {
      skills: ['cordis-plugin-development', 'tui-plugin-development'],
      inspectProviders: ['Commands', 'Overlay', 'ConfigOptions'],
      forbiddenInspectProviders: ['Slots'],
      toolCounts: {
        cordis_inspect_list: 1,
        cordis_define: 1,
        cordis_run: 1,
      },
      maxToolCalls: 8,
      directDefineRunAfterInspect: true,
      clientCodeMustMatch: [
        String.raw`transaction\s*\(\s*\{\s*category\s*:\s*['"]thought_level['"]`,
      ],
      clientCodeMustNotMatch: [String.raw`chrome\.right`, String.raw`\bwhen\s*:`],
      ...expected,
    },
  })}\n`)
  return { log, spec }
}

function tool(name, args, step) {
  return {
    type: 'tool/call',
    seq: step * 2,
    data: { turn: 1, step, name, arguments: JSON.stringify(args) },
  }
}

const clientCode = `return {
  inject: ['tuiCommands', 'tuiOverlay', 'acpSessionConfig'],
  apply(ctx) {
    ctx.tuiCommands.register({ name: 'focus-level', description: 'focus' }, () => {
      const tx = ctx.acpSessionConfig.transaction({ category: 'thought_level' })
      ctx.tuiOverlay.openSlider({ id: 'focus-level', min: 0, max: 8, step: 1, value: 4 })
      return tx
    })
  },
}`

const runtimeDerivedCategoryCode = `const CATEGORY = 'thought_level'
return {
  inject: ['tuiCommands', 'tuiOverlay', 'acpSessionConfig'],
  apply(ctx) {
    ctx.tuiCommands.register({ name: 'focus-level', description: 'focus' }, () => {
      const option = ctx.acpSessionConfig.byCategory(CATEGORY)[0]
      const tx = ctx.acpSessionConfig.transaction({ category: option.category })
      ctx.tuiOverlay.openSlider(
        { id: 'focus-level', min: 0, max: 8, step: 1, value: 4 },
        {
          onChange: (value) => tx.preview(value),
          onSubmit: (value) => tx.commit(value),
          onCancel: () => tx.rollback(),
        },
      )
    })
  },
}`

test('real-agent trajectory scorer accepts the minimal inspected Creator flow', async () => {
  const { log, spec } = await fixture([
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Commands', method: 'list' }, 3),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Overlay', method: 'active' }, 4),
    tool('cordis_inspect_query', { platform: 'client', provider: 'ConfigOptions', method: 'list' }, 5),
    tool('cordis_define', { code: { client: clientCode } }, 6),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 7),
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ])

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 0, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.equal(report.passed, true)
  assert.equal(report.metrics.toolCalls, 8)
  assert.deepEqual(report.skills, ['cordis-plugin-development', 'tui-plugin-development'])
  assert.deepEqual(report.inspectProviders, ['Commands', 'Overlay', 'ConfigOptions'])
  assert.deepEqual(report.failures, [])
})

test('config-slider spec accepts a transaction selected from the live semantic category', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'dsh-tui-real-agent-production-spec-'))
  const log = path.join(root, 'session.jsonl')
  const spec = path.join(import.meta.dirname, 'real-agent-e2e.config-slider.json')
  const events = [
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Commands', method: 'list' }, 3),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Overlay', method: 'active' }, 4),
    tool('cordis_inspect_query', { platform: 'client', provider: 'ConfigOptions', method: 'list' }, 5),
    tool('cordis_define', { code: { client: runtimeDerivedCategoryCode } }, 6),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 7),
    {
      type: 'user/message',
      seq: 19,
      data: {
        content: [{
          type: 'text',
          text: 'Cordis run focus-1/pkg-1 (run-1) completed successfully. currentPackageId is pkg-1.',
        }],
        source: { kind: 'plugin', plugin: 'cordis-host-runner' },
      },
    },
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ]
  await writeFile(log, `${events.map((event) => JSON.stringify(event)).join('\n')}\n`)

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 0, result.stderr || result.stdout)
})

test('real-agent trajectory scorer rejects a completed turn whose Package activation failed', async () => {
  const { log, spec } = await fixture([
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Commands', method: 'list' }, 3),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Overlay', method: 'active' }, 4),
    tool('cordis_inspect_query', { platform: 'client', provider: 'ConfigOptions', method: 'list' }, 5),
    tool('cordis_define', { code: { client: clientCode } }, 6),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 7),
    {
      type: 'user/message',
      seq: 19,
      data: {
        content: [{
          type: 'text',
          text: 'Cordis run focus-1/pkg-1 (run-1) failed: client-half-failed',
        }],
        source: { kind: 'plugin', plugin: 'cordis-host-runner' },
      },
    },
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ], { successfulRun: true })

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 1, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.match(report.failures.join('\n'), /activate successfully/i)
})

test('real-agent trajectory scorer rejects the wrong method on a required Provider', async () => {
  const expectedQueries = [
    { platform: 'client', provider: 'Commands', method: 'list' },
    { platform: 'client', provider: 'Overlay', method: 'active' },
    { platform: 'client', provider: 'ConfigOptions', method: 'list' },
  ]
  const { log, spec } = await fixture([
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_query', expectedQueries[0], 3),
    tool('cordis_inspect_query', expectedQueries[1], 4),
    tool('cordis_inspect_query', {
      platform: 'client', provider: 'ConfigOptions', method: 'guess-values',
    }, 5),
    tool('cordis_define', { code: { client: clientCode } }, 6),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 7),
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ], { inspectQueries: expectedQueries })

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 1, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.match(report.failures.join('\n'), /inspect queries exactly match/i)
})

test('real-agent trajectory scorer rejects generic skill, provider drift, probes, and bad Client code', async () => {
  const { log, spec } = await fixture([
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_list', {}, 3),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Slots', method: 'list' }, 4),
    tool('cordis_define', {
      code: {
        client: `return { apply(ctx) { ctx.tuiSlots.register('chrome.right', []); ctx.tuiCommands.register({ name: 'x', description: 'x', when: { theme: 'x' } }, () => {}) } }`,
      },
    }, 5),
    tool('cordis_run', { pluginId: 'probe-1', packageId: 'pkg-1', mode: 'run' }, 6),
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ])

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 1, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.equal(report.passed, false)
  assert.match(report.failures.join('\n'), /skills exactly match/i)
  assert.match(report.failures.join('\n'), /inspect providers exactly match/i)
  assert.match(report.failures.join('\n'), /cordis_inspect_list count/i)
  assert.match(report.failures.join('\n'), /client code must match/i)
  assert.match(report.failures.join('\n'), /client code must not match/i)
})

test('real-agent trajectory scorer caps retries of an optional capability', async () => {
  const { log, spec } = await fixture([
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('read_image', { file_path: '/tmp/frame-00.png' }, 2),
    tool('read_image', { file_path: '/tmp/frame-30.png' }, 3),
    tool('cordis_inspect_list', {}, 4),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Commands', method: 'list' }, 5),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Overlay', method: 'active' }, 6),
    tool('cordis_inspect_query', { platform: 'client', provider: 'ConfigOptions', method: 'list' }, 7),
    tool('cordis_define', { code: { client: clientCode } }, 8),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 9),
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ], {
    maxToolCalls: 10,
    maxToolCounts: { read_image: 1 },
  })

  const result = spawnSync('python3', [script, 'analyze', '--log', log, '--spec', spec], {
    encoding: 'utf8',
  })
  assert.equal(result.status, 1, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.match(report.failures.join('\n'), /read_image count <= 1/i)
})

test('real-agent PTY runner waits for the native composer then scores its durable turn', async () => {
  const minimal = [
    tool('skill', { name: 'cordis-plugin-development' }, 1),
    tool('skill', { name: 'tui-plugin-development' }, 1),
    tool('cordis_inspect_list', {}, 2),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Commands', method: 'list' }, 3),
    tool('cordis_inspect_query', { platform: 'client', provider: 'Overlay', method: 'active' }, 4),
    tool('cordis_inspect_query', { platform: 'client', provider: 'ConfigOptions', method: 'list' }, 5),
    tool('cordis_define', { code: { client: clientCode } }, 6),
    tool('cordis_run', { pluginId: 'focus-1', packageId: 'pkg-1', mode: 'run' }, 7),
    { type: 'turn/end', seq: 20, data: { turn: 1, reason: { kind: 'completed' } } },
  ]
  const { spec } = await fixture(minimal)
  const runRoot = await mkdtemp(path.join(tmpdir(), 'dsh-tui-real-agent-run-'))
  const fakeHome = path.join(runRoot, 'dsh-home')
  const fakeDsh = path.join(runRoot, 'fake-dsh')
  const durable = minimal.map((event) => JSON.stringify(event)).join('\n')
  await writeFile(fakeDsh, `#!/bin/sh
printf 'describe what you want to build…'
IFS= read -r preset
mkdir -p "$DSH_HOME/sessions/fake"
printf '{"type":"session","version":0,"id":"fake","cwd":"%s"}\n' "$PWD" > "$DSH_HOME/sessions/fake/session.jsonl"
printf '%s\n' '{"type":"agent-preset/selected","seq":1,"data":{"agentPreset":"cordis"}}' >> "$DSH_HOME/sessions/fake/session.jsonl"
IFS= read -r prompt
printf '%s\n' '{"type":"user/message","seq":2,"data":{"content":[{"type":"text","text":"create the requested TUI plugin"}]}}' >> "$DSH_HOME/sessions/fake/session.jsonl"
cat >> "$DSH_HOME/sessions/fake/session.jsonl" <<'EVENTS'
${durable}
EVENTS
sleep 1
`)
  await chmod(fakeDsh, 0o755)

  const result = spawnSync('python3', [
    script, 'run',
    '--spec', spec,
    '--root', runRoot,
    '--dsh-bin', fakeDsh,
    '--tui-bin', fakeDsh,
    '--ready-timeout', '5',
    '--preset-timeout', '5',
    '--timeout', '5',
  ], {
    encoding: 'utf8',
    env: { ...process.env, DSH_HOME: fakeHome },
  })

  assert.equal(result.status, 0, result.stderr || result.stdout)
  const report = JSON.parse(result.stdout)
  assert.equal(report.passed, true)
  assert.equal(report.run.agentPreset, 'cordis')
  assert.equal(report.run.profile, 'tui-test')
})

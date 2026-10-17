import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import * as creatorOverlay from '../npm/lib/creator-overlay.js'

function harness(options = {}) {
  const skills = new Map()
  const promptSections = []
  const tools = new Map()
  const effects = []
  const standingKey = { preset: 'cordis' }
  const requestedModules = []
  let requestedPreset
  let disposed = false
  let leaseReleased = false
  let scopeKey
  let inspected
  let packageSource = {
    pluginId: 'skin-1',
    packageId: 'dyn-1',
    name: 'Paper Lantern',
    purpose: 'Warm terminal palette',
    code: { client: 'return { apply() {} }' },
    currentPackageId: 'dyn-1',
  }
  const header = {
    id: 'stored-1',
    version: 3,
    createdAt: 1,
    cwd: '/work/one',
    isSeeded: false,
    delegationDepth: 0,
  }
  const persistence = options.persistence === true
    ? {
        async list() {
          return [{ header, revision: 'rev-1', sizeBytes: 3 }]
        },
        async open() {
          return {
            header,
            inheritedEventCount: 0,
            async read() {
              return { events: [] }
            },
            async close() {},
          }
        },
      }
    : undefined

  const overlay = {
    ctx: {
      get(name) {
        if (name === 'skills' && options.noSkills !== true) {
          return {
            register(skill) {
              skills.set(skill.name, skill)
            },
          }
        }
        if (name === 'systemPrompt' && options.noSystemPrompt !== true) {
          return {
            section(section) {
              promptSections.push(section)
            },
          }
        }
        if (name === 'tools') {
          return {
            register(tool) {
              tools.set(tool.name, tool)
              return () => tools.delete(tool.name)
            },
          }
        }
        return undefined
      },
    },
    async dispose() {
      disposed = true
      skills.clear()
      promptSections.length = 0
    },
  }

  const ctx = {
    get(name) {
      return name === 'sessionPersistence' ? persistence : undefined
    },
    agentPresets: {
      async acquireScope(id) {
        requestedPreset = id
        if (options.lease !== undefined) return options.lease
        return {
          key: standingKey,
          async [Symbol.asyncDispose]() {
            leaseReleased = true
          },
        }
      },
    },
    loader: {
      async import(name) {
        requestedModules.push(name)
        if (name === '@deepseek-ai/dsh-tools') {
          if (options.toolsModule !== undefined) return options.toolsModule
          return { defineTool: (definition) => definition }
        }
        if (options.scopeModule !== undefined) return options.scopeModule
        return {
          createScope(parent, key) {
            assert.equal(parent, ctx)
            scopeKey = key
            return overlay
          },
        }
      },
    },
    dynamicCordisRunner: {
      inspectPackage(agent, pluginId, packageId) {
        inspected = { agent, pluginId, packageId }
        return packageSource
      },
    },
    effect(setup, label) {
      effects.push({ cleanup: setup(), label })
    },
  }

  return {
    ctx,
    effects,
    persistence,
    promptSections,
    skills,
    tools,
    setPackageSource(value) {
      packageSource = value
    },
    state: () => ({ disposed, inspected, leaseReleased, requestedModules, requestedPreset, scopeKey }),
  }
}

test('exports one internal Host overlay with the required injected services', () => {
  assert.equal(creatorOverlay.name, 'tui-creator-overlay')
  assert.deepEqual(creatorOverlay.inject, ['skills', 'systemPrompt', 'loader', 'tools'])
  assert.deepEqual(creatorOverlay.acpServices, ['agentPresets', 'dynamicCordisRunner'])
})

test('waits for the ACP plugin services in a nested scope, never as a pending entry', async () => {
  const fixture = harness()
  let deferred
  let applied = 0
  const ctx = {
    inject(deps, callback) {
      deferred = { deps, callback }
      return { then() {} } // a fiber that never settles while the ACP plugin is absent
    },
  }
  assert.equal(creatorOverlay.apply(ctx), undefined)
  assert.deepEqual(deferred.deps, ['agentPresets', 'dynamicCordisRunner'])
  const original = fixture.ctx.effect
  fixture.ctx.effect = (...args) => {
    applied += 1
    return original?.apply(fixture.ctx, args)
  }
  await deferred.callback(fixture.ctx)
  assert.ok(applied > 0)
})

test('applies the standalone Host compatibility shim to the ACP persistence service', async () => {
  const fixture = harness({ persistence: true })
  await creatorOverlay.applyOverlay(fixture.ctx)

  const listed = await fixture.persistence.list()
  assert.deepEqual(listed.map((row) => row.id), ['stored-1'])
  assert.equal(listed[0].cwd, '/work/one')
  assert.equal(typeof fixture.persistence.inspect, 'function')
})

test('adds the TUI companion skill only to the requested preset scope', async () => {
  const fixture = harness()
  await creatorOverlay.applyOverlay(fixture.ctx)

  const skill = fixture.skills.get('tui-plugin-development')
  assert.equal(fixture.state().requestedPreset, 'cordis')
  assert.equal(skill?.source, 'martty/creator-overlay')
  assert.equal(skill?.invocation.modelInvocable, true)
  assert.equal(skill?.invocation.userInvocable, true)
  assert.match(skill?.description ?? '', /companion/i)
  assert.match(skill?.description ?? '', /load `cordis-plugin-development` first/i)
})

test('loads the profile-owned scope module instead of importing a private copy', async () => {
  const fixture = harness()
  await creatorOverlay.applyOverlay(fixture.ctx)
  assert.ok(fixture.state().requestedModules.includes('@deepseek-ai/dsh-scope'))
})

test('routes generic Cordis first and TUI behavior to the companion skill', async () => {
  const fixture = harness()
  await creatorOverlay.applyOverlay(fixture.ctx)

  assert.equal(fixture.promptSections.length, 1)
  const prompt = fixture.promptSections[0]
  assert.equal(prompt.name, 'tool:tui-cordis-routing')
  assert.match(prompt.text, /cordis-plugin-development.*every dynamic Plugin/is)
  assert.match(prompt.text, /TUI.*also load.*tui-plugin-development/is)
  assert.match(prompt.text, /browser|Web/i)
})

test('unloading the TUI overlay retracts the skill and prompt together', async () => {
  const fixture = harness()
  await creatorOverlay.applyOverlay(fixture.ctx)
  assert.equal(fixture.effects.length, 1)
  assert.match(fixture.effects[0].label, /tui-creator-overlay/)

  await fixture.effects[0].cleanup()

  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.state().leaseReleased, true)
  assert.equal(fixture.skills.size, 0)
  assert.equal(fixture.promptSections.length, 0)
})

test('a failed mount disposes the overlay and releases the preset scope lease', async () => {
  const fixture = harness({ noSkills: true })
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx),
    /skills service is unavailable/,
  )
  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.state().leaseReleased, true)
})

test('rejects when the preset registry hands out no usable lease', async () => {
  const fixture = harness({ lease: null })
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx),
    /preset scope is unavailable for "cordis"/,
  )
  assert.equal(fixture.state().disposed, false)
  assert.equal(fixture.state().leaseReleased, false)
})

test('releases the lease when the scope module cannot create the overlay', async () => {
  const fixture = harness({ scopeModule: {} })
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx),
    /dsh-scope module is unavailable/,
  )
  assert.equal(fixture.state().disposed, false)
  assert.equal(fixture.state().leaseReleased, true)
})

test('releases the lease and overlay when the tools module is missing defineTool', async () => {
  const fixture = harness({ toolsModule: {} })
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx),
    /dsh-tools module is unavailable/,
  )
  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.state().leaseReleased, true)
})

test('releases the lease and overlay when the scoped systemPrompt service is missing', async () => {
  const fixture = harness({ noSystemPrompt: true })
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx),
    /systemPrompt service is unavailable/,
  )
  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.state().leaseReleased, true)
})

test('a lease without asyncDispose still mounts and unloads cleanly', async () => {
  const fixture = harness({ lease: { key: { preset: 'cordis' } } })
  await creatorOverlay.applyOverlay(fixture.ctx)
  assert.equal(fixture.skills.has('tui-plugin-development'), true)
  assert.deepEqual(fixture.state().scopeKey, { preset: 'cordis' })

  await fixture.effects[0].cleanup()
  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.state().leaseReleased, false)
})

test('rejects an empty preset id without touching the preset registry', async () => {
  const fixture = harness()
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { preset: '' }),
    /preset must be a non-empty string/,
  )
  assert.equal(fixture.state().requestedPreset, undefined)
})

test('Creator scope exposes explicit durable TUI artifact tools', async () => {
  const fixture = harness()
  const root = mkdtempSync(path.join(tmpdir(), 'martty-creator-artifacts-'))
  try {
    await creatorOverlay.applyOverlay(fixture.ctx, { artifactRoot: root })

    assert.deepEqual([...fixture.tools.keys()].sort(), [
      'tui_plugin_list',
      'tui_plugin_read',
      'tui_plugin_remove',
      'tui_plugin_save',
    ])

    const agent = { id: 'agent-1' }
    const saved = await fixture.tools.get('tui_plugin_save').execute({
      artifactId: 'paper-lantern',
      kind: 'theme',
      pluginId: 'skin-1',
      packageId: 'dyn-1',
    }, { agent })
    assert.deepEqual(fixture.state().inspected, {
      agent,
      pluginId: 'skin-1',
      packageId: 'dyn-1',
    })
    assert.equal(saved.status, 'saved')
    assert.equal(saved.artifact.id, 'paper-lantern')
    assert.equal(saved.artifact.kind, 'theme')
    assert.equal(saved.path, path.join(root, 'paper-lantern', 'plugin.json'))

    const listed = await fixture.tools.get('tui_plugin_list').execute({}, { agent })
    assert.equal(listed.artifacts[0].id, 'paper-lantern')

    const read = await fixture.tools.get('tui_plugin_read').execute({
      artifactId: 'paper-lantern',
    }, { agent })
    assert.equal(read.artifact.id, 'paper-lantern')
    assert.equal(read.artifact.source.packageId, 'dyn-1')
    assert.equal(read.artifact.code.client, 'return { apply() {} }')

    fixture.setPackageSource({
      pluginId: 'ui-1',
      packageId: 'dyn-2',
      name: 'Focused UI',
      purpose: 'A focused layout',
      code: { client: 'return { apply() {} }' },
      currentPackageId: 'dyn-2',
    })
    const savedUi = await fixture.tools.get('tui_plugin_save').execute({
      artifactId: 'focused-ui',
      kind: 'ui',
      pluginId: 'ui-1',
      packageId: 'dyn-2',
    }, { agent })
    assert.equal(savedUi.artifact.kind, 'ui')
    assert.equal(JSON.parse(readFileSync(savedUi.path, 'utf8')).kind, 'ui')

    const removed = await fixture.tools.get('tui_plugin_remove').execute({
      artifactId: 'paper-lantern',
    }, { agent })
    assert.deepEqual(removed, { status: 'removed', artifactId: 'paper-lantern' })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Creator refuses to persist unvalidated or Host-backed dynamic Packages as TUI artifacts', async () => {
  const fixture = harness()
  const root = mkdtempSync(path.join(tmpdir(), 'martty-creator-artifacts-'))
  try {
    await creatorOverlay.applyOverlay(fixture.ctx, { artifactRoot: root })
    const save = fixture.tools.get('tui_plugin_save')
    assert.equal(typeof save?.execute, 'function')
    if (typeof save?.execute !== 'function') return

    fixture.setPackageSource({
      pluginId: 'skin-1',
      packageId: 'dyn-2',
      name: 'Not run',
      purpose: 'Not validated',
      code: { client: 'return {}' },
    })
    await assert.rejects(
      save.execute({
        artifactId: 'not-run',
        kind: 'theme',
        pluginId: 'skin-1',
        packageId: 'dyn-2',
      }, { agent: { id: 'agent-1' } }),
      /successful|current|run/i,
    )

    fixture.setPackageSource({
      pluginId: 'mixed-1',
      packageId: 'dyn-3',
      name: 'Mixed',
      purpose: 'Has Host behavior',
      code: { host: 'return {}', client: 'return {}' },
      currentPackageId: 'dyn-3',
    })
    await assert.rejects(
      save.execute({
        artifactId: 'mixed',
        kind: 'ui',
        pluginId: 'mixed-1',
        packageId: 'dyn-3',
      }, { agent: { id: 'agent-1' } }),
      /Host|client-only/i,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('retries acquireScope while preset declarations are still registering', async () => {
  const fixture = harness()
  const attempts = []
  fixture.ctx.agentPresets.acquireScope = async (id) => {
    attempts.push(id)
    if (attempts.length < 3) {
      throw Object.assign(new Error(`Unknown agent preset: ${id}`), {
        code: 'agent-preset/not-found',
      })
    }
    return { key: { preset: id }, async [Symbol.asyncDispose]() {} }
  }
  await creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 })
  assert.equal(attempts.length, 3)
  assert.equal(fixture.state().scopeKey.preset, 'cordis')
})

test('not-found failures without a remote code are retried on their message', async () => {
  const fixture = harness()
  let calls = 0
  fixture.ctx.agentPresets.acquireScope = async (id) => {
    calls += 1
    if (calls === 1) throw new Error(`Unknown agent preset: ${id}`)
    return { key: { preset: id }, async [Symbol.asyncDispose]() {} }
  }
  await creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 })
  assert.equal(calls, 2)
})

test('a not-found preset is retried only until the wait window ends', async () => {
  const fixture = harness()
  let calls = 0
  fixture.ctx.agentPresets.acquireScope = async () => {
    calls += 1
    throw Object.assign(new Error('Unknown agent preset: ghost'), {
      code: 'agent-preset/not-found',
    })
  }
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { preset: 'ghost', scopeWaitMs: 30, scopePollMs: 5 }),
    /Unknown agent preset: ghost/,
  )
  assert.ok(calls >= 2, `expected retries before the deadline, got ${calls}`)
})

test('a scopeWaitMs of 0 rejects the first not-found immediately', async () => {
  const fixture = harness()
  let calls = 0
  fixture.ctx.agentPresets.acquireScope = async () => {
    calls += 1
    throw Object.assign(new Error('Unknown agent preset: cordis'), {
      code: 'agent-preset/not-found',
    })
  }
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { scopeWaitMs: 0, scopePollMs: 1 }),
    /Unknown agent preset/,
  )
  assert.equal(calls, 1)
})

test('non-not-found acquireScope failures are never retried', async () => {
  const fixture = harness()
  let calls = 0
  fixture.ctx.agentPresets.acquireScope = async () => {
    calls += 1
    throw Object.assign(new Error('preset cordis is broken'), { code: 'agent-preset/invalid' })
  }
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 }),
    /preset cordis is broken/,
  )
  assert.equal(calls, 1)
})

test('non-Error acquireScope failures are never retried', async () => {
  const fixture = harness()
  fixture.ctx.agentPresets.acquireScope = async () => {
    throw 'registry exploded'
  }
  await assert.rejects(creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 }), /registry exploded/)
})

test('a registry without acquireScope reports the scope as unavailable', async () => {
  const fixture = harness()
  delete fixture.ctx.agentPresets.acquireScope
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 }),
    /preset scope is unavailable for "cordis"/,
  )
})

test('a missing preset registry reports the scope as unavailable', async () => {
  const fixture = harness()
  fixture.ctx.agentPresets = undefined
  await assert.rejects(
    creatorOverlay.applyOverlay(fixture.ctx, { scopePollMs: 1 }),
    /preset scope is unavailable for "cordis"/,
  )
})

test('non-finite scope timing options fall back to the defaults', async () => {
  const fixture = harness()
  await creatorOverlay.applyOverlay(fixture.ctx, { scopeWaitMs: Number.NaN, scopePollMs: Number.NaN })
  assert.equal(fixture.state().scopeKey.preset, 'cordis')
})

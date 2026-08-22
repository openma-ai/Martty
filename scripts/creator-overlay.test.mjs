import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import * as creatorOverlay from '../npm/lib/creator-overlay.js'

function harness() {
  const skills = new Map()
  const promptSections = []
  const tools = new Map()
  const effects = []
  const standingKey = { preset: 'cordis' }
  const requestedModules = []
  let requestedPreset
  let disposed = false
  let inspected
  let packageSource = {
    pluginId: 'skin-1',
    packageId: 'dyn-1',
    name: 'Paper Lantern',
    purpose: 'Warm terminal palette',
    code: { client: 'return { apply() {} }' },
    currentPackageId: 'dyn-1',
  }

  const overlay = {
    ctx: {
      get(name) {
        if (name === 'skills') {
          return {
            register(skill) {
              skills.set(skill.name, skill)
            },
          }
        }
        if (name === 'systemPrompt') {
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
    agentPresets: {
      async standingKeyFor(id) {
        requestedPreset = id
        return standingKey
      },
    },
    loader: {
      async import(name) {
        requestedModules.push(name)
        if (name === '@deepseek-ai/dsh-tools') {
          return { defineTool: (definition) => definition }
        }
        return {
          createScope(parent, key) {
            assert.equal(parent, ctx)
            assert.equal(key, standingKey)
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
    promptSections,
    skills,
    tools,
    setPackageSource(value) {
      packageSource = value
    },
    state: () => ({ disposed, inspected, requestedModules, requestedPreset }),
  }
}

test('exports one internal Host overlay with the required injected services', () => {
  assert.equal(creatorOverlay.name, 'tui-creator-overlay')
  assert.deepEqual(
    creatorOverlay.inject,
    ['agentPresets', 'skills', 'systemPrompt', 'loader', 'tools', 'dynamicCordisRunner'],
  )
})

test('adds the TUI companion skill only to the requested preset scope', async () => {
  const fixture = harness()
  await creatorOverlay.apply(fixture.ctx)

  const skill = fixture.skills.get('tui-plugin-development')
  assert.equal(fixture.state().requestedPreset, 'cordis')
  assert.equal(skill?.source, '@openma/deepseek-harness-tui/creator-overlay')
  assert.equal(skill?.invocation.modelInvocable, true)
  assert.equal(skill?.invocation.userInvocable, true)
  assert.match(skill?.description ?? '', /companion/i)
  assert.match(skill?.description ?? '', /load `cordis-plugin-development` first/i)
})

test('loads the profile-owned scope module instead of importing a private copy', async () => {
  const fixture = harness()
  await creatorOverlay.apply(fixture.ctx)
  assert.ok(fixture.state().requestedModules.includes('@deepseek-ai/dsh-scope'))
})

test('routes generic Cordis first and TUI behavior to the companion skill', async () => {
  const fixture = harness()
  await creatorOverlay.apply(fixture.ctx)

  assert.equal(fixture.promptSections.length, 1)
  const prompt = fixture.promptSections[0]
  assert.equal(prompt.name, 'tool:tui-cordis-routing')
  assert.match(prompt.text, /cordis-plugin-development.*every dynamic Plugin/is)
  assert.match(prompt.text, /TUI.*also load.*tui-plugin-development/is)
  assert.match(prompt.text, /browser|Web/i)
})

test('unloading the TUI overlay retracts the skill and prompt together', async () => {
  const fixture = harness()
  await creatorOverlay.apply(fixture.ctx)
  assert.equal(fixture.effects.length, 1)
  assert.match(fixture.effects[0].label, /tui-creator-overlay/)

  await fixture.effects[0].cleanup()

  assert.equal(fixture.state().disposed, true)
  assert.equal(fixture.skills.size, 0)
  assert.equal(fixture.promptSections.length, 0)
})

test('rejects an empty preset id without touching the preset registry', async () => {
  const fixture = harness()
  await assert.rejects(
    creatorOverlay.apply(fixture.ctx, { preset: '' }),
    /preset must be a non-empty string/,
  )
  assert.equal(fixture.state().requestedPreset, undefined)
})

test('Creator scope exposes explicit durable TUI artifact tools', async () => {
  const fixture = harness()
  const root = mkdtempSync(path.join(tmpdir(), 'martty-creator-artifacts-'))
  try {
    await creatorOverlay.apply(fixture.ctx, { artifactRoot: root })

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
    await creatorOverlay.apply(fixture.ctx, { artifactRoot: root })
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

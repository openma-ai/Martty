import assert from 'node:assert/strict'
import test from 'node:test'

import * as creatorOverlay from '../npm/lib/creator-overlay.js'

function harness() {
  const skills = new Map()
  const promptSections = []
  const effects = []
  const standingKey = { preset: 'cordis' }
  let requestedModule
  let requestedPreset
  let disposed = false

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
        requestedModule = name
        return {
          createScope(parent, key) {
            assert.equal(parent, ctx)
            assert.equal(key, standingKey)
            return overlay
          },
        }
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
    state: () => ({ disposed, requestedModule, requestedPreset }),
  }
}

test('exports one internal Host overlay with the required injected services', () => {
  assert.equal(creatorOverlay.name, 'tui-creator-overlay')
  assert.deepEqual(
    creatorOverlay.inject,
    ['agentPresets', 'skills', 'systemPrompt', 'loader'],
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
  assert.equal(fixture.state().requestedModule, '@deepseek-ai/dsh-scope')
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

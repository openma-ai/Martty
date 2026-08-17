/**
 * Add TUI plugin authoring guidance to the shipped Creator preset without
 * copying or modifying that preset's composition.
 */

import { readFileSync } from 'node:fs'

export const name = 'tui-creator-overlay'
export const inject = ['agentPresets', 'skills', 'systemPrompt', 'loader']

const SKILL_NAME = 'tui-plugin-development'
const ROUTING_PROMPT = `# Dynamic TUI Plugin routing

Load cordis-plugin-development for every dynamic Plugin; it owns the common Plugin, Package, lifecycle, approval, Host, RPC, and repair model.

- When any requested behavior belongs to the TUI — terminal themes or backgrounds, native TUI slots, local TUI commands, native overlays, or current ACP Session options — also load tui-plugin-development. Its TUI Provider guidance overrides generic browser UI assumptions only for that half.
- Browser/Web-only work needs no TUI companion. A mixed Plugin uses the generic skill for common, Host, and Web behavior and the TUI skill for its terminal behavior. Do not translate a TUI request into Web Slots or add Host code unless the requested behavior owns Host data.`

const skillDocument = readFileSync(
  new URL('../skills/tui-plugin-development/SKILL.md', import.meta.url),
  'utf8',
)
const skillFrontmatter = skillDocument.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/)
if (!skillFrontmatter) {
  throw new Error('tui-creator-overlay: TUI skill frontmatter is unavailable')
}
const skillDescription = skillFrontmatter[1]
  .match(/^description:\s*(.+)$/m)?.[1]
  ?.trim()
if (!skillDescription) {
  throw new Error('tui-creator-overlay: TUI skill description is unavailable')
}
const skillContent = skillDocument.slice(skillFrontmatter[0].length)

/**
 * @param {object} ctx
 * @param {{ preset?: string }} [config]
 */
export async function apply(ctx, config = {}) {
  const preset = config.preset ?? 'cordis'
  if (typeof preset !== 'string' || preset.length === 0) {
    throw new Error('tui-creator-overlay: preset must be a non-empty string')
  }
  const key = await ctx.agentPresets.standingKeyFor(preset)
  const scopeModule = await ctx.loader.import('@deepseek-ai/dsh-scope')
  if (typeof scopeModule?.createScope !== 'function') {
    throw new Error('tui-creator-overlay: profile dsh-scope module is unavailable')
  }
  const { createScope } = scopeModule
  const overlay = createScope(ctx, key)
  try {
    const skills = overlay.ctx.get('skills')
    if (skills === undefined || typeof skills.register !== 'function') {
      throw new Error('tui-creator-overlay: skills service is unavailable in the preset scope')
    }
    skills.register({
      name: SKILL_NAME,
      description: skillDescription,
      source: '@openma/deepseek-harness-tui/creator-overlay',
      content: skillContent,
      invocation: { modelInvocable: true, userInvocable: true },
    })
    const systemPrompt = overlay.ctx.get('systemPrompt')
    if (systemPrompt === undefined || typeof systemPrompt.section !== 'function') {
      throw new Error('tui-creator-overlay: systemPrompt service is unavailable in the preset scope')
    }
    systemPrompt.section({
      name: 'tool:tui-cordis-routing',
      order: 116,
      text: ROUTING_PROMPT,
    })
    ctx.effect(() => () => overlay.dispose(), `tui-creator-overlay(${JSON.stringify(preset)})`)
  } catch (error) {
    await overlay.dispose()
    throw error
  }
}

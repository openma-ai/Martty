/**
 * Add TUI plugin authoring guidance to the shipped Creator preset without
 * copying or modifying that preset's composition.
 */

import { readFileSync } from 'node:fs'
import { createTuiPluginStore } from './tui-plugin-store.js'

export const name = 'tui-creator-overlay'
export const inject = [
  'agentPresets', 'skills', 'systemPrompt', 'loader', 'tools', 'dynamicCordisRunner',
]

const SKILL_NAME = 'tui-plugin-development'
const ROUTING_PROMPT = `# Dynamic TUI Plugin routing

Load cordis-plugin-development for every dynamic Plugin; it owns the common Plugin, Package, lifecycle, approval, Host, RPC, and repair model.

- When any requested behavior belongs to the TUI — terminal themes or backgrounds, native TUI slots, local TUI commands, native overlays, or current ACP Session options — also load tui-plugin-development. Its TUI Provider guidance overrides generic browser UI assumptions only for that half.
- Browser/Web-only work needs no TUI companion. A mixed Plugin uses the generic skill for common, Host, and Web behavior and the TUI skill for its terminal behavior. Do not translate a TUI request into Web Slots or add Host code unless the requested behavior owns Host data.`

const jsonOutput = {
  schema: { type: 'json' },
  render: (_args, value) => [{ type: 'text', text: JSON.stringify(value, null, 2) }],
}

function requireAgent(exec) {
  if (exec?.agent === undefined) {
    throw new Error('TUI artifact tools require an Agent-backed Creator session')
  }
  return exec.agent
}

function registerArtifactTools(ctx, scopedCtx, defineTool, store) {
  const tools = scopedCtx.get('tools')
  if (tools === undefined || typeof tools.register !== 'function') {
    throw new Error('tui-creator-overlay: tools service is unavailable in the preset scope')
  }

  tools.register(defineTool({
    name: 'tui_plugin_save',
    description:
      'Persist one successfully activated, Client-only Cordis Package as a durable TUI artifact. '
      + 'Use this only after cordis_run succeeds. UI and Theme Plugins survive restart only '
      + 'after this Tool returns saved. Replacing an existing artifact must be explicit.',
    parameters: {
      artifactId: {
        type: 'string',
        required: true,
        description: 'Stable lowercase artifact id used as its directory name.',
      },
      kind: {
        type: 'string',
        required: true,
        enum: ['ui', 'theme'],
        description: 'Durable TUI contribution kind.',
      },
      pluginId: {
        type: 'string',
        required: true,
        description: 'Dynamic Plugin id returned by cordis_define.',
      },
      packageId: {
        type: 'string',
        required: true,
        description: 'Exact immutable Package id that cordis_run activated successfully.',
      },
      replace: {
        type: 'boolean',
        description: 'Replace an existing artifact with this Package. Defaults to false.',
      },
    },
    output: jsonOutput,
    async execute(args, exec) {
      const source = ctx.dynamicCordisRunner.inspectPackage(
        requireAgent(exec),
        args.pluginId,
        args.packageId,
      )
      if (source.currentPackageId !== args.packageId) {
        throw new Error(
          `tui_plugin_save: ${args.pluginId}/${args.packageId} is not the successful current Package; `
          + 'run it successfully before persisting it',
        )
      }
      if (typeof source.code?.host === 'string') {
        throw new Error(
          'tui_plugin_save: durable TUI artifacts are client-only; a Package with a Host half '
          + 'belongs to the connected Harness and needs its separate shipping path',
        )
      }
      if (typeof source.code?.client !== 'string') {
        throw new Error('tui_plugin_save: Package has no Client half')
      }
      const saved = store.save({
        id: args.artifactId,
        kind: args.kind,
        name: source.name,
        purpose: source.purpose,
        source: { pluginId: args.pluginId, packageId: args.packageId },
        clientCode: source.code.client,
      }, { replace: args.replace === true })
      return {
        status: 'saved',
        artifact: { id: args.artifactId, kind: args.kind },
        path: saved.path,
        recovery: 'The TUI Client discovers this artifact from disk on startup.',
      }
    },
  }))

  tools.register(defineTool({
    name: 'tui_plugin_list',
    description:
      'List durable user-authored TUI artifacts from disk, including broken rows that need repair or removal.',
    parameters: {},
    output: jsonOutput,
    execute(_args, exec) {
      requireAgent(exec)
      return Promise.resolve({ root: store.root, artifacts: store.list() })
    },
    isConcurrencySafe() {
      return true
    },
  }))

  tools.register(defineTool({
    name: 'tui_plugin_read',
    description:
      'Read one durable Creator-authored TUI artifact including its exact code.client source. '
      + 'Use this before continuing development after a restart; define a new temporary preview '
      + 'Plugin from the source, then save the successful result with replace:true.',
    parameters: {
      artifactId: {
        type: 'string',
        required: true,
        description: 'Exact durable artifact id returned by tui_plugin_list.',
      },
    },
    output: jsonOutput,
    execute(args, exec) {
      requireAgent(exec)
      return Promise.resolve({ artifact: store.resolve(args.artifactId) })
    },
    isConcurrencySafe() {
      return true
    },
  }))

  tools.register(defineTool({
    name: 'tui_plugin_remove',
    description:
      'Remove one durable user-authored TUI artifact from disk. This does not undefine the current '
      + 'Session\'s temporary preview Plugin.',
    parameters: {
      artifactId: {
        type: 'string',
        required: true,
        description: 'Exact durable artifact id returned by tui_plugin_list.',
      },
    },
    output: jsonOutput,
    execute(args, exec) {
      requireAgent(exec)
      const removed = store.remove(args.artifactId)
      return Promise.resolve({
        status: removed ? 'removed' : 'absent',
        artifactId: args.artifactId,
      })
    },
  }))
}

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
    const toolsModule = await ctx.loader.import('@deepseek-ai/dsh-tools')
    if (typeof toolsModule?.defineTool !== 'function') {
      throw new Error('tui-creator-overlay: profile dsh-tools module is unavailable')
    }
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
    registerArtifactTools(
      ctx,
      overlay.ctx,
      toolsModule.defineTool,
      createTuiPluginStore({ root: config.artifactRoot }),
    )
    ctx.effect(() => () => overlay.dispose(), `tui-creator-overlay(${JSON.stringify(preset)})`)
  } catch (error) {
    await overlay.dispose()
    throw error
  }
}

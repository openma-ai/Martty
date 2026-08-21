/** Built-in Client Plugin: the `/status` run-state overlay. */

import { formatDuration, formatRate, formatTokens } from './stats-view.js'

export const name = 'status-view'
export const inject = ['acpSessionStatus', 'acpSessionStats', 'tuiCommands', 'tuiOverlay']

export function apply(ctx) {
  const stopCommand = ctx.tuiCommands.register({
    name: 'status',
    description: 'Session run state and key stats',
  }, async () => {
    ctx.tuiOverlay.openView({
      id: 'status',
      title: 'Status',
      nodes: [{
        id: 'content',
        kind: 'markdown',
        text: statusMarkdown(
          ctx.acpSessionStatus.current(),
          ctx.acpSessionStats.current(),
        ),
      }],
    })
  })
  return () => stopCommand?.()
}

/**
 * One `## status` markdown node. Run-state facts come from
 * `acpSessionStatus`; every token/turn/step/timing fact comes from
 * `acpSessionStats.current()` — the same snapshot `stats-view` renders in
 * the composer dock, so the two readouts can never drift apart.
 */
function statusMarkdown(status, stats) {
  const usage = stats?.usage ?? {}
  const folded = stats?.stats ?? {}
  const total = (usage.input ?? 0) + (usage.output ?? 0)
    + (usage.cached ?? 0) + (usage.reasoning ?? 0)
  const lines = ['## status', '']
  lines.push(`- state · ${status.state ?? 'idle'}`)
  lines.push(`- acp · ${status.connection ?? 'not attached'}`)
  if (status.auth?.status !== undefined) {
    lines.push(`- auth · ${status.auth.status}${status.auth.method ? ` · ${status.auth.method}` : ''}`)
  }
  lines.push(`- session · ${status.session?.bound ? status.session.sessionId : 'unbound'}`)
  if (status.server !== undefined) lines.push(`- server · ${status.server}`)
  if (status.model !== undefined) lines.push(`- model · ${status.model}`)
  if (status.agent !== undefined) lines.push(`- agent · ${status.agent}`)
  if (status.permission !== undefined) lines.push(`- permission · ${status.permission}`)
  if (status.plan !== undefined) lines.push(`- plan · ${status.plan ? 'on' : 'off'}`)
  if (status.effort !== undefined) lines.push(`- effort · ${status.effort}`)
  lines.push(
    `- tokens · ↑${formatTokens(usage.input ?? 0)} ↓${formatTokens(usage.output ?? 0)}`
    + ` (cached ${formatTokens(usage.cached ?? 0)}) · Σ ${formatTokens(total)}`,
  )
  lines.push(`- turns · ${folded.turns ?? 0} · steps · ${folded.steps ?? 0}`)
  lines.push(`- LLM · ${formatDuration(folded.llmMillis ?? 0)} · tool · ${formatDuration(folded.toolMillis ?? 0)}`)
  if ((folded.ttftCount ?? 0) > 0) {
    lines.push(`- TTFT avg · ${formatDuration(folded.ttftTotalMillis / folded.ttftCount)}`)
  }
  const llmMillis = folded.llmMillis ?? 0
  const output = usage.output ?? 0
  if (llmMillis > 0 && output > 0) {
    lines.push(`- rate · ${formatRate(output / (llmMillis / 1000))} tok/s`)
  }
  return lines.join('\n')
}

export { statusMarkdown }

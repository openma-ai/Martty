/** Built-in Client Plugin: standard ACP usage and timing in the composer dock. */

export const name = 'stats-view'
export const inject = ['acpSessionStats', 'tuiSlots']

export function apply(ctx) {
  let current = ctx.acpSessionStats.current()
  let panel
  const stopSlot = ctx.tuiSlots.inject('conversation.composer.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.composer.dock', id: 'stats' },
      nodesOf(current),
    )
    return () => panel.dispose()
  })
  const stopStats = ctx.acpSessionStats.subscribe((snapshot) => {
    current = snapshot
    panel?.update(nodesOf(current))
  })
  return () => {
    stopStats?.()
    stopSlot?.()
  }
}

function nodesOf(snapshot) {
  const usage = snapshot?.usage ?? {}
  const stats = snapshot?.stats ?? {}
  const nodes = []
  if ((usage.input ?? 0) > 0 || (usage.output ?? 0) > 0) {
    nodes.push(node('tokens', `Input ${formatTokens(usage.input)} tok · Output ${formatTokens(usage.output)} tok`))
  }
  if ((stats.turns ?? 0) > 0 || (stats.steps ?? 0) > 0) {
    nodes.push(node(
      'counts',
      `${stats.turns ?? 0} ${plural(stats.turns ?? 0, 'turn')} · ${stats.steps ?? 0} ${plural(stats.steps ?? 0, 'step')}`,
    ))
  }
  if ((usage.input ?? 0) > 0) {
    const cacheRead = usage.cacheRead ?? usage.cached ?? 0
    const cacheWrite = usage.cacheWrite ?? 0
    const eligible = usage.input + cacheRead + cacheWrite
    const rate = eligible > 0 ? Math.round(cacheRead / eligible * 100) : 0
    nodes.push(node('cache', `Cache hit ${rate}%`))
  }
  if ((stats.llmMillis ?? 0) > 0 || (stats.toolMillis ?? 0) > 0) {
    nodes.push(node(
      'time',
      `LLM ${formatDuration(stats.llmMillis ?? 0)} · Tool call ${formatDuration(stats.toolMillis ?? 0)}`,
    ))
  }
  if ((stats.ttftCount ?? 0) > 0 || ((usage.output ?? 0) > 0 && (stats.llmMillis ?? 0) > 0)) {
    const parts = []
    if ((stats.ttftCount ?? 0) > 0) {
      parts.push(`TTFT avg ${formatDuration(stats.ttftTotalMillis / stats.ttftCount)}`)
    }
    if ((usage.output ?? 0) > 0 && (stats.llmMillis ?? 0) > 0) {
      parts.push(`${formatRate(usage.output / (stats.llmMillis / 1000))} tok/s`)
    }
    nodes.push(node('speed', parts.join(' · ')))
  }
  return nodes
}

function node(id, title) {
  return { id, kind: 'generic', title, body: '' }
}

function plural(value, singular) { return value === 1 ? singular : `${singular}s` }

function formatTokens(value = 0) {
  if (value < 1000) return String(value)
  const unit = value < 1_000_000 ? 1000 : 1_000_000
  const suffix = unit === 1000 ? 'K' : 'M'
  const scaled = value / unit
  const rounded = scaled >= 100 ? Math.round(scaled) : Math.round(scaled * 10) / 10
  return `${rounded}${suffix}`
}

function formatDuration(ms) {
  if (ms < 60_000) return `${Math.round(ms / 100) / 10}s`
  const seconds = Math.round(ms / 1000)
  return `${Math.floor(seconds / 60)}m${seconds % 60}s`
}

function formatRate(value) {
  return String(Math.round(value * 10) / 10)
}

export { formatDuration, formatRate, formatTokens, nodesOf }

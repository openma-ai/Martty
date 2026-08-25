/** Built-in Client Plugin: standard ACP usage and timing in the composer dock. */

export const name = 'stats-view'
export const inject = ['acpSessionStats', 'acpSessionStatus', 'tuiSlots']

export function apply(ctx) {
  let current = ctx.acpSessionStats.current()
  let status = ctx.acpSessionStatus.current()
  let panel
  const stopSlot = ctx.tuiSlots.inject('conversation.composer.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.composer.dock', id: 'stats' },
      nodesOf(current, status),
    )
    return () => panel.dispose()
  })
  const stopStats = ctx.acpSessionStats.subscribe((snapshot) => {
    current = snapshot
    panel?.update(nodesOf(current, status))
  })
  const stopStatus = ctx.acpSessionStatus.subscribe((snapshot) => {
    status = snapshot
    panel?.update(nodesOf(current, status))
  })
  return () => {
    stopStats?.()
    stopStatus?.()
    stopSlot?.()
  }
}

function nodesOf(snapshot, status) {
  const usage = snapshot?.usage ?? {}
  const stats = snapshot?.stats ?? {}
  const model = status?.model
  const nodes = []
  if ((usage.input ?? 0) > 0 || (usage.output ?? 0) > 0) {
    nodes.push(node('tokens', `↑${formatTokens(usage.input)} · ↓${formatTokens(usage.output)}`))
    const used = (usage.input ?? 0) + (usage.output ?? 0)
      + (usage.cached ?? 0) + (usage.reasoning ?? 0)
    const size = contextWindowOf(model)
    if (used > 0 && size > 0) {
      const pct = Math.min(100, used / size * 100).toFixed(1)
      nodes.push(node('context', `${pct}%/${contextSizeLabel(size)}`))
    }
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

/**
 * Context window of the current model (tokens). The ACP surface does not
 * carry the window size, so the known DeepSeek family sizes stand in;
 * unknown models fall back to a common 128K.
 */
function contextWindowOf(model) {
  switch (model) {
    case 'deepseek-v4-flash':
    case 'deepseek-v4-pro':
      return 1_000_000
    default:
      return 128_000
  }
}

/** `1000000` → `1.0M`, `128000` → `128K` — keeps the one-decimal look of
 * the percentage side (e.g. `17.5%/1.0M`). */
function contextSizeLabel(size) {
  if (size >= 1_000_000) {
    const scaled = size / 1_000_000
    return `${scaled >= 100 ? Math.round(scaled) : scaled.toFixed(1)}M`
  }
  return formatTokens(size)
}

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

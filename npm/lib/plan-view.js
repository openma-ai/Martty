/** Built-in Client Plugin: ACP Plan dock plus a `/plan-view` fallback modal. */

export const name = 'plan-view'
export const inject = ['acpSessionPlan', 'tuiSlots', 'tuiCommands', 'tuiOverlay']

export function apply(ctx) {
  let current = ctx.acpSessionPlan.current()
  let panel

  const stopSlot = ctx.tuiSlots.inject('conversation.input.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.input.dock', id: 'plan-view' },
      dockNodes(current),
    )
    return () => panel.dispose()
  })
  const stopPlan = ctx.acpSessionPlan.subscribe((snapshot) => {
    current = snapshot.plans.at(-1) ?? null
    panel?.update(dockNodes(current))
  })
  const stopCommand = ctx.tuiCommands.register({
    name: 'plan-view',
    description: 'Open the current ACP plan',
  }, async () => {
    current = ctx.acpSessionPlan.current()
    ctx.tuiOverlay.openView({
      id: 'plan-view',
      title: 'Plan',
      nodes: viewNodes(current),
    })
  })

  return () => {
    stopCommand?.()
    stopPlan?.()
    stopSlot?.()
  }
}

function dockNodes(plan) {
  if (plan === null) return []
  if (plan.kind === 'items') {
    const total = plan.entries.length
    if (total === 0) return []
    const completed = plan.entries.filter((entry) => entry.status === 'completed').length
    const focus = plan.entries.find((entry) => entry.status === 'in_progress')
      ?? plan.entries.find((entry) => entry.status !== 'completed')
      ?? plan.entries.at(-1)
    return [{
      id: 'summary',
      kind: 'generic',
      title: `Plan · ${completed}/${total}${focus ? ` · ${focus.content}` : ''}`,
      body: '',
      action: { kind: 'command', name: 'plan-view', args: '' },
      ...(completed === total ? { status: 'ok' } : { status: 'running' }),
    }]
  }
  return [{
    id: 'summary',
    kind: 'generic',
    title: plan.kind === 'file' ? `Plan · ${plan.uri}` : 'Plan · available',
    body: '',
    action: { kind: 'command', name: 'plan-view', args: '' },
    status: 'running',
  }]
}

function viewNodes(plan) {
  if (plan === null) {
    return [{ id: 'empty', kind: 'notice', level: 'info', text: 'No active plan' }]
  }
  if (plan.kind === 'markdown') {
    return [{ id: 'content', kind: 'markdown', text: plan.content }]
  }
  if (plan.kind === 'file') {
    return [{ id: 'file', kind: 'notice', level: 'info', text: plan.uri }]
  }
  // Items render as one markdown node: a task-list review the transcript
  // pipeline can fully render (headings, bold, strikethrough).
  const total = plan.entries.length
  const completed = plan.entries.filter((entry) => entry.status === 'completed').length
  const lines = [`## Plan · ${completed}/${total}`, '']
  for (const entry of plan.entries) {
    const content = entry.content.replace(/\s*\n\s*/g, ' ')
    let item
    if (entry.status === 'completed') {
      item = `- [x] ${content}`
    } else if (entry.status === 'cancelled' || entry.status === 'failed') {
      item = `- [ ] ~~${content}~~`
    } else {
      item = entry.status === 'in_progress' ? `- [ ] **${content}**` : `- [ ] ${content}`
    }
    if (entry.priority) item += ` · priority · ${entry.priority}`
    lines.push(item)
  }
  return [{ id: 'content', kind: 'markdown', text: lines.join('\n') }]
}

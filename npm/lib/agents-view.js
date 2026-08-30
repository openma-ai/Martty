/** Built-in Client Plugin: Agent navigation above the composer's metadata row. */

export const name = 'agents-view'
export const inject = ['tuiAgents', 'tuiSlots', 'tuiCommands']

export function apply(ctx) {
  let current = ctx.tuiAgents.current()
  // Issue #80: `/agents` is an on/off switch for the panel. `visible` is
  // the user preference; `forced` remembers `/agents on` so the summary
  // stays open even after every task ended, until the next batch starts.
  let visible = true
  let forced = false
  let panel

  const stopSlot = ctx.tuiSlots.inject('conversation.navigation.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.navigation.dock', id: 'agents-view', order: 0 },
      dockNodes(current, visible, forced),
    )
    return () => panel.dispose()
  })
  const stopAgents = ctx.tuiAgents.subscribe((snapshot) => {
    current = snapshot
    // A new running batch clears the forced-open state so the panel
    // auto-closes again once this batch ends.
    if (snapshot.items.some((item) => item.kind === 'subagent' && item.status === 'running')) {
      forced = false
    }
    panel?.update(dockNodes(current, visible, forced))
  })
  const stopCommand = ctx.tuiCommands.register({
    name: 'agents',
    description: 'Toggle the Agent panel (on/off)',
    input: { hint: '[on|off|agent-id]' },
  }, async (args) => {
    const arg = args.trim().toLowerCase()
    if (arg === 'on') {
      visible = true
      forced = true
    } else if (arg === 'off') {
      visible = false
      forced = false
    } else if (arg === '') {
      visible = !visible
      forced = visible
    } else {
      return ctx.tuiAgents.select(args.trim())
    }
    panel?.update(dockNodes(current, visible, forced))
    return true
  })

  return () => {
    stopCommand?.()
    stopAgents?.()
    stopSlot?.()
  }
}

function dockNodes(snapshot, visible = true, forced = false) {
  if (!Array.isArray(snapshot?.items) || snapshot.items.length < 2) return []
  const selecting = snapshot.selectedId !== null && snapshot.selectedId !== undefined
  if (!visible && !selecting) return []
  if (!selecting) {
    const agents = snapshot.items.filter((item) => item.kind === 'subagent')
    const marked = agents.filter((item) => item.current !== false)
    const current = marked.length > 0 ? marked : agents
    const completed = current.filter((item) => item.status === 'finished' || item.status === 'failed').length
    const running = current.some((item) => item.status === 'running')
    const failed = current.some((item) => item.status === 'failed')
    // Issue #80: auto-close once every Agent task has ended. A failed task
    // keeps the summary so the failure stays visible; `/agents on` (forced)
    // holds it open until the next batch starts running.
    if (!running && !failed && !forced) return []
    return [
      {
        id: 'summary', kind: 'generic', title: '· Agents', body: `${completed}/${current.length}`,
        status: failed ? 'err' : running ? 'running' : 'done', tone: 'caption',
      },
      {
        id: 'switch', kind: 'generic', title: '↓ expand', body: '', tone: 'caption',
      },
    ]
  }
  const historyActive = snapshot.items.some((item) => (
    item.kind === 'subagent'
      && item.current === false
      && sameId(item.id, snapshot.activeId)
  ))
  const visibleItems = snapshot.items
    .filter((item) => item.kind !== 'subagent' || item.current !== false)
    .sort((left, right) => Number(left.kind === 'history') - Number(right.kind === 'history'))
  return [
    {
      id: 'label', kind: 'generic', title: '· Agents', body: '',
      tone: 'caption',
    },
    ...visibleItems.map((item) => agentNode(item, {
      active: sameId(item.id, snapshot.activeId) || (item.kind === 'history' && historyActive),
      focused: selecting && sameId(item.id, snapshot.selectedId),
      selecting,
    })),
    {
      id: 'switch',
      kind: 'generic',
      title: '←/→ · enter · esc close',
      body: '',
      tone: 'caption',
    },
  ]
}

function agentNode(item, state) {
  const prefix = state.focused
    ? '▸ '
    : state.selecting && state.active
      ? '• '
      : !state.selecting && state.active
        ? '▸ '
        : ''
  return {
    id: `agent-${item.id}`,
    kind: 'generic',
    title: `${prefix}${item.label}`,
    body: '',
    ...(item.kind === 'subagent' && item.status === 'running'
      ? { status: 'running' }
      : item.kind === 'subagent' && item.status === 'finished'
        ? { status: 'done' }
        : item.kind === 'subagent' && item.status === 'failed'
          ? { status: 'err' }
          : {}),
    ...(state.focused ? { selected: true } : {}),
    tone: state.focused
      ? 'brand'
      : state.active
        ? state.selecting ? 'brand_soft' : 'brand'
        : item.kind === 'history' ? 'caption' : 'fg',
  }
}

function sameId(left, right) {
  return right !== null && right !== undefined && String(left) === String(right)
}

export { dockNodes }

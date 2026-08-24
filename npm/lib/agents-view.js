/** Built-in Client Plugin: Agent navigation above the composer's metadata row. */

export const name = 'agents-view'
export const inject = ['tuiAgents', 'tuiSlots', 'tuiCommands']

export function apply(ctx) {
  let current = ctx.tuiAgents.current()
  let panel

  const stopSlot = ctx.tuiSlots.inject('conversation.navigation.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.navigation.dock', id: 'agents-view', order: 0 },
      dockNodes(current),
    )
    return () => panel.dispose()
  })
  const stopAgents = ctx.tuiAgents.subscribe((snapshot) => {
    current = snapshot
    panel?.update(dockNodes(current))
  })
  const stopCommand = ctx.tuiCommands.register({
    name: 'agents',
    description: 'Switch the visible Agent transcript',
  }, async (args) => {
    current = ctx.tuiAgents.current()
    const target = args.trim()
    if (target.length > 0) {
      return ctx.tuiAgents.select(target)
    }
    return ctx.tuiAgents.navigate('begin')
  })

  return () => {
    stopCommand?.()
    stopAgents?.()
    stopSlot?.()
  }
}

function dockNodes(snapshot) {
  if (!Array.isArray(snapshot?.items) || snapshot.items.length < 2) return []
  const selecting = snapshot.selectedId !== null && snapshot.selectedId !== undefined
  if (!selecting) {
    const agents = snapshot.items.filter((item) => item.kind === 'subagent')
    const marked = agents.filter((item) => item.current !== false)
    const current = marked.length > 0 ? marked : agents
    const completed = current.filter((item) => item.status === 'finished' || item.status === 'failed').length
    const running = current.some((item) => item.status === 'running')
    const failed = current.some((item) => item.status === 'failed')
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

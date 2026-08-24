/** Built-in Client Plugin: native Queue state in the composer input dock. */

export const name = 'queue-view'
export const inject = ['tuiQueue', 'tuiSlots']

export function apply(ctx) {
  let current = ctx.tuiQueue.current()
  let panel

  const stopSlot = ctx.tuiSlots.inject('conversation.input.dock', () => {
    panel = ctx.tuiSlots.register(
      { name: 'conversation.input.dock', id: 'queue-view', order: -10 },
      dockNodes(current),
    )
    return () => panel.dispose()
  })
  const stopQueue = ctx.tuiQueue.subscribe((snapshot) => {
    current = snapshot
    panel?.update(dockNodes(current))
  })

  return () => {
    stopQueue?.()
    stopSlot?.()
  }
}

function dockNodes(queue) {
  if (!Array.isArray(queue?.items) || queue.items.length === 0) return []
  const expanded = queue.selectedId !== null || queue.editingId !== null
  const count = Number.isInteger(queue.count) ? queue.count : queue.items.length
  const title = expanded
    ? queue.editingId !== null
      ? `Queue · ${count} · enter save · ctrl+d delete · esc cancel`
      : `Queue · ${count} · ↑/↓ choose · enter edit · esc close`
    : `Queue · ${count} · enter send first · ⌥↑ edit`
  const nodes = [{
    id: 'summary',
    kind: 'generic',
    title,
    body: '',
    status: 'running',
  }]
  nodes.push({
    id: 'items',
    kind: 'group',
    children: queue.items.map((item) => {
      const editing = sameId(item.id, queue.editingId)
      const selected = sameId(item.id, queue.selectedId)
      const deleting = editing && queue.deleteConfirm
      return {
        id: `item-${item.id}`,
        kind: 'text',
        text: `${deleting ? '×' : editing ? '✎' : selected ? '▸' : '›'} ${item.ordinal}  ${item.summary}`,
        tone: deleting ? 'err' : editing ? 'warn' : selected ? 'brand' : 'fg_secondary',
      }
    }),
  })
  return nodes
}

function sameId(left, right) {
  return right !== null && String(left) === String(right)
}

export { dockNodes }

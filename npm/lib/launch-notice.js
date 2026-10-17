/**
 * Built-in Client Plugin: one launch-time notice in the composer dock.
 *
 * Used when the launcher had to change course before the TUI started — for
 * example `martty --dsh <profile>` falling back to the bundled harness
 * because the global dsh is below the supported floor. The notice is short,
 * names the reason, and goes away on `/dismiss` or after `dismissAfterMs`.
 */

export const name = 'launch-notice'
export const inject = ['tuiSlots', 'tuiCommands']

export const DEFAULT_DISMISS_AFTER_MS = 60_000

/**
 * @param {{ text?: string, level?: 'info' | 'warn' | 'error', dismissAfterMs?: number }} [config]
 */
export function apply(ctx, config = {}) {
  if (typeof config.text !== 'string' || config.text.trim() === '') return undefined
  const level = ['info', 'warn', 'error'].includes(config.level) ? config.level : 'warn'
  const controller = ctx.tuiSlots.register(
    { name: 'conversation.input.dock', id: 'launch-notice', order: -30 },
    [{ id: 'notice', kind: 'notice', level, text: `${config.text.trim()}\n(/dismiss to hide)` }],
  )
  let stopCommand
  let timer
  let dismissed = false
  const dismiss = () => {
    if (dismissed) return
    dismissed = true
    clearTimeout(timer)
    controller.dispose()
    stopCommand?.()
    stopCommand = undefined
  }
  stopCommand = ctx.tuiCommands.register(
    { name: 'dismiss', description: 'Hide the launch notice' },
    async () => dismiss(),
  )
  const after = Number.isFinite(config.dismissAfterMs) ? config.dismissAfterMs : DEFAULT_DISMISS_AFTER_MS
  if (after > 0) {
    timer = setTimeout(dismiss, after)
    timer.unref?.()
  }
  return dismiss
}

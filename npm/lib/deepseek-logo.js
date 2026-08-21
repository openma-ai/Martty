/** Built-in Client Plugin: the classic DeepSeek Harness whale lockup. */

export const name = 'deepseek-logo'
export const inject = ['tuiCommands', 'tuiSlots']

export function apply(ctx) {
  let hero
  const stopCommand = ctx.tuiCommands.register({
    name: 'deepseeklogo',
    description: 'Replace the welcome hero with classic DeepSeek Harness',
  }, async (args = '') => {
    const preset = args.trim().toLowerCase()
    if (['martty', 'default', 'off'].includes(preset)) {
      if (hero !== undefined) {
        hero.dispose()
        hero = undefined
      } else {
        // A persisted DeepSeek selection can outlive this process-local
        // handle. Publish an empty snapshot once to select builtin Martty.
        ctx.tuiSlots.register(
          { name: 'welcome.hero', id: 'deepseek-logo-reset' },
          [],
        ).dispose()
      }
      return
    }
    if (preset !== '' && preset !== 'deepseek') {
      throw new Error('usage: /deepseeklogo [deepseek|martty]')
    }
    if (hero !== undefined) return
    hero = ctx.tuiSlots.register(
      { name: 'welcome.hero', id: 'deepseek-logo' },
      [
        { id: 'logo', kind: 'logo', name: 'deepseek' },
        {
          id: 'hint',
          kind: 'text',
          text: 'Into the Unknown',
          tone: 'fg_tertiary',
        },
      ],
    )
  })
  return () => {
    hero?.dispose()
    stopCommand?.()
  }
}

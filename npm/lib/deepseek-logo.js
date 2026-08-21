/** Built-in Client Plugin: the classic DeepSeek Harness UI Preset. */

export const name = 'deepseek-logo'
export const inject = ['tuiPresets', 'tuiSlots']

export function apply(ctx) {
  return ctx.tuiPresets.register({ id: 'deepseek', label: 'DeepSeek' }, () => {
    const hero = ctx.tuiSlots.register(
      { name: 'welcome.hero', id: 'deepseek-hero' },
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
    const info = ctx.tuiSlots.register(
      { name: 'welcome.info', id: 'deepseek-info' },
      [{ id: 'info', kind: 'welcomeinfo' }],
    )
    return () => {
      info.dispose()
      hero.dispose()
    }
  })
}

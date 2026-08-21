/** Built-in Martty UI Preset. */

export const name = 'martty-preset'
export const inject = ['tuiPresets', 'tuiSlots']

export function apply(ctx) {
  return ctx.tuiPresets.register({ id: 'default', label: 'Martty' }, () => {
    const hero = ctx.tuiSlots.register(
      { name: 'welcome.hero', id: 'martty-hero' },
      [
        { id: 'logo', kind: 'logo', name: 'martty' },
        { id: 'hint', kind: 'text', text: 'https://martty.sh', tone: 'fg_tertiary' },
      ],
    )
    const info = ctx.tuiSlots.register(
      { name: 'welcome.info', id: 'martty-info' },
      [{ id: 'info', kind: 'welcomeinfo' }],
    )
    return () => {
      info.dispose()
      hero.dispose()
    }
  })
}

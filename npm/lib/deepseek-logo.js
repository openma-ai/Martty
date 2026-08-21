/** Built-in Client Plugin: the classic DeepSeek Harness whale lockup. */

export const name = 'deepseek-logo'
export const inject = ['tuiCommands', 'tuiSlots']

const WHALE_LG = [
  '          ▄▄▄▄ ▄▄▄███      █▄',
  '     ▄▄█████████████       ███▄▄     ▄█',
  '   ▄█████████████████▄▄    █████▄██████',
  '  ▄█████████████████████▄  ▀██████████',
  ' ▄████████████████████████▄  ▀████▀▀▀',
  ' ██▀    ▀▀▀██████████▀▀█████▄████',
  ' ███         ▀███████▀▄ ▀████████',
  ' ███           ▀███████   ██████▀',
  ' ████            ▀██████████████',
  '  ████            ▀████████████',
  '   ████▄     ▄▄▄    █████████▀',
  '    ▀████▄    ███▄▄  ▀██████▄',
  '      ▀█████████████▄▄▄████████',
  '        ▀▀████████████▀▀',
  '             ▀▀▀▀▀▀',
]

const WORDMARK_SMALL = [
  ' ___  ___ ___ ___  ___ ___ ___ _  __',
  '|   \\| __| __| _ \\/ __| __| __| |/ /',
  '| |) | _|| _||  _/\\__ \\ _|| _|| \' < ',
  '|___/|___|___|_|  |___/___|___|_|\\_\\',
]

export function apply(ctx) {
  let hero
  const stopCommand = ctx.tuiCommands.register({
    name: 'deepseeklogo',
    description: 'Replace the welcome hero with classic DeepSeek Harness',
  }, async () => {
    if (hero !== undefined) return
    hero = ctx.tuiSlots.register(
      { name: 'welcome.hero', id: 'deepseek-logo' },
      [
        { id: 'whale', kind: 'ascii', lines: WHALE_LG, tone: 'brand' },
        {
          id: 'wordmark',
          kind: 'ascii',
          lines: [...WORDMARK_SMALL, 'H A R N E S S', 'Into the Unknown'],
          tone: 'fg_secondary',
        },
      ],
    )
  })
  return () => {
    hero?.dispose()
    stopCommand?.()
  }
}

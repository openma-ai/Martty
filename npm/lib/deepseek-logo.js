/** Built-in Client Plugin: the classic DeepSeek Harness whale lockup. */

export const name = 'deepseek-logo'
export const inject = ['tuiCommands', 'tuiOverlay']

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

export function deepseekLogoMarkdown() {
  return [
    '## DeepSeek Harness',
    '',
    '```text',
    ...WHALE_LG,
    '',
    ...WORDMARK_SMALL,
    'H A R N E S S',
    '```',
    '',
    '_Into the Unknown_',
  ].join('\n')
}

export function apply(ctx) {
  const stopCommand = ctx.tuiCommands.register({
    name: 'deepseeklogo',
    description: 'Open the classic DeepSeek Harness whale',
  }, async () => {
    ctx.tuiOverlay.openView({
      id: 'deepseek-logo',
      title: 'DeepSeek Harness',
      nodes: [{
        id: 'lockup',
        kind: 'markdown',
        text: deepseekLogoMarkdown(),
      }],
    })
  })
  return () => stopCommand?.()
}

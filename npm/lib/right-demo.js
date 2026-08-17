/** A deliberately rich `chrome.right` gallery; not mounted by default. */

export const name = 'tui-right-slot-gallery'
export const inject = ['tuiSlots']

export function apply(ctx) {
  ctx.tuiSlots.inject('chrome.right', () => {
    const panel = ctx.tuiSlots.register(
      { name: 'chrome.right', id: 'slot-gallery', order: 10 },
      [
        {
          id: 'mission-control',
          kind: 'group',
          title: 'Mission Control',
          tone: 'brand',
          children: [
            {
              id: 'intro',
              kind: 'markdown',
              text: '**Live plugin canvas**\n\nAnything below is owned by one sibling Cordis plugin.',
            },
            {
              id: 'tests',
              kind: 'generic',
              title: 'Regression suite',
              body: '247 checks passed',
              status: 'ok',
            },
            {
              id: 'deploy',
              kind: 'terminal',
              title: 'Preview deploy',
              body: '$ dsh plugin run slot-gallery\nmounted chrome.right\nwatching source…',
              exit: 0,
            },
            {
              id: 'patch',
              kind: 'diff',
              title: 'Live patch',
              path: 'client/plugin.js',
              unified: '@@ -1 +1 @@\n-static card\n+live TuiNode tree',
            },
          ],
        },
        {
          id: 'hint',
          kind: 'notice',
          level: 'info',
          text: 'panel.update(nodes) replaces this view immediately; unload removes the rail.',
        },
      ],
    )
    return panel.dispose
  })
}

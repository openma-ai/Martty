/** Profile sibling that selects the linked ACP + Creator dependency stack. */

import { apply as applyAcpClient } from './acp-client.js'
import { resolveStackedAgent } from './agent.js'

export const name = 'dsh-tui-profile-acp-client'
export const inject = []

export function apply(ctx) {
  return applyAcpClient(ctx, { agent: resolveStackedAgent() })
}

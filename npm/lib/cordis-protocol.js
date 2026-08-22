/** ACP extension contract for DSH Cordis client capabilities. */

export const CORDIS_PROTOCOL = 0

export const CORDIS_CAPABILITY = Object.freeze({ protocol: CORDIS_PROTOCOL })

export const CORDIS_METHODS = Object.freeze({
  inspectSync: '_dsh/cordis/inspect/sync',
  inspectResolve: '_dsh/cordis/inspect/resolve',
  inspectQuery: '_dsh/cordis/inspect/query',
  inspectQueryResolved: '_dsh/cordis/inspect/query-resolved',
  runHost: '_dsh/cordis/run/host',
  getClientCode: '_dsh/cordis/run/client-code',
  resolveRequestRun: '_dsh/cordis/run/resolve',
  requestRun: '_dsh/cordis/run/request',
  requestRunResolved: '_dsh/cordis/run/request-resolved',
  userRun: '_dsh/cordis/run/user',
  settleUserRun: '_dsh/cordis/run/settle',
  pluginInvoke: '_dsh/cordis/plugin/invoke',
  pluginsList: '_dsh/cordis/plugins/list',
  pluginStart: '_dsh/cordis/plugins/start',
  pluginStop: '_dsh/cordis/plugins/stop',
  pluginRetract: '_dsh/cordis/plugins/retract',
  approvalsUpdate: '_dsh/cordis/tui/approvals/update',
  approvalRespond: '_dsh/cordis/tui/approvals/respond',
  uiUpdate: '_dsh/cordis/tui/ui/update',
  uiSelected: '_dsh/cordis/tui/ui/selected',
  themeUpdate: '_dsh/cordis/tui/theme/update',
  themeRemove: '_dsh/cordis/tui/theme/remove',
  themeSelected: '_dsh/cordis/tui/theme/selected',
  slotsUpdate: '_dsh/cordis/tui/slots/update',
  commandsUpdate: '_dsh/cordis/tui/commands/update',
  commandInvoke: '_dsh/cordis/tui/commands/invoke',
  overlayUpdate: '_dsh/cordis/tui/overlay/update',
  overlayEvent: '_dsh/cordis/tui/overlay/event',
  sessionConfigSet: '_dsh/cordis/tui/session-config/set',
})

/**
 * Read the negotiated DSH Cordis capability from an ACP initialize result.
 * @param {unknown} result
 * @returns {{ protocol: number } | null}
 */
export function readCordisCapability(result) {
  if (result === null || typeof result !== 'object' || Array.isArray(result)) return null
  const capabilities = result.agentCapabilities
  if (capabilities === null || typeof capabilities !== 'object' || Array.isArray(capabilities)) return null
  const meta = capabilities._meta
  if (meta === null || typeof meta !== 'object' || Array.isArray(meta)) return null
  const dsh = meta.dsh
  if (dsh === null || typeof dsh !== 'object' || Array.isArray(dsh)) return null
  const cordis = dsh.cordis
  if (cordis === null || typeof cordis !== 'object' || Array.isArray(cordis)) return null
  return cordis.protocol === CORDIS_PROTOCOL ? { protocol: CORDIS_PROTOCOL } : null
}

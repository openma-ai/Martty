/**
 * Host-side compatibility for newer @deepseek-ai/dsh releases.
 *
 * Two adapter-vs-dsh seams broke when dsh moved past 0.1.0-rc.7:
 *
 * 1. Permission switches: dsh-permission-presets >= 0.1.1-rc.1 records the
 *    origin of every live permission switch in the session log, and
 *    dsh-session >= 0.1.1-rc.1 rejects event data that is not losslessly
 *    JSON-serializable (an object property holding `undefined` fails that
 *    check). The bundled ACP adapter calls
 *    `permissionPresets.apply(session, modeId, setApproval)` with no origin,
 *    so on the new dsh a switch appended `{ preset, origin: undefined }` and
 *    every `session/set_mode` failed with
 *    `session event "permission/preset" carries non-JSON-serializable data`.
 *    We default the omitted origin to `"selection"` — the same origin the
 *    host's own `/permission` command uses.
 *
 * 2. Harness command dispatch: dsh-commands >= 0.1.0-rc.8 changed
 *    `execute(agent, line, signal)` to `execute(agent, line, images, signal)`
 *    (composer image attachments). The bundled ACP adapter still calls with
 *    three arguments, so `signal` arrives `undefined` and every harness
 *    command — `/plan` and `/plan off` via the `collaboration_mode` config
 *    option, plus user-typed `/compact`, `/goal`, `/permission`, `/plan` —
 *    failed with `Cannot read properties of undefined (reading 'aborted')`.
 *    We detect the legacy three-argument call shape and insert the empty
 *    images batch; the legacy two-argument shape passes through untouched.
 */

export const name = 'dsh-tui-permission-compat'
export const inject = ['permissionPresets', 'commands']

export function apply(ctx) {
  const permission = ctx.permissionPresets
  const originalApply = permission?.apply
  if (typeof originalApply === 'function') {
    permission.apply = function applyWithOrigin(session, preset, setApproval, origin) {
      return originalApply.call(this, session, preset, setApproval, origin ?? 'selection')
    }
  }

  const commands = ctx.commands
  const originalExecute = commands?.execute
  if (typeof originalExecute === 'function') {
    // `Function.length` distinguishes the legacy three-parameter signature
    // from the rc.8+ four-parameter one.
    const legacy = originalExecute.length < 4
    commands.execute = function executeWithImages(agent, line, images, signal) {
      if (legacy) return originalExecute.call(this, agent, line, images)
      if (arguments.length < 4 && images !== undefined && typeof images.aborted === 'boolean') {
        return originalExecute.call(this, agent, line, [], images)
      }
      return originalExecute.call(this, agent, line, images ?? [], signal)
    }
  }
}

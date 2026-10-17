/**
 * Martty's dsh compatibility policy: a minimum-version floor, never an
 * exact-version allow-list.
 *
 * - Below the floor (`engines.dsh` in package.json — the field dsh's own
 *   plugin loader evaluates for bundles): not attempted; the user gets a
 *   message naming the installed version, the floor, and what to do.
 * - At or above the floor: attempted. API differences are handled by feature
 *   detection where they are used.
 * - Newer than {@link TESTED_DSH_THROUGH}: attempted with a warning only.
 *
 * A dependency that still declares an exact-version list (dsh-acp <= 0.4.38)
 * is evaluated the same way through {@link requirementOf}: its declared range
 * is what it will enforce at mount time.
 */

import { readFileSync } from 'node:fs'
import semver from 'semver'

/** Highest dsh CI exercises Martty with (the `next` leg of the dsh-plugin matrix). */
export const TESTED_DSH_THROUGH = '0.2.1-rc.2'

const ownManifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))

/** The floor this Martty declares for its hosting dsh. */
export const DSH_FLOOR = ownManifest.engines?.dsh ?? '*'

export function satisfiesRange(version, range) {
  if (typeof version !== 'string' || semver.valid(version) === null) return false
  if (typeof range !== 'string' || range.trim() === '') return false
  return semver.satisfies(version, range, { includePrerelease: true })
}

export function isUntested(version, testedThrough = TESTED_DSH_THROUGH) {
  return typeof version === 'string' && semver.valid(version) !== null && semver.gt(version, testedThrough)
}

/**
 * The dsh range a plugin manifest declares, mirroring dsh's own rule: an
 * `engines.dsh` range decides alone, otherwise the `@deepseek-ai/dsh` peer.
 * @returns {string | undefined}
 */
export function requirementOf(manifest) {
  const engines = manifest?.engines?.dsh
  if (typeof engines === 'string') return engines
  const peer = manifest?.peerDependencies?.['@deepseek-ai/dsh']
  return typeof peer === 'string' ? peer : undefined
}

export function untestedWarning(version, where) {
  return `martty: dsh ${version}${where ? ` (${where})` : ''} is newer than the newest dsh Martty was tested with (${TESTED_DSH_THROUGH}); continuing.`
}

import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const packageRoot = path.resolve(import.meta.dirname, '../npm')

function runDsh(home, args) {
  return spawnSync('dsh', args, {
    cwd: packageRoot,
    env: { ...process.env, DSH_HOME: home },
    encoding: 'utf8',
  })
}

test('one TUI install adds no duplicate Base loader entries', () => {
  const home = mkdtempSync(path.join(tmpdir(), 'dsh-tui-install-'))
  try {
    const install = runDsh(home, [
      'plugin', '--profile', 'tui', 'add', `link:${packageRoot}`,
    ])
    assert.equal(install.status, 0, install.stderr)

    const dump = runDsh(home, ['--profile', 'tui', '--dump-config'])
    assert.equal(dump.status, 0, dump.stderr)
    assert.equal(dump.stdout.match(/^- id: timer$/gm)?.length, 1)
    assert.equal(dump.stdout.match(/^- id: hmr$/gm)?.length, 1)
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
})

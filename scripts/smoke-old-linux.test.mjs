import assert from 'node:assert/strict'
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const smoke = path.join(repoRoot, 'scripts', 'smoke-old-linux.mjs')

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-old-linux-'))
  const runtime = path.join(root, 'container-runtime')
  const binary = path.join(root, 'dsh-tui')
  const argsLog = path.join(root, 'args.log')
  writeFileSync(binary, 'fixture')
  writeFileSync(runtime, `#!/bin/sh
printf '%s\n' "$@" > "$DSH_TUI_TEST_CONTAINER_ARGS"
exit "\${DSH_TUI_TEST_CONTAINER_STATUS:-0}"
`)
  chmodSync(runtime, 0o755)
  return { root, runtime, binary, argsLog }
}

test('runs the packaged binary in an Ubuntu 20.04 amd64 container', (t) => {
  const item = fixture()
  t.after(() => rmSync(item.root, { recursive: true, force: true }))

  const result = spawnSync(process.execPath, [smoke, item.binary], {
    encoding: 'utf8',
    env: {
      ...process.env,
      DSH_TUI_CONTAINER_BIN: item.runtime,
      DSH_TUI_TEST_CONTAINER_ARGS: item.argsLog,
    },
  })

  assert.equal(result.status, 0, result.stderr)
  assert.deepEqual(readFileSync(item.argsLog, 'utf8').trim().split('\n'), [
    'run',
    '--rm',
    '--platform',
    'linux/amd64',
    '--network',
    'none',
    '--volume',
    `${path.resolve(item.binary)}:/usr/local/bin/dsh-tui:ro`,
    'ubuntu:20.04',
    '/usr/local/bin/dsh-tui',
    '--help',
  ])
})

test('runs the packaged binary in an Ubuntu 20.04 arm64 container', (t) => {
  const item = fixture()
  t.after(() => rmSync(item.root, { recursive: true, force: true }))

  const result = spawnSync(process.execPath, [smoke, item.binary], {
    encoding: 'utf8',
    env: {
      ...process.env,
      DSH_TUI_CONTAINER_BIN: item.runtime,
      DSH_TUI_TEST_CONTAINER_ARGS: item.argsLog,
      DSH_TUI_SMOKE_PLATFORM: 'linux/arm64',
    },
  })

  assert.equal(result.status, 0, result.stderr)
  assert.deepEqual(readFileSync(item.argsLog, 'utf8').trim().split('\n'), [
    'run',
    '--rm',
    '--platform',
    'linux/arm64',
    '--network',
    'none',
    '--volume',
    `${path.resolve(item.binary)}:/usr/local/bin/dsh-tui:ro`,
    'ubuntu:20.04',
    '/usr/local/bin/dsh-tui',
    '--help',
  ])
})

test('fails when the packaged binary cannot run in old Linux', (t) => {
  const item = fixture()
  t.after(() => rmSync(item.root, { recursive: true, force: true }))

  const result = spawnSync(process.execPath, [smoke, item.binary], {
    encoding: 'utf8',
    env: {
      ...process.env,
      DSH_TUI_CONTAINER_BIN: item.runtime,
      DSH_TUI_TEST_CONTAINER_ARGS: item.argsLog,
      DSH_TUI_TEST_CONTAINER_STATUS: '126',
    },
  })

  assert.equal(result.status, 1)
  assert.match(result.stderr, /old Linux smoke test exited with status 126/)
})

#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import path from 'node:path'

const binary = process.argv[2]
const runtime = process.env.DSH_TUI_CONTAINER_BIN || 'docker'

try {
  if (!binary) throw new Error('usage: smoke-old-linux.mjs <binary>')

  const mountedBinary = '/usr/local/bin/dsh-tui'
  const result = spawnSync(runtime, [
    'run',
    '--rm',
    '--platform',
    'linux/amd64',
    '--network',
    'none',
    '--volume',
    `${path.resolve(binary)}:${mountedBinary}:ro`,
    'ubuntu:20.04',
    mountedBinary,
    '--help',
  ], { stdio: 'inherit' })

  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`old Linux smoke test exited with status ${result.status}`)
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

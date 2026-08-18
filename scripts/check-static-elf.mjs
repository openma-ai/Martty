#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import path from 'node:path'

const binary = process.argv[2]
const readelf = process.env.DSH_TUI_READELF_BIN || 'readelf'

try {
  if (!binary) throw new Error('usage: check-static-elf.mjs <binary>')

  const result = spawnSync(readelf, ['--program-headers', binary], { encoding: 'utf8' })
  if (result.error) throw result.error
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `readelf exited with status ${result.status}`)
  }
  if (/\bINTERP\b|Requesting program interpreter/i.test(result.stdout)) {
    throw new Error(`${path.basename(binary)} is dynamically linked and requires a program interpreter`)
  }
  process.stdout.write(`static ELF verified: ${binary}\n`)
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}

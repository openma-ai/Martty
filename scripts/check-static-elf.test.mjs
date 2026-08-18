import assert from 'node:assert/strict'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const checker = path.join(repoRoot, 'scripts', 'check-static-elf.mjs')

function fixture(programHeaders) {
  const root = mkdtempSync(path.join(tmpdir(), 'dsh-tui-static-elf-'))
  const readelf = path.join(root, 'readelf')
  const binary = path.join(root, 'dsh-tui')
  writeFileSync(binary, 'fixture')
  writeFileSync(readelf, `#!/bin/sh
printf '%s\n' "${programHeaders}"
`)
  chmodSync(readelf, 0o755)
  return { root, readelf, binary }
}

test('accepts a static ELF binary without a program interpreter', (t) => {
  const item = fixture('Elf file type is EXEC (Executable file)')
  t.after(() => rmSync(item.root, { recursive: true, force: true }))

  const result = spawnSync(process.execPath, [checker, item.binary], {
    encoding: 'utf8',
    env: { ...process.env, DSH_TUI_READELF_BIN: item.readelf },
  })

  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /static ELF verified/)
})

test('rejects an ELF binary that needs the glibc program interpreter', (t) => {
  const item = fixture('INTERP         0x0000000000000350\n      [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]')
  t.after(() => rmSync(item.root, { recursive: true, force: true }))

  const result = spawnSync(process.execPath, [checker, item.binary], {
    encoding: 'utf8',
    env: { ...process.env, DSH_TUI_READELF_BIN: item.readelf },
  })

  assert.equal(result.status, 1)
  assert.match(result.stderr, /dynamically linked/)
})

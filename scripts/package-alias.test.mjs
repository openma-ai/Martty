import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  mkdirSync,
  mkdtempSync,
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

const script = path.resolve(import.meta.dirname, 'package-alias.mjs')

test('creates a thin Martty package with platform optional dependencies', (t) => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-package-alias-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'legacy')
  const destination = path.join(root, 'martty')
  mkdirSync(path.join(source, 'lib'), { recursive: true })
  mkdirSync(path.join(source, 'vendor', 'linux-x64'), { recursive: true })
  writeFileSync(path.join(source, 'package.json'), JSON.stringify({
    name: '@openma/deepseek-harness-tui',
    version: '1.2.3',
  }, null, 2) + '\n')
  writeFileSync(
    path.join(source, 'cordis.patch.yml'),
    "name: '@openma/deepseek-harness-tui/acp-host'\n",
  )
  writeFileSync(
    path.join(source, 'lib', 'meta.js'),
    "export const source = '@openma/deepseek-harness-tui/creator-overlay'\n",
  )
  const binary = Buffer.from([0, 255, 1, 254])
  writeFileSync(path.join(source, 'vendor', 'linux-x64', 'martty'), binary)

  const result = spawnSync(process.execPath, [script, source, destination, 'martty'], {
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr)
  assert.deepEqual(
    JSON.parse(readFileSync(path.join(destination, 'package.json'), 'utf8')),
    {
      name: 'martty',
      version: '1.2.3',
      optionalDependencies: {
        '@openma/martty-darwin-arm64': '1.2.3',
        '@openma/martty-darwin-x64': '1.2.3',
        '@openma/martty-linux-arm64': '1.2.3',
        '@openma/martty-linux-x64': '1.2.3',
        '@openma/martty-win32-x64': '1.2.3',
      },
    },
  )
  assert.match(readFileSync(path.join(destination, 'cordis.patch.yml'), 'utf8'), /martty\/acp-host/)
  assert.match(readFileSync(path.join(destination, 'lib', 'meta.js'), 'utf8'), /martty\/creator-overlay/)
  assert.equal(existsSync(path.join(destination, 'vendor')), false)
  assert.deepEqual(readFileSync(path.join(source, 'vendor', 'linux-x64', 'martty')), binary)
  assert.equal(
    JSON.parse(readFileSync(path.join(source, 'package.json'), 'utf8')).name,
    '@openma/deepseek-harness-tui',
  )
})
